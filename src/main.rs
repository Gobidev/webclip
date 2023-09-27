use actix_files::Files;
use actix_web::{middleware, web::Data, App, HttpResponse, HttpServer, Responder};
use tokio::sync::Mutex;

struct AppState(pub Mutex<String>);

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

#[actix_web::main]
async fn main() {
    let data = Data::new(AppState(Mutex::new(String::new())));
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .service(get_clipboard)
            .service(update_clipboard)
            .service(Files::new("/", "./web/dist").index_file("index.html"))
            .app_data(data.clone())
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

#[actix_web::post("/clipboard")]
async fn update_clipboard(data: Data<AppState>, body: String) -> impl Responder {
    if body.len() > MAX_SIZE {
        return HttpResponse::BadRequest();
    }
    *data.0.lock().await = body;
    HttpResponse::Ok()
}

#[actix_web::get("/clipboard")]
async fn get_clipboard(data: Data<AppState>) -> String {
    data.0.lock().await.clone()
}
