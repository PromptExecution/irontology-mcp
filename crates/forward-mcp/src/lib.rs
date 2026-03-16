use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnMode {
    FinalOnly,
    FinalWithTrace,
    Structured,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardRequest {
    pub target: String,
    pub task: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_resources: Vec<String>,
    #[serde(default)]
    pub allowed_prompts: Vec<String>,
    #[serde(default)]
    pub context: Vec<String>,
    pub budget_tokens: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub return_mode: ReturnMode,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardResponse {
    pub target: String,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub trace: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[async_trait]
pub trait McpForwarder: Send + Sync {
    async fn forward(&self, request: ForwardRequest) -> Result<ForwardResponse>;
}

#[derive(Default)]
pub struct DisabledForwarder;

#[async_trait]
impl McpForwarder for DisabledForwarder {
    async fn forward(&self, _request: ForwardRequest) -> Result<ForwardResponse> {
        Err(anyhow!("mcp forwarding is not configured"))
    }
}

#[derive(Clone)]
pub struct StaticForwarder {
    response: ForwardResponse,
    seen: Arc<Mutex<Vec<ForwardRequest>>>,
}

impl StaticForwarder {
    pub fn new(response: ForwardResponse) -> Self {
        Self {
            response,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn seen_requests(&self) -> Vec<ForwardRequest> {
        self.seen.lock().expect("seen requests").clone()
    }
}

#[async_trait]
impl McpForwarder for StaticForwarder {
    async fn forward(&self, request: ForwardRequest) -> Result<ForwardResponse> {
        self.seen.lock().expect("seen requests").push(request);
        Ok(self.response.clone())
    }
}

pub struct TransportForwarder {
    client: Client,
}

impl Default for TransportForwarder {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportForwarder {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("forward transport client"),
        }
    }

    async fn forward_http(
        &self,
        target: &str,
        request: &ForwardRequest,
    ) -> Result<ForwardResponse> {
        let mut builder = self.client.post(target).json(&rpc_request(request));
        if let Some(timeout_ms) = request.timeout_ms {
            builder = builder.timeout(Duration::from_millis(timeout_ms));
        }

        let response = builder.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("http forward failed with {status}: {body}"));
        }

        parse_rpc_response(response.json().await?)
    }

    async fn forward_stdio(
        &self,
        command: &str,
        request: &ForwardRequest,
    ) -> Result<ForwardResponse> {
        let (program, args) = parse_command(command)?;
        let mut child = Command::new(&program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn forward target: {program}"))?;

        let payload = serde_json::to_vec(&rpc_request(request)).context("serialize rpc request")?;
        let mut stdin = child.stdin.take().context("forward child missing stdin")?;
        stdin.write_all(&payload).await?;
        stdin.write_all(b"\n").await?;
        stdin.shutdown().await?;
        drop(stdin);

        let mut stdout = child
            .stdout
            .take()
            .context("forward child missing stdout")?;
        let mut stderr = child
            .stderr
            .take()
            .context("forward child missing stderr")?;
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            stdout.read_to_end(&mut buf).await?;
            Result::<Vec<u8>>::Ok(buf)
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            stderr.read_to_end(&mut buf).await?;
            Result::<Vec<u8>>::Ok(buf)
        });

        let status = wait_for_child(&mut child, request.timeout_ms).await?;
        let stdout = stdout_task.await.context("join stdout task")??;
        let stderr = stderr_task.await.context("join stderr task")??;

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            return Err(anyhow!("stdio forward failed with {status}: {stderr}"));
        }

        let body = String::from_utf8(stdout).context("forward stdout was not valid utf-8")?;
        let value = parse_json_output(&body)?;
        parse_rpc_response(value)
    }
}

#[async_trait]
impl McpForwarder for TransportForwarder {
    async fn forward(&self, request: ForwardRequest) -> Result<ForwardResponse> {
        match parse_target(&request.target)? {
            ForwardTarget::Http(url) => self.forward_http(&url, &request).await,
            ForwardTarget::Stdio(command) => self.forward_stdio(&command, &request).await,
        }
    }
}

#[derive(Debug)]
enum ForwardTarget {
    Http(String),
    Stdio(String),
}

fn parse_target(target: &str) -> Result<ForwardTarget> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return Ok(ForwardTarget::Http(target.to_string()));
    }

    if let Some(command) = target.strip_prefix("stdio://child:") {
        return Ok(ForwardTarget::Stdio(command.trim().to_string()));
    }

    if let Some(command) = target.strip_prefix("stdio://") {
        return Ok(ForwardTarget::Stdio(command.trim().to_string()));
    }

    Err(anyhow!("unsupported forward target: {target}"))
}

fn parse_command(command: &str) -> Result<(String, Vec<String>)> {
    let parts = shlex::split(command).ok_or_else(|| anyhow!("invalid stdio command: {command}"))?;
    let Some((program, args)) = parts.split_first() else {
        return Err(anyhow!("stdio command is empty"));
    };
    Ok((program.clone(), args.to_vec()))
}

fn rpc_request(request: &ForwardRequest) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "forward-1",
        "method": "tools/call",
        "params": {
            "name": "agent.execute_delegated_task",
            "arguments": request,
        }
    })
}

fn parse_rpc_response(value: Value) -> Result<ForwardResponse> {
    if let Some(error) = value.get("error") {
        return Err(anyhow!("forward target returned error: {error}"));
    }

    if let Some(result) = value.get("result") {
        if let Some(content) = result.get("content").and_then(Value::as_array) {
            if let Some(json) = content.first().and_then(|item| item.get("json")).cloned() {
                return serde_json::from_value(json).context("deserialize rpc tool content");
            }
        }
        return serde_json::from_value(result.clone()).context("deserialize rpc result");
    }

    serde_json::from_value(value).context("deserialize direct forward response")
}

fn parse_json_output(body: &str) -> Result<Value> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("forward target returned no stdout"));
    }

    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    for line in trimmed.lines().rev() {
        let candidate = line.trim();
        if candidate.starts_with('{') || candidate.starts_with('[') {
            if let Ok(value) = serde_json::from_str(candidate) {
                return Ok(value);
            }
        }
    }

    Err(anyhow!(
        "forward target stdout did not contain a json document"
    ))
}

async fn wait_for_child(
    child: &mut tokio::process::Child,
    timeout_ms: Option<u64>,
) -> Result<std::process::ExitStatus> {
    if let Some(timeout_ms) = timeout_ms {
        match timeout(Duration::from_millis(timeout_ms), child.wait()).await {
            Ok(status) => Ok(status?),
            Err(_) => {
                let _ = child.kill().await;
                Err(anyhow!("stdio forward timed out after {timeout_ms}ms"))
            }
        }
    } else {
        Ok(child.wait().await?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::{
        parse_target, ForwardRequest, ForwardResponse, McpForwarder, ReturnMode, StaticForwarder,
        TransportForwarder,
    };

    #[test]
    fn request_contract_serializes_stably() {
        let request = ForwardRequest {
            target: "stdio://child:other-agent".to_string(),
            task: "Summarize auth module risks".to_string(),
            allowed_tools: vec!["repo.search".to_string()],
            allowed_resources: vec!["repo://tree".to_string()],
            allowed_prompts: vec!["delegate_task".to_string()],
            context: vec!["repo://tree".to_string()],
            budget_tokens: Some(8000),
            timeout_ms: Some(30_000),
            return_mode: ReturnMode::FinalWithTrace,
            payload: json!({"kind": "summary"}),
        };

        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["return_mode"], "final_with_trace");
        assert_eq!(json["allowed_tools"][0], "repo.search");
    }

    #[test]
    fn unsupported_target_is_rejected() {
        let error = parse_target("ftp://example.com").expect_err("unsupported target");
        assert!(error.to_string().contains("unsupported forward target"));
    }

    #[tokio::test]
    async fn static_forwarder_records_requests() {
        let forwarder = StaticForwarder::new(ForwardResponse {
            target: "stdio://child:other-agent".to_string(),
            output: json!({"answer": "ok"}),
            trace: vec!["delegated".to_string()],
            artifacts: vec!["artifact://1".to_string()],
        });

        let response = forwarder
            .forward(ForwardRequest {
                target: "stdio://child:other-agent".to_string(),
                task: "run".to_string(),
                allowed_tools: vec!["repo.search".to_string()],
                allowed_resources: vec![],
                allowed_prompts: vec![],
                context: vec![],
                budget_tokens: None,
                timeout_ms: None,
                return_mode: ReturnMode::FinalOnly,
                payload: json!({"kind": "test"}),
            })
            .await
            .expect("forward");

        assert_eq!(response.output["answer"], "ok");
        assert_eq!(forwarder.seen_requests()[0].task, "run");
    }

    #[tokio::test]
    async fn transport_forwarder_rejects_invalid_stdio_command() {
        let forwarder = TransportForwarder::new();
        let error = forwarder
            .forward(ForwardRequest {
                target: "stdio://".to_string(),
                task: "run".to_string(),
                allowed_tools: vec![],
                allowed_resources: vec![],
                allowed_prompts: vec![],
                context: vec![],
                budget_tokens: None,
                timeout_ms: Some(50),
                return_mode: ReturnMode::FinalOnly,
                payload: Value::Null,
            })
            .await
            .expect_err("invalid stdio target");

        assert!(error.to_string().contains("stdio command is empty"));
    }
}
