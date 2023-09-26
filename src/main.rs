use actix_files::Files;
use actix_web::{web::Data, App, HttpResponse, HttpServer, Responder};
use tokio::sync::Mutex;

struct AppState(pub Mutex<String>);

#[actix_web::main]
async fn main() {
    let data = Data::new(AppState(Mutex::new(String::new())));
    HttpServer::new(move || {
        App::new()
            .service(get_clipboard)
            .service(update_clipboard)
            .service(Files::new("/", "./web/dist").index_file("index.html"))
            .app_data(data.clone())
    })
    .bind(("0.0.0.0", 9257))
    .unwrap()
    .run()
    .await
    .unwrap();
}

#[actix_web::post("/clipboard")]
async fn update_clipboard(data: Data<AppState>, body: String) -> impl Responder {
    if body.len() > 100_000 {
        return HttpResponse::BadRequest();
    }
    *data.0.lock().await = body;
    HttpResponse::Ok()
}

#[actix_web::get("/clipboard")]
async fn get_clipboard(data: Data<AppState>) -> String {
    data.0.lock().await.clone()
}
