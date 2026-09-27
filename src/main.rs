use std::{
    collections::HashMap,
    str::FromStr,
    sync::Mutex,
    time::{Duration, Instant},
};

use actix::prelude::*;
use actix_files::Files;
use actix_web::{
    http::header,
    middleware,
    rt,
    web::{self, Data},
    App, Error, HttpRequest, HttpResponse, HttpServer,
};
use actix_web_actors::ws;
use log::{debug, info, warn};
use rand::Rng;

struct Clipboard {
    value: String,
    last_change: Instant,
}

struct PrivateRoom {
    value: String,
    last_change: Instant,
    connections: Vec<Addr<PrivateWebsocket>>,
}

struct PrivateConfig {
    enabled: bool,
    ttl: Duration,
    pin_length: usize,
    max_rooms: usize,
    max_attempts: u32,
    attempt_window: Duration,
}

struct AttemptWindow {
    failures: u32,
    started: Instant,
}

struct AppState {
    clipboard: Mutex<Clipboard>,
    private_rooms: Mutex<HashMap<String, PrivateRoom>>,
    private_attempts: Mutex<HashMap<String, AttemptWindow>>,
    private_config: PrivateConfig,
    connections: Mutex<Vec<Addr<ClipboardWebsocket>>>,
}

fn env_parse<T: FromStr>(name: &str, default: T) -> T {
    match dotenvy::var(name) {
        Ok(value) => match value.trim().parse() {
            Ok(parsed) => parsed,
            Err(_) => {
                warn!("config: invalid value for {name}, using default");
                default
            }
        },
        Err(_) => default,
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    match dotenvy::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => {
                warn!("config: invalid value for {name}, using default");
                default
            }
        },
        Err(_) => default,
    }
}

impl PrivateConfig {
    fn from_env() -> Self {
        let pin_length = env_parse("WEBCLIP_PRIVATE_PIN_LENGTH", 4usize).clamp(3, 12);
        let max_rooms = env_parse("WEBCLIP_PRIVATE_MAX_ROOMS", 100usize).max(1);
        Self {
            enabled: env_bool("WEBCLIP_PRIVATE_ENABLED", true),
            ttl: Duration::from_secs(env_parse("WEBCLIP_PRIVATE_TTL", 30 * 60u64).max(30)),
            pin_length,
            max_rooms,
            max_attempts: env_parse("WEBCLIP_PRIVATE_MAX_ATTEMPTS", 10u32).max(1),
            attempt_window: Duration::from_secs(60),
        }
    }
}

const fn parse_int(s: &str) -> usize {
    let mut bytes = s.as_bytes();
    let mut val = 0;
    while let [byte, rest @ ..] = bytes {
        assert!(b'0' <= *byte && *byte <= b'9', "invalid digit");
        val = val * 10 + (*byte - b'0') as usize;
        bytes = rest;
    }
    val
}

const MAX_SIZE: usize = match option_env!("WEBCLIP_MAX_SIZE") {
    Some(size) => parse_int(size),
    None => 100_000,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_LOG_INTERVAL: Duration = Duration::from_secs(60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const CLIPBOARD_TTL: Duration = Duration::from_secs(12 * 60 * 60);

struct ClipboardWebsocket {
    heartbeat: Instant,
    state: Data<AppState>,
    ip: String,
    user_agent: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Message(pub String);

impl ClipboardWebsocket {
    fn heartbeat(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT {
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}

impl Actor for ClipboardWebsocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.heartbeat(ctx);
        let mut connections = self.state.connections.lock().unwrap();
        connections.push(ctx.address());
        let clients = connections.len();
        ctx.text(self.state.clipboard.lock().unwrap().value.clone());
        info!(
            "clipboard: websocket connected (ip={}, user-agent=\"{}\", clients={})",
            self.ip, self.user_agent, clients
        );
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        let mut connections = self.state.connections.lock().unwrap();
        connections.retain(|a| *a != addr);
        let clients = connections.len();
        info!(
            "clipboard: websocket disconnected (ip={}, clients={})",
            self.ip, clients
        );
    }
}

impl Handler<Message> for ClipboardWebsocket {
    type Result = ();

    fn handle(&mut self, msg: Message, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClipboardWebsocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        match msg {
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => self.heartbeat = Instant::now(),
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Text(text) => {
                let text = text.to_string();
                {
                    let mut clipboard = self.state.clipboard.lock().unwrap();
                    clipboard.value = text.clone();
                    clipboard.last_change = Instant::now();
                }
                let sender = ctx.address();
                for connection in self.state.connections.lock().unwrap().iter() {
                    if *connection != sender {
                        connection.do_send(Message(text.clone()));
                    }
                }
                debug!(
                    "clipboard: updated via websocket (ip={}, bytes={})",
                    self.ip,
                    text.len()
                );
            }
            _ => (),
        }
    }
}

struct PrivateWebsocket {
    heartbeat: Instant,
    opened_at: Instant,
    state: Data<AppState>,
    ip: String,
    user_agent: String,
    pin: Option<String>,
}

impl PrivateWebsocket {
    fn heartbeat(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT
                || (act.pin.is_none() && act.opened_at.elapsed() > AUTH_TIMEOUT)
            {
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }

    fn authenticate(&mut self, pin: String, ctx: &mut <Self as Actor>::Context) {
        let config = &self.state.private_config;
        let join_key = attempts_key("join", &self.ip);

        let blocked = {
            let attempts = self.state.private_attempts.lock().unwrap();
            retry_after(&attempts, &join_key, config)
        };
        if let Some(retry) = blocked {
            info!(
                "private: join rate limited (ip={}, retry={retry}s)",
                self.ip
            );
            ctx.close(Some(ws::CloseReason {
                code: ws::CloseCode::Policy,
                description: Some("rate limited".to_owned()),
            }));
            ctx.stop();
            return;
        }

        let pin = pin.trim().to_owned();
        let mut peers = 0;
        let joined = {
            let mut rooms = self.state.private_rooms.lock().unwrap();
            match rooms.get_mut(&pin) {
                Some(room) => {
                    room.connections.push(ctx.address());
                    peers = room.connections.len();
                    Some(room.value.clone())
                }
                None => None,
            }
        };

        let Some(value) = joined else {
            let failures = {
                let mut attempts = self.state.private_attempts.lock().unwrap();
                record_attempt(&mut attempts, &join_key, config.attempt_window)
            };
            info!("private: join failed (ip={}, failures={failures})", self.ip);
            ctx.close(Some(ws::CloseReason {
                code: ws::CloseCode::Policy,
                description: Some("invalid pin".to_owned()),
            }));
            ctx.stop();
            return;
        };

        self.state
            .private_attempts
            .lock()
            .unwrap()
            .remove(&join_key);
        self.pin = Some(pin);
        info!(
            "private: peer joined (ip={}, user-agent=\"{}\", peers={peers})",
            self.ip, self.user_agent
        );
        ctx.text(value);
    }
}

impl Actor for PrivateWebsocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.heartbeat(ctx);
        debug!(
            "private: websocket connected, awaiting pin (ip={}, user-agent=\"{}\")",
            self.ip, self.user_agent
        );
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let Some(pin) = self.pin.take() else {
            return;
        };
        let mut rooms = self.state.private_rooms.lock().unwrap();
        let Some(room) = rooms.get_mut(&pin) else {
            return;
        };
        let addr = ctx.address();
        room.connections.retain(|a| *a != addr);
        let peers = room.connections.len();
        if peers == 0 {
            room.last_change = Instant::now();
        }
        info!(
            "private: peer left (ip={}, peers={peers}, rooms={})",
            self.ip,
            rooms.len()
        );
    }
}

impl Handler<Message> for PrivateWebsocket {
    type Result = ();

    fn handle(&mut self, msg: Message, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for PrivateWebsocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        match msg {
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => self.heartbeat = Instant::now(),
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Text(text) => {
                let text = text.to_string();
                let Some(pin) = self.pin.clone() else {
                    self.authenticate(text, ctx);
                    return;
                };
                {
                    let mut rooms = self.state.private_rooms.lock().unwrap();
                    let Some(room) = rooms.get_mut(&pin) else {
                        return;
                    };
                    room.value = text.clone();
                    room.last_change = Instant::now();
                    let sender = ctx.address();
                    for connection in &room.connections {
                        if *connection != sender {
                            connection.do_send(Message(text.clone()));
                        }
                    }
                }
                debug!(
                    "private: updated via websocket (ip={}, bytes={})",
                    self.ip,
                    text.len()
                );
            }
            _ => (),
        }
    }
}

fn same_origin(req: &HttpRequest) -> bool {
    let Some(origin) = req.headers().get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    matches!(origin.split("://").nth(1), Some(host) if host == req.connection_info().host())
}

fn client_ip(req: &HttpRequest) -> String {
    req.connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_owned()
}

fn user_agent(req: &HttpRequest) -> String {
    req.headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned()
}

fn log_status(state: &AppState) {
    let clients = state.connections.lock().unwrap().len();
    let chars = state.clipboard.lock().unwrap().value.chars().count();
    let (rooms, peers) = {
        let rooms = state.private_rooms.lock().unwrap();
        (
            rooms.len(),
            rooms
                .values()
                .map(|room| room.connections.len())
                .sum::<usize>(),
        )
    };
    if clients == 0 && chars == 0 && rooms == 0 {
        return;
    }
    let percent = chars as f64 / MAX_SIZE as f64 * 100.0;
    info!("clipboard: status (clients={clients}, filled={chars}/{MAX_SIZE} chars, {percent:.2}%, private_rooms={rooms}, private_peers={peers})");
}

fn clear_if_stale(state: &AppState) -> bool {
    let stale = {
        let mut clipboard = state.clipboard.lock().unwrap();
        if clipboard.value.is_empty() || clipboard.last_change.elapsed() < CLIPBOARD_TTL {
            return false;
        }
        clipboard.value.clear();
        clipboard.last_change = Instant::now();
        true
    };
    for connection in state.connections.lock().unwrap().iter() {
        connection.do_send(Message(String::new()));
    }
    stale
}

fn clear_expired_private_rooms(state: &AppState) -> usize {
    let ttl = state.private_config.ttl;
    let mut rooms = state.private_rooms.lock().unwrap();
    let before = rooms.len();
    rooms.retain(|_, room| !room.connections.is_empty() || room.last_change.elapsed() < ttl);
    before - rooms.len()
}

fn clear_stale_attempts(state: &AppState) {
    let window = state.private_config.attempt_window;
    state
        .private_attempts
        .lock()
        .unwrap()
        .retain(|_, entry| entry.started.elapsed() < window);
}

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    state: Data<AppState>,
) -> Result<HttpResponse, Error> {
    if !same_origin(&req) {
        return Ok(HttpResponse::Forbidden().finish());
    }
    ws::start(
        ClipboardWebsocket {
            heartbeat: Instant::now(),
            state,
            ip: client_ip(&req),
            user_agent: user_agent(&req),
        },
        &req,
        stream,
    )
}

async fn ws_private_route(
    req: HttpRequest,
    stream: web::Payload,
    state: Data<AppState>,
) -> Result<HttpResponse, Error> {
    if !state.private_config.enabled {
        return Ok(HttpResponse::NotFound().finish());
    }
    if !same_origin(&req) {
        return Ok(HttpResponse::Forbidden().finish());
    }
    ws::start(
        PrivateWebsocket {
            heartbeat: Instant::now(),
            opened_at: Instant::now(),
            state,
            ip: client_ip(&req),
            user_agent: user_agent(&req),
            pin: None,
        },
        &req,
        stream,
    )
}

#[actix_web::post("/clipboard")]
async fn update_clipboard(req: HttpRequest, data: Data<AppState>, body: String) -> HttpResponse {
    if !same_origin(&req) {
        return HttpResponse::Forbidden().finish();
    }
    if body.chars().count() > MAX_SIZE {
        return HttpResponse::BadRequest().finish();
    }
    *data.clipboard.lock().unwrap() = Clipboard {
        value: body.clone(),
        last_change: Instant::now(),
    };
    for connection in data.connections.lock().unwrap().iter() {
        connection.do_send(Message(body.clone()));
    }
    debug!(
        "clipboard: updated via http (ip={}, bytes={})",
        client_ip(&req),
        body.len()
    );
    HttpResponse::Ok().finish()
}

#[actix_web::get("/clipboard")]
async fn get_clipboard(data: Data<AppState>) -> String {
    data.clipboard.lock().unwrap().value.clone()
}

fn text_no_store(body: String, expires_in: u64) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .insert_header(("Cache-Control", "no-store"))
        .insert_header(("X-Expires-In", expires_in.to_string()))
        .body(body)
}

fn generate_pin(rooms: &HashMap<String, PrivateRoom>, length: usize) -> Option<String> {
    let mut rng = rand::thread_rng();
    for _ in 0..1000 {
        let pin: String = (0..length)
            .map(|_| char::from(b'0' + rng.gen_range(0u8..10)))
            .collect();
        if !rooms.contains_key(&pin) {
            return Some(pin);
        }
    }
    None
}

fn attempts_key(action: &str, ip: &str) -> String {
    format!("{action}:{ip}")
}

fn retry_after(
    attempts: &HashMap<String, AttemptWindow>,
    key: &str,
    config: &PrivateConfig,
) -> Option<u64> {
    let entry = attempts.get(key)?;
    let elapsed = entry.started.elapsed();
    if elapsed >= config.attempt_window || entry.failures < config.max_attempts {
        return None;
    }
    Some((config.attempt_window - elapsed).as_secs().max(1))
}

fn record_attempt(
    attempts: &mut HashMap<String, AttemptWindow>,
    key: &str,
    window: Duration,
) -> u32 {
    let entry = attempts.entry(key.to_owned()).or_insert(AttemptWindow {
        failures: 0,
        started: Instant::now(),
    });
    if entry.started.elapsed() >= window {
        entry.failures = 0;
        entry.started = Instant::now();
    }
    entry.failures += 1;
    entry.failures
}

#[actix_web::post("/private")]
async fn create_private_room(req: HttpRequest, data: Data<AppState>) -> HttpResponse {
    if !data.private_config.enabled {
        return HttpResponse::NotFound().finish();
    }
    if !same_origin(&req) {
        return HttpResponse::Forbidden().finish();
    }

    let ip = client_ip(&req);
    let config = &data.private_config;
    let create_key = attempts_key("create", &ip);

    let blocked = {
        let attempts = data.private_attempts.lock().unwrap();
        retry_after(&attempts, &create_key, config)
    };
    if let Some(retry) = blocked {
        return HttpResponse::TooManyRequests()
            .insert_header(("Retry-After", retry.to_string()))
            .finish();
    }

    let (pin, rooms) = {
        let mut rooms = data.private_rooms.lock().unwrap();
        if rooms.len() >= config.max_rooms {
            warn!(
                "private: rejected creation, storage is full (rooms={})",
                rooms.len()
            );
            return HttpResponse::ServiceUnavailable().finish();
        }
        let Some(pin) = generate_pin(&rooms, config.pin_length) else {
            warn!("private: rejected creation, no unique pin available");
            return HttpResponse::ServiceUnavailable().finish();
        };
        rooms.insert(
            pin.clone(),
            PrivateRoom {
                value: String::new(),
                last_change: Instant::now(),
                connections: Vec::new(),
            },
        );
        (pin, rooms.len())
    };
    {
        let mut attempts = data.private_attempts.lock().unwrap();
        record_attempt(&mut attempts, &create_key, config.attempt_window);
    }
    info!("private: room created (ip={ip}, rooms={rooms})");
    text_no_store(pin, config.ttl.as_secs())
}

#[actix_web::get("/config.js")]
async fn config_js(data: Data<AppState>) -> HttpResponse {
    let config = &data.private_config;
    HttpResponse::Ok()
        .content_type("application/javascript")
        .insert_header(("Cache-Control", "no-cache"))
        .body(format!(
            "window.WEBCLIP_MAX_SIZE = {MAX_SIZE};\nwindow.WEBCLIP_PRIVATE_ENABLED = {};\nwindow.WEBCLIP_PRIVATE_TTL = {};\nwindow.WEBCLIP_PRIVATE_PIN_LENGTH = {};\n",
            config.enabled,
            config.ttl.as_secs(),
            config.pin_length,
        ))
}

fn color_env(name: &str, default: &str) -> String {
    dotenvy::var(name)
        .ok()
        .filter(|color| {
            !color.is_empty()
                && color
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "#(),.% -/".contains(c))
        })
        .unwrap_or_else(|| default.to_string())
}

#[actix_web::get("/theme.css")]
async fn theme_css() -> HttpResponse {
    let primary = color_env("WEBCLIP_COLOR_PRIMARY", "#98971a");
    let background = color_env("WEBCLIP_COLOR_BACKGROUND", "#282828");
    let error = color_env("WEBCLIP_COLOR_ERROR", "#fb4934");
    let foreground = color_env("WEBCLIP_COLOR_FOREGROUND", "#ebdbb2");
    HttpResponse::Ok()
        .content_type("text/css")
        .insert_header(("Cache-Control", "no-cache"))
        .body(format!(
            ":root {{\n  --mdc-theme-primary: {primary};\n  --mdc-theme-background: {background};\n  --mdc-theme-error: {error};\n  --mdc-theme-on-surface: {foreground};\n}}\n"
        ))
}

const DEF_LOG_LEVEL: &str = "info";
const ENV_LOG_LEVEL: &str = "RUST_LOG";

#[actix_web::main]
async fn main() {
    dotenvy::dotenv().ok();
    if std::env::var(ENV_LOG_LEVEL).is_err() {
        std::env::set_var(ENV_LOG_LEVEL, DEF_LOG_LEVEL);
    }
    pretty_env_logger::init();
    let private_config = PrivateConfig::from_env();
    info!(
        "private: enabled={}, ttl={}s, pin_length={}, max_rooms={}",
        private_config.enabled,
        private_config.ttl.as_secs(),
        private_config.pin_length,
        private_config.max_rooms
    );
    let state = Data::new(AppState {
        clipboard: Mutex::new(Clipboard {
            value: String::new(),
            last_change: Instant::now(),
        }),
        private_rooms: Mutex::new(HashMap::new()),
        private_attempts: Mutex::new(HashMap::new()),
        private_config,
        connections: Mutex::new(Vec::new()),
    });
    rt::spawn({
        let state = state.clone();
        async move {
            loop {
                rt::time::sleep(STATUS_LOG_INTERVAL).await;
                log_status(&state);
            }
        }
    });
    rt::spawn({
        let state = state.clone();
        async move {
            loop {
                rt::time::sleep(CLEANUP_INTERVAL).await;
                if clear_if_stale(&state) {
                    info!(
                        "clipboard: cleared after {}h without changes",
                        CLIPBOARD_TTL.as_secs() / 3600
                    );
                }
                let expired = clear_expired_private_rooms(&state);
                if expired > 0 {
                    info!("private: cleared {expired} expired room(s)");
                }
                clear_stale_attempts(&state);
            }
        }
    });
    let address = dotenvy::var("WEBCLIP_BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = dotenvy::var("WEBCLIP_BIND_PORT")
        .map(|port| port.parse::<u16>().expect("Invalid port"))
        .unwrap_or(9257);
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .app_data(web::PayloadConfig::new(MAX_SIZE.saturating_mul(4)))
            .app_data(state.clone())
            .service(get_clipboard)
            .service(update_clipboard)
            .service(create_private_room)
            .service(config_js)
            .service(theme_css)
            .service(web::resource("/ws").route(web::get().to(ws_route)))
            .service(web::resource("/ws/private").route(web::get().to(ws_private_route)))
            .service(Files::new("/", "./web/static").index_file("index.html"))
    })
    .bind((address, port))
    .unwrap()
    .run()
    .await
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(ttl: Duration) -> PrivateConfig {
        PrivateConfig {
            enabled: true,
            ttl,
            pin_length: 4,
            max_rooms: 10,
            max_attempts: 2,
            attempt_window: Duration::from_secs(60),
        }
    }

    fn test_state(ttl: Duration) -> AppState {
        AppState {
            clipboard: Mutex::new(Clipboard {
                value: String::new(),
                last_change: Instant::now(),
            }),
            private_rooms: Mutex::new(HashMap::new()),
            private_attempts: Mutex::new(HashMap::new()),
            private_config: test_config(ttl),
            connections: Mutex::new(Vec::new()),
        }
    }

    fn room(value: &str, last_change: Instant) -> PrivateRoom {
        PrivateRoom {
            value: value.to_owned(),
            last_change,
            connections: Vec::new(),
        }
    }

    #[test]
    fn pins_have_the_configured_length_and_are_digits() {
        let pin = generate_pin(&HashMap::new(), 4).unwrap();
        assert_eq!(pin.len(), 4);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn pin_generation_fails_when_the_space_is_exhausted() {
        let mut rooms = HashMap::new();
        for digit in 0..10 {
            rooms.insert(digit.to_string(), room("", Instant::now()));
        }
        assert!(generate_pin(&rooms, 1).is_none());
    }

    #[test]
    fn rate_limiter_blocks_after_the_configured_attempts() {
        let config = test_config(Duration::from_secs(60));
        let mut attempts = HashMap::new();
        assert!(retry_after(&attempts, "ip", &config).is_none());
        record_attempt(&mut attempts, "ip", config.attempt_window);
        assert!(retry_after(&attempts, "ip", &config).is_none());
        record_attempt(&mut attempts, "ip", config.attempt_window);
        assert!(retry_after(&attempts, "ip", &config).is_some());
    }

    #[test]
    fn expired_private_rooms_are_removed() {
        let state = test_state(Duration::from_secs(60));
        let old = Instant::now()
            .checked_sub(Duration::from_secs(120))
            .unwrap();
        state
            .private_rooms
            .lock()
            .unwrap()
            .insert("1234".to_owned(), room("secret", old));
        assert_eq!(clear_expired_private_rooms(&state), 1);
        assert!(state.private_rooms.lock().unwrap().is_empty());
    }

    #[test]
    fn fresh_private_rooms_are_kept() {
        let state = test_state(Duration::from_secs(60));
        state
            .private_rooms
            .lock()
            .unwrap()
            .insert("1234".to_owned(), room("secret", Instant::now()));
        assert_eq!(clear_expired_private_rooms(&state), 0);
        assert_eq!(state.private_rooms.lock().unwrap().len(), 1);
    }

    #[test]
    fn stale_attempt_windows_are_cleared() {
        let state = test_state(Duration::from_secs(60));
        state.private_attempts.lock().unwrap().insert(
            "ip".to_owned(),
            AttemptWindow {
                failures: 5,
                started: Instant::now()
                    .checked_sub(Duration::from_secs(120))
                    .unwrap(),
            },
        );
        clear_stale_attempts(&state);
        assert!(state.private_attempts.lock().unwrap().is_empty());
    }
}
