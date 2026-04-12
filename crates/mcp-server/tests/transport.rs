use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use mcp_server::{JsonRpcRequest, JsonRpcResponse, McpServerRuntime};
use retrieval::{RankedResult, SearchBackend};
use serde_json::{json, Value};
use storage_neumann::config::NeumannConfig;
use tempfile::tempdir;
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

struct FixedBackend;

impl SearchBackend for FixedBackend {
    fn search_vector(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![RankedResult {
            id: "alpha".into(),
            score: 0.9,
            anchor_locator: None,
            artifact_uri: None,
        }])
    }

    fn search_graph(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![])
    }

    fn search_lexical(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![])
    }

    fn search_ontology(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn stdio_and_http_transports_match_for_tools_list() {
    let dir = tempdir().expect("tempdir");
    let runtime = Arc::new(
        McpServerRuntime::start_phase2(Box::new(FixedBackend), test_config(dir.path().join("list")))
            .await
            .expect("start runtime"),
    );
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(1),
        method: "tools/list".to_string(),
        params: Value::Null,
    };

    let stdio = call_over_stdio(runtime.clone(), request.clone()).await;
    let http = call_over_http(runtime, request).await;

    assert_eq!(stdio.result, http.result);
    assert!(stdio.error.is_none());
    assert!(http.error.is_none());
}

#[tokio::test]
async fn stdio_and_http_transports_match_for_tool_calls() {
    let dir = tempdir().expect("tempdir");
    let runtime = Arc::new(
        McpServerRuntime::start_phase2(Box::new(FixedBackend), test_config(dir.path().join("call")))
            .await
            .expect("start runtime"),
    );
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(2),
        method: "tools/call".to_string(),
        params: json!({
            "name": "repo.search",
            "arguments": {
                "query": "alpha",
                "top_k": 1
            }
        }),
    };

    let stdio = call_over_stdio(runtime.clone(), request.clone()).await;
    let http = call_over_http(runtime, request).await;

    assert_eq!(stdio.result, http.result);
    assert_eq!(
        stdio.result.expect("stdio result")["content"][0]["json"]["results"][0]["id"],
        json!("alpha")
    );
}

async fn call_over_stdio(
    runtime: Arc<McpServerRuntime>,
    request: JsonRpcRequest,
) -> JsonRpcResponse {
    let (mut client_writer, server_reader) = duplex(8192);
    let (server_writer, client_reader) = duplex(8192);

    let server = tokio::spawn(async move {
        McpServerRuntime::serve_stdio_streams(runtime, server_reader, server_writer)
            .await
            .expect("serve stdio");
    });

    let payload = serde_json::to_vec(&request).expect("serialize request");
    client_writer
        .write_all(&payload)
        .await
        .expect("write request");
    client_writer.write_all(b"\n").await.expect("write newline");
    client_writer.shutdown().await.expect("shutdown writer");

    let mut lines = BufReader::new(client_reader).lines();
    let line = lines
        .next_line()
        .await
        .expect("read response")
        .expect("response line");

    server.await.expect("stdio server task");
    serde_json::from_str(&line).expect("parse stdio response")
}

async fn call_over_http(
    runtime: Arc<McpServerRuntime>,
    request: JsonRpcRequest,
) -> JsonRpcResponse {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let router = McpServerRuntime::router(runtime);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve http");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .json(&request)
        .send()
        .await
        .expect("send request")
        .json::<JsonRpcResponse>()
        .await
        .expect("decode response");

    server.abort();
    response
}

fn test_config(path: std::path::PathBuf) -> NeumannConfig {
    NeumannConfig {
        endpoint: "http://localhost:7777".to_string(),
        namespace: "test".to_string(),
        data_path: Some(path),
    }
}
