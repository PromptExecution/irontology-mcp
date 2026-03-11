use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use forward_mcp::{ForwardRequest, McpForwarder, ReturnMode};
use serde_json::{json, Value};

use crate::Tool;

pub struct AgentForwardMcpTool {
    forwarder: Arc<dyn McpForwarder>,
}

impl AgentForwardMcpTool {
    pub fn new(forwarder: Arc<dyn McpForwarder>) -> Self {
        Self { forwarder }
    }
}

#[async_trait]
impl Tool for AgentForwardMcpTool {
    fn name(&self) -> &str {
        "agent.forward_mcp"
    }

    fn description(&self) -> &str {
        "Delegate a task to another MCP endpoint with an explicit allowlist and budget"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": { "type": "string" },
                "task": { "type": "string" },
                "allowed_tools": { "type": "array", "items": { "type": "string" }, "default": [] },
                "allowed_resources": { "type": "array", "items": { "type": "string" }, "default": [] },
                "allowed_prompts": { "type": "array", "items": { "type": "string" }, "default": [] },
                "context": { "type": "array", "items": { "type": "string" }, "default": [] },
                "budget_tokens": { "type": "integer" },
                "timeout_ms": { "type": "integer" },
                "return_mode": {
                    "type": "string",
                    "enum": ["final_only", "final_with_trace", "structured"],
                    "default": "final_with_trace"
                },
                "payload": {}
            },
            "required": ["target", "task"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let target = required_string(&params, "target")?;
        let task = required_string(&params, "task")?;
        let return_mode = parse_return_mode(params.get("return_mode"))?;

        let response = self
            .forwarder
            .forward(ForwardRequest {
                target,
                task,
                allowed_tools: string_list(&params, "allowed_tools")?,
                allowed_resources: string_list(&params, "allowed_resources")?,
                allowed_prompts: string_list(&params, "allowed_prompts")?,
                context: string_list(&params, "context")?,
                budget_tokens: params
                    .get("budget_tokens")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32),
                timeout_ms: params.get("timeout_ms").and_then(|value| value.as_u64()),
                return_mode,
                payload: params.get("payload").cloned().unwrap_or(Value::Null),
            })
            .await?;

        Ok(json!({
            "target": response.target,
            "output": response.output,
            "trace": response.trace,
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

fn parse_return_mode(value: Option<&Value>) -> Result<ReturnMode> {
    match value.and_then(|value| value.as_str()) {
        None | Some("final_with_trace") => Ok(ReturnMode::FinalWithTrace),
        Some("final_only") => Ok(ReturnMode::FinalOnly),
        Some("structured") => Ok(ReturnMode::Structured),
        Some(other) => Err(anyhow!("unsupported return_mode: {other}")),
    }
}
