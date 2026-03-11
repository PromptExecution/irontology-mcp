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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use crate::{AgentRunRequest, Job, JobMode, JobStep};

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
}
