use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use orchestrator::{AgentExecutor, AgentRunRequest};
use serde_json::{json, Value};

use crate::Tool;

pub struct AgentRunTool {
    executor: Arc<dyn AgentExecutor>,
}

impl AgentRunTool {
    pub fn new(executor: Arc<dyn AgentExecutor>) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl Tool for AgentRunTool {
    fn name(&self) -> &str {
        "agent.run"
    }

    fn description(&self) -> &str {
        "Run the bounded internal agent loop for a single task"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" },
                "model": { "type": "string" },
                "max_turns": { "type": "integer", "default": 8 },
                "budget_tokens": { "type": "integer", "default": 32000 },
                "context": { "type": "array", "items": { "type": "string" }, "default": [] }
            },
            "required": ["task", "model"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let request = AgentRunRequest {
            task: required_string(&params, "task")?,
            model: required_string(&params, "model")?,
            max_turns: params
                .get("max_turns")
                .and_then(|value| value.as_u64())
                .unwrap_or(8) as u32,
            budget_tokens: params
                .get("budget_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(32_000) as u32,
            context: string_list(&params, "context")?,
        };

        let response = self.executor.run(request).await?;
        Ok(json!({
            "run_id": response.run_id,
            "answer": response.answer,
            "artifacts": response.artifacts,
        }))
    }
}

fn required_string(params: &Value, key: &str) -> Result<String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("{key} missing"))
}

fn string_list(params: &Value, key: &str) -> Result<Vec<String>> {
    let Some(value) = params.get(key) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(anyhow!("{key} must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("{key} entries must be strings"))
        })
        .collect()
}
