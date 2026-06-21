use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use provider_api::{ChatMessage, ChatRequest, ModelProvider};
use retrieval::{fusion_search, FusionWeights, SearchBackend};
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

pub struct SimpleAgentExecutor {
    backend: Arc<dyn SearchBackend + Send + Sync>,
    provider: Arc<dyn ModelProvider>,
    weights: FusionWeights,
    top_k: usize,
}

impl SimpleAgentExecutor {
    pub fn new(
        backend: Arc<dyn SearchBackend + Send + Sync>,
        provider: Arc<dyn ModelProvider>,
    ) -> Self {
        Self {
            backend,
            provider,
            weights: FusionWeights::default(),
            top_k: 6,
        }
    }

    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k.max(1);
        self
    }
}

#[async_trait]
impl AgentExecutor for SimpleAgentExecutor {
    async fn run(&self, request: AgentRunRequest) -> Result<AgentRunResponse> {
        let evidence = fusion_search(&request.task, self.top_k, self.weights, self.backend.as_ref())?;
        let evidence_text = if evidence.is_empty() {
            "No indexed evidence found.".to_string()
        } else {
            evidence
                .iter()
                .map(|item| format!("- {} ({:.3})", item.id, item.score))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let context_text = if request.context.is_empty() {
            "None".to_string()
        } else {
            request.context.join(", ")
        };

        let response = self
            .provider
            .chat(ChatRequest {
                model: request.model.clone(),
                messages: vec![
                    ChatMessage {
                        role: "system".to_string(),
                        content: "You are a low-frills bounded orchestration agent. Answer using the provided evidence and state uncertainty when evidence is thin.".to_string(),
                    },
                    ChatMessage {
                        role: "user".to_string(),
                        content: format!(
                            "Task:\n{}\n\nContext:\n{}\n\nEvidence:\n{}\n\nProvide a concise answer.",
                            request.task, context_text, evidence_text
                        ),
                    },
                ],
                max_tokens: Some(request.budget_tokens.min(2048)),
                stream: false,
                params: Default::default(),
            })
            .await?;

        let run_id = Uuid::new_v4();
        Ok(AgentRunResponse {
            run_id,
            answer: response.content,
            artifacts: vec![format!("run://{run_id}")],
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use async_trait::async_trait;
    use provider_api::{ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, ModelProvider, ProviderHealth, TokenUsage};
    use retrieval::{RankedResult, SearchBackend};
    use serde_json::json;
    use uuid::Uuid;

    use crate::{
        AgentExecutor, AgentRunRequest, AgentRunResponse, Job, JobMode, JobStep,
        SimpleAgentExecutor, StaticExecutor,
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

    struct FixedBackend;

    impl SearchBackend for FixedBackend {
        fn search_vector(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
            Ok(vec![RankedResult {
                id: "repo://artifact/standing-data".to_string(),
                score: 0.91,
            anchor_locator: None,
            artifact_uri: None,
            }])
        }

        fn search_graph(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
            Ok(vec![])
        }

        fn search_lexical(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
            Ok(vec![RankedResult {
                id: "repo://file/acme/notes.md".to_string(),
                score: 0.76,
            anchor_locator: None,
            artifact_uri: None,
            }])
        }

        fn search_ontology(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
            Ok(vec![])
        }
    }

    struct ProbeProvider {
        seen: Arc<Mutex<Vec<ChatRequest>>>,
    }

    impl ProbeProvider {
        fn new() -> Self {
            Self {
                seen: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn seen_requests(&self) -> Vec<ChatRequest> {
            self.seen.lock().expect("seen").clone()
        }
    }

    #[async_trait]
    impl ModelProvider for ProbeProvider {
        async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
            self.seen.lock().expect("seen").push(req.clone());
            Ok(ChatResponse {
                model: req.model,
                content: "bounded answer".to_string(),
                usage: TokenUsage {
                    prompt_tokens: 42,
                    completion_tokens: 7,
                    total_tokens: 49,
                },
            })
        }

        async fn embed(&self, _req: EmbedRequest) -> Result<EmbedResponse> {
            unreachable!("embed is not used by the agent executor test")
        }

        async fn health(&self) -> Result<ProviderHealth> {
            Ok(ProviderHealth {
                healthy: true,
                message: "ok".to_string(),
            })
        }

        fn model_id(&self) -> &str {
            "fixture-agent"
        }
    }

    #[tokio::test]
    async fn simple_executor_uses_retrieval_evidence_in_prompt() -> Result<()> {
        let provider = Arc::new(ProbeProvider::new());
        let executor = SimpleAgentExecutor::new(Arc::new(FixedBackend), provider.clone());

        let response = executor
            .run(AgentRunRequest {
                task: "find latent dependencies".to_string(),
                model: "local/code".to_string(),
                max_turns: 6,
                budget_tokens: 16_000,
                context: vec!["repo://tree".to_string(), "ontology://classes".to_string()],
            })
            .await?;

        assert_eq!(response.answer, "bounded answer");
        assert!(response.artifacts[0].starts_with("run://"));
        let seen = provider.seen_requests();
        assert_eq!(seen[0].model, "local/code");
        assert!(seen[0].messages[1]
            .content
            .contains("repo://artifact/standing-data"));
        assert!(seen[0].messages[1].content.contains("ontology://classes"));
        Ok(())
    }
}
