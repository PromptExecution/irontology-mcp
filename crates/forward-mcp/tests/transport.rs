use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::post, Json, Router};
use forward_mcp::{ForwardRequest, McpForwarder, ReturnMode, TransportForwarder};
use serde_json::{json, Value};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct MockHttpState {
    seen: Arc<Mutex<Vec<Value>>>,
}

#[tokio::test]
async fn transport_forwarder_posts_json_rpc_over_http() {
    let state = MockHttpState::default();
    let target = spawn_http_server(state.clone()).await;
    let forwarder = TransportForwarder::new();

    let response = forwarder
        .forward(sample_request(target.clone()))
        .await
        .expect("http forward");

    assert_eq!(response.target, target);
    assert_eq!(response.output["summary"], "delegated");
    assert_eq!(response.trace, vec!["mock-http"]);

    let seen = state.seen.lock().expect("seen");
    assert_eq!(seen[0]["method"], "tools/call");
    assert_eq!(seen[0]["params"]["name"], "agent.execute_delegated_task");
    assert_eq!(
        seen[0]["params"]["arguments"]["task"],
        "Summarize auth module risks"
    );
}

#[tokio::test]
async fn transport_forwarder_sends_json_rpc_over_stdio() {
    let forwarder = TransportForwarder::new();
    let target = format!(
        "stdio://child:{}",
        shell_escape(binary_path("mock_forward_stdio"))
    );

    let response = forwarder
        .forward(sample_request(target.clone()))
        .await
        .expect("stdio forward");

    assert_eq!(response.target, target);
    assert_eq!(
        response.output["answer"],
        "handled: Summarize auth module risks"
    );
    assert_eq!(response.trace, vec!["mock-stdio"]);
}

async fn spawn_http_server(state: MockHttpState) -> String {
    async fn forward(State(state): State<MockHttpState>, Json(body): Json<Value>) -> Json<Value> {
        state.seen.lock().expect("seen").push(body.clone());
        Json(json!({
            "jsonrpc": "2.0",
            "id": "forward-1",
            "result": {
                "target": body["params"]["arguments"]["target"],
                "output": { "summary": "delegated" },
                "trace": ["mock-http"],
                "artifacts": ["artifact://http-1"]
            }
        }))
    }

    let app = Router::new().route("/", post(forward)).with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}/")
}

fn sample_request(target: String) -> ForwardRequest {
    ForwardRequest {
        target,
        task: "Summarize auth module risks".to_string(),
        allowed_tools: vec!["repo.search".to_string()],
        allowed_resources: vec!["repo://tree".to_string()],
        allowed_prompts: vec!["delegate_task".to_string()],
        context: vec!["repo://tree".to_string()],
        budget_tokens: Some(8_000),
        timeout_ms: Some(3_000),
        return_mode: ReturnMode::FinalWithTrace,
        payload: json!({ "question": "auth risks" }),
    }
}

fn binary_path(name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
        return PathBuf::from(path);
    }

    let current = std::env::current_exe().expect("current exe");
    current
        .parent()
        .and_then(|dir| dir.parent())
        .map(|dir| dir.join(name))
        .expect("derive cargo bin path")
}

fn shell_escape(path: PathBuf) -> String {
    let path = path.display().to_string();
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}
