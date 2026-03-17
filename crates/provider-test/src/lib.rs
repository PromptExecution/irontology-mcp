use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use provider_api::{
    ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, ModelProvider, ProviderHealth,
    TokenUsage,
};

#[derive(Debug, Clone)]
pub struct FixtureProvider {
    model_id: String,
    chat_content: String,
    embedding_dim: usize,
}

impl FixtureProvider {
    pub fn new(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            chat_content: "fixture response".to_string(),
            embedding_dim: 4,
        }
    }

    pub fn with_chat_content(mut self, chat_content: impl Into<String>) -> Self {
        self.chat_content = chat_content.into();
        self
    }

    pub fn with_embedding_dim(mut self, embedding_dim: usize) -> Self {
        self.embedding_dim = embedding_dim.max(1);
        self
    }
}

#[async_trait]
impl ModelProvider for FixtureProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse {
            model: self.model_id.clone(),
            content: self.chat_content.clone(),
            usage: TokenUsage {
                prompt_tokens: 8,
                completion_tokens: 4,
                total_tokens: 12,
            },
        })
    }

    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        let vectors = req
            .inputs
            .iter()
            .map(|input| deterministic_vector(input, self.embedding_dim))
            .collect();
        Ok(EmbedResponse {
            model: self.model_id.clone(),
            vectors,
            usage: TokenUsage {
                prompt_tokens: req.inputs.len() as u32,
                completion_tokens: 0,
                total_tokens: req.inputs.len() as u32,
            },
        })
    }

    async fn health(&self) -> Result<ProviderHealth> {
        Ok(ProviderHealth {
            healthy: true,
            message: "fixture ready".to_string(),
        })
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

fn deterministic_vector(input: &str, dim: usize) -> Arc<[f32]> {
    let mut out = vec![0.0_f32; dim];
    for (index, byte) in input.bytes().enumerate() {
        out[index % dim] += byte as f32 / 255.0;
    }
    Arc::from(out)
}

#[cfg(test)]
mod tests {
    use provider_api::{ChatRequest, EmbedRequest, ModelProvider};

    use crate::FixtureProvider;

    #[tokio::test]
    async fn provider_test_is_deterministic() {
        let provider = FixtureProvider::new("fixture-model")
            .with_chat_content("deterministic")
            .with_embedding_dim(3);

        let chat_a = provider
            .chat(ChatRequest {
                model: String::new(),
                messages: vec![],
                max_tokens: None,
                stream: false,
                params: Default::default(),
            })
            .await
            .expect("chat a");
        let chat_b = provider
            .chat(ChatRequest {
                model: String::new(),
                messages: vec![],
                max_tokens: None,
                stream: false,
                params: Default::default(),
            })
            .await
            .expect("chat b");
        let embed_a = provider
            .embed(EmbedRequest {
                model: String::new(),
                inputs: vec!["alpha".to_string()],
                batch_size: 1,
            })
            .await
            .expect("embed a");
        let embed_b = provider
            .embed(EmbedRequest {
                model: String::new(),
                inputs: vec!["alpha".to_string()],
                batch_size: 1,
            })
            .await
            .expect("embed b");

        assert_eq!(chat_a, chat_b);
        assert_eq!(embed_a.model, embed_b.model);
        assert_eq!(&*embed_a.vectors[0], &*embed_b.vectors[0]);
    }
}
