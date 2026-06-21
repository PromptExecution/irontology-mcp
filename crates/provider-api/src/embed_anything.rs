//! `ModelProvider` wrapper around [`b00t_embed::EmbedBackend`].
//!
//! This module is gated behind the `embed-anything` feature flag.
//!
//! [`EmbedAnythingProvider`] implements [`ModelProvider`](crate::ModelProvider) for
//! embedding-only backends from `b00t-embed`. The `chat()` and `health()` methods
//! return stubs — use a chat-capable provider (e.g. `provider-openai`) for those.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use b00t_embed::EmbedBackend;

use crate::{
    ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, ModelProvider, ProviderHealth,
    TokenUsage,
};

/// Wraps any [`b00t_embed::EmbedBackend`] as a [`ModelProvider`].
///
/// Only the `embed()` method delegates to the real backend; `chat()` returns an
/// empty response, and `health()` reflects the backend's availability.
#[derive(Clone)]
pub struct EmbedAnythingProvider {
    backend: Arc<dyn EmbedBackend>,
    model_id: String,
}

impl EmbedAnythingProvider {
    /// Wrap an existing [`EmbedBackend`] into a [`ModelProvider`].
    pub fn new(backend: Arc<dyn EmbedBackend>) -> Self {
        let model_id = backend.model_id().to_string();
        Self { backend, model_id }
    }
}

#[async_trait]
impl ModelProvider for EmbedAnythingProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse {
            model: self.model_id.clone(),
            content: String::new(),
            usage: TokenUsage::default(),
        })
    }

    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        let texts: Vec<&str> = req.inputs.iter().map(|s| s.as_str()).collect();
        let embeddings = self.backend.embed_batch(&texts).await?;
        let vectors = embeddings
            .into_iter()
            .map(|e| Arc::from(e.data.as_slice()))
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
            healthy: self.backend.is_available(),
            message: format!("b00t-embed {} available", self.model_id),
        })
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
