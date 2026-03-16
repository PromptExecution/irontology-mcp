use std::{env, net::SocketAddr, process::exit, time::Duration};

use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use tokio::{net::TcpListener, time::sleep};

#[tokio::main]
async fn main() {
    let mut port = 0_u16;
    let mut model = "mock-model".to_string();
    let mut lifetime_ms: Option<u64> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .expect("port value")
                    .parse()
                    .expect("valid port");
            }
            "-m" | "--model" => {
                model = args.next().expect("model value");
            }
            "--lifetime-ms" => {
                lifetime_ms = Some(
                    args.next()
                        .expect("lifetime value")
                        .parse()
                        .expect("valid lifetime"),
                );
            }
            "run" => {}
            _ => {}
        }
    }

    if let Some(lifetime_ms) = lifetime_ms {
        tokio::spawn(async move {
            sleep(Duration::from_millis(lifetime_ms)).await;
            exit(0);
        });
    }

    let model_for_routes = model.clone();
    let app = Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/v1/models", get(move || models(model_for_routes.clone())))
        .route("/v1/chat/completions", axum::routing::post(chat))
        .route("/v1/embeddings", axum::routing::post(embeddings));

    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind mock server");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    eprintln!("mock mistralrs listening on {addr}");
    axum::serve(listener, app).await.expect("serve");
}

async fn models(model: String) -> impl IntoResponse {
    Json(json!({ "data": [{ "id": model }] }))
}

async fn chat() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "model": "mock-model",
            "choices": [{ "message": { "content": "managed response" } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })),
    )
}

async fn embeddings() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "model": "mock-model",
            "data": [{ "embedding": [0.5, 0.5] }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 0, "total_tokens": 1 }
        })),
    )
}
