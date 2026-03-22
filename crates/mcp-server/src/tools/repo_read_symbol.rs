use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use storage_neumann::KnowledgeStore;

use crate::Tool;
use crate::tools::symbol_context::{edges_json, fact_text, facts_json, resolve_symbol_context};

pub struct RepoReadSymbolTool {
    store: Arc<dyn KnowledgeStore>,
}

impl RepoReadSymbolTool {
    pub fn new(store: Arc<dyn KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for RepoReadSymbolTool {
    fn name(&self) -> &str {
        "repo.read_symbol"
    }

    fn description(&self) -> &str {
        "Read symbol metadata by identifier"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("id missing"))?;
        let context = resolve_symbol_context(self.store.as_ref(), id, true).await?;

        let found = !context.symbols.is_empty()
            || !context.facts.is_empty()
            || !context.edges.is_empty();

        // Prefer fields from SymbolRecord; fall back to facts for enrichment.
        let sym = context.symbols.first();
        let content = sym
            .map(|s| s.content.as_str())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| fact_text(&context.facts, &["content", "snippet", "summary", "text"]));
        let location = sym
            .map(|s| s.path.as_str())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| fact_text(&context.facts, &["location", "path", "uri", "source"]));
        let symbol_kind = sym
            .map(|s| s.kind.as_str())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| fact_text(&context.facts, &["symbol_kind", "kind", "type"]));
        let span = sym.map(|s| json!({ "start": s.start_line, "end": s.end_line }));
        let signature = sym.and_then(|s| s.signature.as_deref().map(ToOwned::to_owned));

        Ok(json!({
            "id": id,
            "found": found,
            "content": content,
            "location": location,
            "symbol_kind": symbol_kind,
            "span": span,
            "signature": signature,
            "facts": facts_json(&context.facts),
            "edges": edges_json(&context.edges),
        }))
    }
}
