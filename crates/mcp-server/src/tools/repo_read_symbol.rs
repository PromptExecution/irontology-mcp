use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::Tool;

pub struct RepoReadSymbolTool;

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
        Ok(json!({ "id": id, "status": "ok" }))
    }
}
