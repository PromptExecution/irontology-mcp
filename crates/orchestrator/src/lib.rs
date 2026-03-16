use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunRequest {
    pub task: String,
    pub model: String,
    pub max_turns: u32,
    pub budget_tokens: u32,
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunResponse {
    pub run_id: Uuid,
    pub answer: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub mode: JobMode,
    pub steps: Vec<JobStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobMode {
    Serial,
    Batch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum JobStep {
    Plan(Value),
    Retrieve(Value),
    CallTool(Value),
    Delegate(Value),
    Synthesize(Value),
    Persist(Value),
}

#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn run(&self, request: AgentRunRequest) -> Result<AgentRunResponse>;
}

#[derive(Default)]
pub struct DisabledExecutor;

#[async_trait]
impl AgentExecutor for DisabledExecutor {
    async fn run(&self, _request: AgentRunRequest) -> Result<AgentRunResponse> {
        Err(anyhow!("agent execution is not configured"))
    }
}

#[derive(Clone)]
pub struct StaticExecutor {
    response: AgentRunResponse,
    seen: Arc<Mutex<Vec<AgentRunRequest>>>,
}

impl StaticExecutor {
    pub fn new(response: AgentRunResponse) -> Self {
        Self {
            response,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn seen_requests(&self) -> Vec<AgentRunRequest> {
        self.seen.lock().expect("seen requests").clone()
    }
}

#[async_trait]
impl AgentExecutor for StaticExecutor {
    async fn run(&self, request: AgentRunRequest) -> Result<AgentRunResponse> {
        self.seen.lock().expect("seen requests").push(request);
        Ok(self.response.clone())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use serde_json::json;
    use uuid::Uuid;

    use crate::{
        AgentExecutor, AgentRunRequest, AgentRunResponse, Job, JobMode, JobStep, StaticExecutor,
    };

    #[test]
    fn orchestration_contract_serializes_stably() {
        let request = AgentRunRequest {
            task: "summarize auth risks".to_string(),
            model: "local/code".to_string(),
            max_turns: 8,
            budget_tokens: 32000,
            context: vec!["repo://tree".to_string()],
        };

        let job = Job {
            id: Uuid::nil(),
            mode: JobMode::Serial,
            steps: vec![
                JobStep::Plan(json!({ "task": request.task })),
                JobStep::Retrieve(json!({ "query": "auth" })),
            ],
        };

        let json = serde_json::to_value(&job).expect("serialize");
        assert_eq!(json["mode"], "Serial");
        assert_eq!(json["steps"][0]["kind"], "Plan");
    }

    #[tokio::test]
    async fn static_executor_records_runs() -> Result<()> {
        let executor = StaticExecutor::new(AgentRunResponse {
            run_id: Uuid::nil(),
            answer: "done".to_string(),
            artifacts: vec!["artifact://1".to_string()],
        });
        let response = executor
            .run(AgentRunRequest {
                task: "summarize auth risks".to_string(),
                model: "local/code".to_string(),
                max_turns: 8,
                budget_tokens: 32_000,
                context: vec!["repo://tree".to_string()],
            })
            .await?;

        assert_eq!(response.answer, "done");
        assert_eq!(executor.seen_requests()[0].task, "summarize auth risks");
        Ok(())
    }
}
