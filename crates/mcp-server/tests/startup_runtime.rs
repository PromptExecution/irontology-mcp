use anyhow::Result;
use mcp_server::McpServerRuntime;
use retrieval::{RankedResult, SearchBackend};
use serde_json::json;
use storage_neumann::config::NeumannConfig;

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
async fn startup_ingests_ontology_and_serves_semantic_mcp_queries() {
    let runtime = McpServerRuntime::start_phase2(Box::new(FixedBackend), NeumannConfig::default())
        .await
        .expect("start runtime");

    assert!(runtime.resources.has("ontology://naming_conventions"));
    assert!(runtime.tools.has("ontology.related_resources"));

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
}
