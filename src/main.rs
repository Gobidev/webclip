use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use actix::prelude::*;
use actix_files::Files;
use actix_web::{
    http::header,
    middleware,
    web::{self, Data},
    App, Error, HttpRequest, HttpResponse, HttpServer,
};
use actix_web_actors::ws;
use log::info;

struct AppState {
    clipboard: Mutex<String>,
    connections: Mutex<Vec<Addr<ClipboardWebsocket>>>,
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

struct ClipboardWebsocket {
    heartbeat: Instant,
    state: Data<AppState>,
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
        ctx.text(self.state.clipboard.lock().unwrap().clone());
        info!("Websocket connection started");
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        self.state.connections.lock().unwrap().retain(|a| *a != addr);
        info!("Websocket connection stopped");
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
                *self.state.clipboard.lock().unwrap() = text.clone();
                let sender = ctx.address();
                for connection in self.state.connections.lock().unwrap().iter() {
                    if *connection != sender {
                        connection.do_send(Message(text.clone()));
                    }
                }
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
    *data.clipboard.lock().unwrap() = body.clone();
    for connection in data.connections.lock().unwrap().iter() {
        connection.do_send(Message(body.clone()));
    }
    HttpResponse::Ok().finish()
}

#[actix_web::get("/clipboard")]
async fn get_clipboard(data: Data<AppState>) -> String {
    data.clipboard.lock().unwrap().clone()
}

#[actix_web::get("/config.js")]
async fn config_js() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/javascript")
        .insert_header(("Cache-Control", "no-cache"))
        .body(format!("window.WEBCLIP_MAX_SIZE = {MAX_SIZE};\n"))
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
    let state = Data::new(AppState {
        clipboard: Mutex::new(String::new()),
        connections: Mutex::new(Vec::new()),
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
            .service(config_js)
            .service(theme_css)
            .service(web::resource("/ws").route(web::get().to(ws_route)))
            .service(Files::new("/", "./web/static").index_file("index.html"))
    })
    .bind((address, port))
    .unwrap()
    .run()
    .await
    .unwrap();
}
