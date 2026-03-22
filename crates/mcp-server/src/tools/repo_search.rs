use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use storage_neumann::KnowledgeStore;

use retrieval::{fusion_search, FusionWeights, SearchBackend};

use crate::Tool;
use crate::tools::symbol_context::{edges_json, fact_text, facts_json, resolve_symbol_context};

pub struct RepoSearchTool {
    backend: Box<dyn SearchBackend + Send + Sync>,
    store: Arc<dyn KnowledgeStore>,
}

impl RepoSearchTool {
    pub fn new(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
    ) -> Self {
        Self { backend, store }
    }
}

#[async_trait]
impl Tool for RepoSearchTool {
    fn name(&self) -> &str {
        "repo.search"
    }

    fn description(&self) -> &str {
        "Search code repository with fusion retrieval"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language or code search query"
                },
                "top_k": {
                    "type": "integer",
                    "default": 10,
                    "description": "Number of ranked results to return"
                },
                "expand": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include graph neighborhood edges for each hit"
                }
            },
            "required": ["query"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("query missing"))?;
        let top_k = params.get("top_k").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let expand = params
            .get("expand")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let results = fusion_search(
            query,
            top_k,
            FusionWeights::default(),
            self.backend.as_ref(),
        )?;

        let mut enriched = Vec::with_capacity(results.len());
        for result in results {
            let context = resolve_symbol_context(self.store.as_ref(), &result.id, expand).await?;
            enriched.push(json!({
                "id": result.id,
                "score": result.score,
                "content": fact_text(&context.facts, &["content", "snippet", "summary", "text"]),
                "location": fact_text(&context.facts, &["location", "path", "uri", "source"]),
                "symbol_kind": fact_text(&context.facts, &["symbol_kind", "kind", "type"]),
                "facts": facts_json(&context.facts),
                "edges": if expand { Value::Array(edges_json(&context.edges)) } else { Value::Array(vec![]) },
            }));
        }

        Ok(json!({ "results": enriched }))
    }
}
