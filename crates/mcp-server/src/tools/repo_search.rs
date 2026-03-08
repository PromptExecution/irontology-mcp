use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use retrieval::{fusion_search, FusionWeights, SearchBackend};

use crate::Tool;

pub struct RepoSearchTool {
    backend: Box<dyn SearchBackend + Send + Sync>,
}

impl RepoSearchTool {
    pub fn new(backend: Box<dyn SearchBackend + Send + Sync>) -> Self {
        Self { backend }
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
                "query": { "type": "string" },
                "top_k": { "type": "integer", "default": 10 }
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

        let results = fusion_search(
            query,
            top_k,
            FusionWeights::default(),
            self.backend.as_ref(),
        )?;
        Ok(json!({
            "results": results.into_iter().map(|r| json!({"id": r.id, "score": r.score})).collect::<Vec<_>>()
        }))
    }
}
