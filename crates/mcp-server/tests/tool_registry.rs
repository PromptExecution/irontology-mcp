use anyhow::Result;
use forward_mcp::{ForwardResponse, StaticForwarder};
use mcp_server::ToolRegistry;
use orchestrator::{AgentRunResponse, StaticExecutor};
use provider_test::FixtureProvider;
use retrieval::{RankedResult, SearchBackend};
use serde_json::json;
use storage_neumann::{config::NeumannConfig, KnowledgeStore, NeumannStore, SemanticQuery};
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

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

#[tokio::test]
async fn registry_contains_and_invokes_phase2_tools() {
    let registry = ToolRegistry::with_phase2_tools(Box::new(FixedBackend));

    assert!(registry.has("repo.search"));
    assert!(registry.has("repo.read_symbol"));
    assert!(registry.has("ontology.list_classes"));
    assert!(registry.has("agent.forward_mcp"));
    assert!(registry.has("agent.run"));

    let search_tool = registry.get("repo.search").expect("search tool");
    let search = search_tool
        .call(json!({"query": "alpha", "top_k": 1}))
        .await
        .expect("search call");
    assert_eq!(search["results"][0]["id"], "alpha");

    let ontology_tool = registry
        .get("ontology.list_classes")
        .expect("ontology tool");
    let ontology = ontology_tool.call(json!({})).await.expect("ontology call");
    assert!(ontology["classes"].is_array());
}

#[tokio::test]
async fn registry_invokes_forward_mcp_tool_with_allowlist() {
    let forwarder = StaticForwarder::new(ForwardResponse {
        target: "stdio://child:other-agent".to_string(),
        output: json!({ "summary": "delegated" }),
        trace: vec!["tool:repo.search".to_string()],
        artifacts: vec!["artifact://run-1".to_string()],
    });
    let registry = ToolRegistry::with_phase2_tools_and_forwarder(
        Box::new(FixedBackend),
        std::sync::Arc::new(forwarder.clone()),
    );

    let tool = registry.get("agent.forward_mcp").expect("forward tool");
    let response = tool
        .call(json!({
            "target": "stdio://child:other-agent",
            "task": "Summarize auth module risks",
            "allowed_tools": ["repo.search"],
            "allowed_resources": ["repo://tree"],
            "allowed_prompts": ["delegate_task"],
            "context": ["repo://tree"],
            "budget_tokens": 8000,
            "timeout_ms": 30000,
            "return_mode": "final_with_trace",
            "payload": { "question": "auth risks" }
        }))
        .await
        .expect("forward call");

    assert_eq!(response["output"]["summary"], "delegated");
    assert_eq!(
        forwarder.seen_requests()[0].allowed_tools,
        vec!["repo.search"]
    );
    assert_eq!(
        forwarder.seen_requests()[0].allowed_resources,
        vec!["repo://tree"]
    );
}

#[tokio::test]
async fn registry_invokes_agent_run_tool() {
    let executor = StaticExecutor::new(AgentRunResponse {
        run_id: Uuid::nil(),
        answer: "bounded answer".to_string(),
        artifacts: vec!["artifact://run-1".to_string()],
    });
    let registry = ToolRegistry::with_phase2_tools_and_executor(
        Box::new(FixedBackend),
        std::sync::Arc::new(executor.clone()),
    );

    let tool = registry.get("agent.run").expect("agent run tool");
    let response = tool
        .call(json!({
            "task": "Summarize auth module risks",
            "model": "local/code",
            "max_turns": 6,
            "budget_tokens": 16000,
            "context": ["repo://tree"]
        }))
        .await
        .expect("agent run call");

    assert_eq!(response["answer"], "bounded answer");
    assert_eq!(executor.seen_requests()[0].model, "local/code");
    assert_eq!(executor.seen_requests()[0].context, vec!["repo://tree"]);
}

#[tokio::test]
async fn registry_invokes_repo_index_tool_and_persists_embeddings() {
    let dir = tempdir().expect("tempdir");
    let config = NeumannConfig { data_path: Some(dir.path().join("store")), ..Default::default() };
    let store: Arc<dyn KnowledgeStore> = Arc::new(NeumannStore::try_new(config).expect("open store"));
    let provider = Arc::new(FixtureProvider::new("fixture-embed").with_embedding_dim(4));
    let registry =
        ToolRegistry::with_phase2_tools_and_provider(Box::new(FixedBackend), store.clone(), provider);

    assert!(registry.has("repo.index"));

    let tool = registry.get("repo.index").expect("repo index tool");
    let response = tool
        .call(json!({
            "topic": "auth-risks",
            "content": "a".repeat(700),
            "source": "https://example.com/auth"
        }))
        .await
        .expect("repo index call");

    assert_eq!(response["chunks_created"], 2);

    let stored = store
        .query(SemanticQuery::Vector {
            embedding: Arc::from([1.0_f32, 0.0, 0.0, 0.0]),
            top_k: 10,
            modality: None,
        })
        .await
        .expect("query embeddings");
    assert_eq!(stored.ids.len(), 2);
}

#[tokio::test]
async fn repo_index_rejects_oversized_content() {
    use mcp_server::tools::repo_index::{MAX_CONTENT_BYTES, MAX_CHUNKS};

    let dir = tempdir().expect("tempdir");
    let config = NeumannConfig { data_path: Some(dir.path().join("store")), ..Default::default() };
    let store: Arc<dyn KnowledgeStore> = Arc::new(NeumannStore::try_new(config).expect("open store"));
    let provider = Arc::new(FixtureProvider::new("fixture-embed").with_embedding_dim(4));
    let registry =
        ToolRegistry::with_phase2_tools_and_provider(Box::new(FixedBackend), store.clone(), provider);

    let tool = registry.get("repo.index").expect("repo index tool");

    // Content that exceeds MAX_CONTENT_BYTES should be rejected.
    let oversized = "x".repeat(MAX_CONTENT_BYTES + 1);
    let err = tool
        .call(json!({
            "topic": "oversized",
            "content": oversized
        }))
        .await
        .expect_err("expected error for oversized content");
    assert!(
        err.to_string().contains("exceeds maximum allowed size"),
        "unexpected error: {err}"
    );

    // Content that fits in bytes but would produce too many chunks.
    // chunk_text splits on byte offsets (512 bytes per chunk for ASCII content);
    // (MAX_CHUNKS + 1) * 512 bytes of ASCII produces MAX_CHUNKS + 1 chunks.
    let too_many_chunks = "y".repeat((MAX_CHUNKS + 1) * 512);
    let err = tool
        .call(json!({
            "topic": "too-many-chunks",
            "content": too_many_chunks
        }))
        .await
        .expect_err("expected error for too many chunks");
    assert!(
        err.to_string().contains("exceeds the maximum"),
        "unexpected error: {err}"
    );
}
