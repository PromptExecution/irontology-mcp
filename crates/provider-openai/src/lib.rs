use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use provider_api::{
    ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, ModelProvider, ProviderHealth,
    TokenUsage,
};
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model_id: String,
    pub retries: u8,
    pub retry_backoff_ms: u64,
}

impl OpenAiCompatConfig {
    pub fn new(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            model_id: model_id.into(),
            retries: 1,
            retry_backoff_ms: 10,
        }
    }
}

pub struct OpenAiCompatProvider {
    client: Client,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client"),
            config,
        }
    }

    async fn send_json<F>(&self, make_request: F) -> Result<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let attempts = self.config.retries.max(1);
        for attempt in 0..attempts {
            let response = make_request().send().await?;
            if !should_retry(response.status()) || attempt + 1 == attempts {
                return Ok(response);
            }
            sleep(Duration::from_millis(self.config.retry_backoff_ms)).await;
        }
        Err(anyhow!("request failed after retries"))
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(api_key) = &self.config.api_key {
            builder.bearer_auth(api_key)
        } else {
            builder
        }
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}{path}")
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let mut payload = Map::<String, Value>::new();
        payload.insert(
            "model".to_string(),
            Value::String(if req.model.is_empty() {
                self.config.model_id.clone()
            } else {
                req.model
            }),
        );
        payload.insert(
            "messages".to_string(),
            serde_json::to_value(req.messages).expect("chat messages"),
        );
        payload.insert("stream".to_string(), Value::Bool(req.stream));
        if let Some(max_tokens) = req.max_tokens {
            payload.insert("max_tokens".to_string(), json!(max_tokens));
        }
        for (key, value) in req.params {
            payload.insert(key, value);
        }

        let response = self
            .send_json(|| {
                self.auth(self.client.post(self.endpoint("/v1/chat/completions")))
                    .json(&payload)
            })
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("chat request failed with {}", response.status()));
        }

        let body: ChatCompletionsResponse = response.json().await?;
        let content = body
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .ok_or_else(|| anyhow!("chat response missing choice"))?;

        Ok(ChatResponse {
            model: body.model.unwrap_or_else(|| self.config.model_id.clone()),
            content,
            usage: body.usage.unwrap_or_default(),
        })
    }

    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        let payload = json!({
            "model": if req.model.is_empty() { self.config.model_id.clone() } else { req.model },
            "input": req.inputs,
        });
        let response = self
            .send_json(|| {
                self.auth(self.client.post(self.endpoint("/v1/embeddings")))
                    .json(&payload)
            })
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "embedding request failed with {}",
                response.status()
            ));
        }

        let body: EmbeddingsResponse = response.json().await?;
        Ok(EmbedResponse {
            model: body.model.unwrap_or_else(|| self.config.model_id.clone()),
            vectors: body
                .data
                .into_iter()
                .map(|item| item.embedding.into())
                .collect(),
            usage: body.usage.unwrap_or_default(),
        })
    }

    async fn health(&self) -> Result<ProviderHealth> {
        let health = self
            .auth(self.client.get(self.endpoint("/health")))
            .send()
            .await;
        if let Ok(response) = health {
            if response.status().is_success() {
                return Ok(ProviderHealth {
                    healthy: true,
                    message: "ok".to_string(),
                });
            }
        }

        let models = self
            .auth(self.client.get(self.endpoint("/v1/models")))
            .send()
            .await?;
        Ok(ProviderHealth {
            healthy: models.status().is_success(),
            message: models.status().to_string(),
        })
    }

    fn model_id(&self) -> &str {
        &self.config.model_id
    }
}

fn should_retry(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    model: Option<String>,
    choices: Vec<ChatChoice>,
    usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageBody,
}

#[derive(Debug, Deserialize)]
struct ChatMessageBody {
    content: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    model: Option<String>,
    data: Vec<EmbeddingDatum>,
    usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use axum::{
        extract::State,
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use provider_api::{ChatMessage, ModelProvider};
    use serde_json::{json, Value};
    use tokio::net::TcpListener;

    use super::{OpenAiCompatConfig, OpenAiCompatProvider};

    #[derive(Clone)]
    struct MockState {
        attempts: Arc<AtomicUsize>,
    }

    #[tokio::test]
    async fn openai_provider_retries_on_429() {
        let state = MockState {
            attempts: Arc::new(AtomicUsize::new(0)),
        };
        let base_url = spawn_mock_server(state.clone()).await;
        let mut config = OpenAiCompatConfig::new(base_url, "mock-model");
        config.retries = 2;
        let provider = OpenAiCompatProvider::new(config);

        let response = provider
            .chat(provider_api::ChatRequest {
                model: String::new(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                }],
                max_tokens: Some(32),
                stream: false,
                params: Default::default(),
            })
            .await
            .expect("chat");

        assert_eq!(response.content, "retried response");
        assert_eq!(state.attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn openai_provider_embeds_inputs() {
        let state = MockState {
            attempts: Arc::new(AtomicUsize::new(1)),
        };
        let base_url = spawn_mock_server(state).await;
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(base_url, "mock-model"));

        let response = provider
            .embed(provider_api::EmbedRequest {
                model: String::new(),
                inputs: vec!["alpha".to_string(), "beta".to_string()],
                batch_size: 2,
            })
            .await
            .expect("embed");

        assert_eq!(response.vectors.len(), 2);
        assert_eq!(&*response.vectors[0], &[1.0, 0.0]);
    }

    async fn spawn_mock_server(state: MockState) -> String {
        let app = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat))
            .route("/v1/embeddings", post(embeddings))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        format!("http://{addr}")
    }

    async fn models() -> impl IntoResponse {
        Json(json!({ "data": [{ "id": "mock-model" }] }))
    }

    async fn chat(State(state): State<MockState>, Json(_body): Json<Value>) -> impl IntoResponse {
        let attempt = state.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({ "error": "slow down" })),
            )
                .into_response();
        }
        (
            StatusCode::OK,
            Json(json!({
                "model": "mock-model",
                "choices": [{ "message": { "content": "retried response" } }],
                "usage": { "prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3 }
            })),
        )
            .into_response()
    }

    async fn embeddings(Json(body): Json<Value>) -> impl IntoResponse {
        let inputs = body["input"].as_array().cloned().unwrap_or_default();
        let data: Vec<Value> = inputs
            .iter()
            .enumerate()
            .map(|(index, _)| json!({ "embedding": [1.0 - index as f32, index as f32] }))
            .collect();
        (
            StatusCode::OK,
            Json(json!({
                "model": "mock-model",
                "data": data,
                "usage": { "prompt_tokens": 2, "completion_tokens": 0, "total_tokens": 2 }
            })),
        )
    }
}
