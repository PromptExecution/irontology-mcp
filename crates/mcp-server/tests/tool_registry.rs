use anyhow::Result;
use mcp_server::ToolRegistry;
use retrieval::{RankedResult, SearchBackend};
use serde_json::json;

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
