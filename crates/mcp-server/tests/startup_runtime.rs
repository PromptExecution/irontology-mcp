use std::{
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use axum::{extract::State, routing::post, Json, Router};
use indexer::{Extraction, GitLedger, Handler, IntakeFile, RuleMatcher, WatchConfig};
use mcp_server::{McpServerRuntime, Phase2RuntimeConfig, WatchRuntimeConfig};
use provider_test::FixtureProvider;
use retrieval::{RankedResult, SearchBackend};
use serde_json::{json, Value};
use storage_neumann::{config::NeumannConfig, SemanticQuery};
use tempfile::tempdir;
use tokio::{net::TcpListener, time::sleep};

struct FixedBackend;

impl SearchBackend for FixedBackend {
    fn search_vector(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![RankedResult {
            id: "alpha".into(),
            score: 0.9,
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

#[derive(Clone, Default)]
struct MockForwardState {
    seen: Arc<Mutex<Vec<Value>>>,
}

struct FixedLedger;

#[async_trait]
impl GitLedger for FixedLedger {
    async fn blob_id(&self, path: &Path) -> Result<String> {
        Ok(path
            .display()
            .to_string()
            .replace(std::path::MAIN_SEPARATOR, "_"))
    }
}

struct MatchAllRules;

impl RuleMatcher for MatchAllRules {
    fn match_file(&self, _file: &IntakeFile) -> bool {
        true
    }
}

struct FileContentHandler;

#[async_trait]
impl Handler for FileContentHandler {
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction> {
        let text = tokio::fs::read_to_string(&file.path).await?;
        Ok(Extraction {
            text,
            has_symbols: file.extension == ".rs",
            fields: Default::default(),
            class: None,
            shape: None,
            claims: vec![],
            relations: vec![],
            notes: vec![],
        })
    }
}

#[tokio::test]
async fn startup_ingests_ontology_and_serves_semantic_mcp_queries() {
    let runtime = McpServerRuntime::start_phase2(Box::new(FixedBackend), NeumannConfig::default())
        .await
        .expect("start runtime");

    assert!(runtime.resources.has("ontology://naming_conventions"));
    assert!(runtime.tools.has("ontology.related_resources"));
    assert!(runtime.tools.has("agent.forward_mcp"));
    assert!(runtime.tools.has("agent.run"));

    let tool = runtime
        .tools
        .get("ontology.related_resources")
        .expect("semantic ontology tool");

    let response = tool
        .call(json!({
            "subject": "https://example.org/pe/doc/incident-42",
            "predicate": "https://example.org/pe/hasTopic"
        }))
        .await
        .expect("tool call");

    assert_eq!(
        response["objects"][0],
        json!("https://example.org/pe/topic/payment-retries")
    );

    runtime.shutdown().await.expect("shutdown runtime");
}

#[tokio::test]
async fn startup_with_transport_forwarding_delegates_via_mcp_tool() {
    let state = MockForwardState::default();
    let target = spawn_forward_server(state.clone()).await;
    let runtime = McpServerRuntime::start_phase2_with_transport_forwarding(
        Box::new(FixedBackend),
        NeumannConfig::default(),
    )
    .await
    .expect("start runtime with transport forwarding");

    let tool = runtime
        .tools
        .get("agent.forward_mcp")
        .expect("forward tool");

    let response = tool
        .call(json!({
            "target": target,
            "task": "Summarize auth module risks",
            "allowed_tools": ["repo.search"],
            "allowed_resources": ["repo://tree"],
            "allowed_prompts": ["delegate_task"],
            "context": ["repo://tree"],
            "budget_tokens": 8000,
            "timeout_ms": 3000,
            "return_mode": "final_with_trace",
            "payload": { "question": "auth risks" }
        }))
        .await
        .expect("delegate call");

    assert_eq!(response["output"]["summary"], "delegated");
    assert_eq!(state.seen.lock().expect("seen")[0]["method"], "tools/call");

    runtime.shutdown().await.expect("shutdown runtime");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_with_watch_runtime_indexes_changed_files() {
    let root = tempdir().expect("tempdir");
    let file = root.path().join("src.rs");
    let runtime = McpServerRuntime::start_phase2_configured(
        Box::new(FixedBackend),
        Phase2RuntimeConfig::new(NeumannConfig::default()).with_watch(WatchRuntimeConfig {
            config: WatchConfig {
                roots: vec![root.path().display().to_string()],
            },
            git_ledger: Arc::new(FixedLedger),
            rules: Arc::new(MatchAllRules),
            handler: Arc::new(FileContentHandler),
            provider: Arc::new(FixtureProvider::new("fixture-embed")),
        }),
    )
    .await
    .expect("start runtime with watcher");

    sleep(Duration::from_millis(250)).await;
    tokio::fs::write(&file, "fn main() {}\n")
        .await
        .expect("write source file");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let result = runtime
                .store
                .query(SemanticQuery::Files {
                    path: Some(file.display().to_string()),
                    blob: None,
                })
                .await
                .expect("query store");
            if !result.files.is_empty() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("watch runtime indexed file");

    runtime.shutdown().await.expect("shutdown runtime");
}

#[tokio::test]
async fn startup_with_provider_registers_repo_index_tool() {
    let runtime = McpServerRuntime::start_phase2_configured(
        Box::new(FixedBackend),
        Phase2RuntimeConfig::new(NeumannConfig::default())
            .with_provider(Arc::new(FixtureProvider::new("fixture-embed").with_embedding_dim(4))),
    )
    .await
    .expect("start runtime with provider");

    assert!(runtime.tools.has("repo.index"));

    let tool = runtime.tools.get("repo.index").expect("repo index tool");
    let response = tool
        .call(json!({
            "topic": "grok-digest",
            "content": "payment retries and auth edge cases",
            "source": "https://example.com/notes"
        }))
        .await
        .expect("repo index call");

    assert_eq!(response["chunks_created"], 1);

    let result = runtime
        .store
        .query(SemanticQuery::Vector {
            embedding: Arc::from([1.0_f32, 0.0, 0.0, 0.0]),
            top_k: 10,
            modality: None,
        })
        .await
        .expect("query vector store");
    assert_eq!(result.ids.len(), 1);

    runtime.shutdown().await.expect("shutdown runtime");
}

async fn spawn_forward_server(state: MockForwardState) -> String {
    async fn forward(
        State(state): State<MockForwardState>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
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
