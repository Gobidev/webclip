use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use actix::prelude::*;
use actix_files::Files;
use actix_web::{
    middleware,
    web::{self, Data},
    App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
};
use actix_web_actors::ws;
use tokio::sync::Mutex;

struct AppState {
    clipboard_content: Arc<Mutex<String>>,
    connections: Arc<Mutex<Vec<Addr<ClipboardWebsocket>>>>,
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
    shared_data: web::Data<AppState>,
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
        let connections = self.shared_data.connections.clone();
        let addr = ctx.address().clone();
        tokio::spawn(async move {
            connections.lock().await.push(addr);
        });
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let connections = self.shared_data.connections.clone();
        let addr = ctx.address().clone();
        tokio::spawn(async move {
            let index = connections
                .lock()
                .await
                .iter()
                .position(|a| *a == addr)
                .unwrap();
            connections.lock().await.remove(index);
        });
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
                let data = self.shared_data.clipboard_content.clone();
                let connections = self.shared_data.connections.clone();
                tokio::spawn(async move {
                    *data.lock().await = text.to_string();
                    for connection in connections.lock().await.iter() {
                        connection.do_send(Message(text.to_string()));
                    }
                });
            }
            _ => (),
        }
    }
}

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    clipboard_content: Data<AppState>,
) -> Result<impl Responder, Error> {
    ws::start(
        ClipboardWebsocket {
            heartbeat: Instant::now(),
            shared_data: clipboard_content.clone(),
        },
        &req,
        stream,
    )
}

#[actix_web::post("/clipboard")]
async fn update_clipboard(data: Data<AppState>, body: String) -> impl Responder {
    if body.len() > MAX_SIZE {
        return HttpResponse::BadRequest();
    }
    *data.clipboard_content.lock().await = body.clone();
    for connection in data.connections.lock().await.iter() {
        connection.do_send(Message(body.clone()));
    }
    HttpResponse::Ok()
}

#[actix_web::get("/clipboard")]
async fn get_clipboard(data: Data<AppState>) -> String {
    data.clipboard_content.lock().await.clone()
}

#[actix_web::main]
async fn main() {
    let clipboard_content = Data::new(AppState {
        clipboard_content: Arc::new(Mutex::new(String::new())),
        connections: Arc::new(Mutex::new(Vec::new())),
    });
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .service(get_clipboard)
            .service(update_clipboard)
            .service(web::resource("/ws").route(web::get().to(ws_route)))
            .service(Files::new("/", "./web/dist").index_file("index.html"))
            .app_data(clipboard_content.clone())
    })
    .bind((
        dotenvy::var("WEBCLIP_BIND_ADDRESS").unwrap_or("0.0.0.0".to_string()),
        dotenvy::var("WEBCLIP_BIND_PORT")
            .map(|port| port.parse::<u16>().expect("Invalid port"))
            .unwrap_or(9257),
    ))
    .unwrap()
    .run()
    .await
    .unwrap();
}
