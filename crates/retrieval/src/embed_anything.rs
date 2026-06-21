//! EmbedAnythingClient — wraps `b00t_embed::EmbedAnythingBackend`
//!
//! Provides the same API surface as [`EmbeddingClient`](crate::embed::EmbeddingClient)
//! for drop-in compatibility. Uses `embed_anything` under the hood via the `b00t-embed` adapter.
//!
//! # Primary backend
//! HuggingFace models via Candle (default: `jinaai/jina-embeddings-v2-small-en`)
//!
//! # Env vars
//! - `B00T_EMBED_MODEL`    — HuggingFace model ID (default: `jinaai/jina-embeddings-v2-small-en`)
//! - `B00T_EMBED_REVISION` — model revision or branch (default: none → latest)
//! - `B00T_EMBED_BATCH`    — batch size for embed_batch (default: 32)

use anyhow::Result;
use b00t_embed::{EmbedAnythingBackend, EmbedBackend, EmbedConfig, EmbedProvider};

/// Wraps [`b00t_embed::EmbedAnythingBackend`] in the same public API as [`crate::embed::EmbeddingClient`].
///
/// Constructors are async because the backend must download / load model weights.
/// Once constructed, `embed()` and `is_available()` mirror `EmbeddingClient` exactly.
pub struct EmbedAnythingClient {
    backend: EmbedAnythingBackend,
    model_id: String,
}

impl EmbedAnythingClient {
    /// Create with default config from env vars (falling back to HuggingFace defaults).
    ///
    /// Env vars:
    /// - `B00T_EMBED_MODEL` → model ID
    /// - `B00T_EMBED_REVISION` → optional revision
    /// - `B00T_EMBED_BATCH` → batch size
    pub async fn new() -> Result<Self> {
        let model_id = std::env::var("B00T_EMBED_MODEL")
            .unwrap_or_else(|_| "jinaai/jina-embeddings-v2-small-en".to_string());
        let revision = std::env::var("B00T_EMBED_REVISION").ok();
        let batch_size = std::env::var("B00T_EMBED_BATCH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32);

        let config = EmbedConfig {
            provider: EmbedProvider::HuggingFace {
                model_id: model_id.clone(),
                revision,
            },
            batch_size,
            chunk_size: 512,
        };

        let backend = EmbedAnythingBackend::new(config).await?;
        Ok(Self { backend, model_id })
    }

    /// Create with an explicit [`EmbedConfig`].
    ///
    /// Useful when you want to specify `ONNX` or `Cloud` providers instead of HuggingFace.
    pub async fn with_config(config: EmbedConfig) -> Result<Self> {
        // Extract model_id from whichever provider variant was supplied
        let model_id = match &config.provider {
            EmbedProvider::HuggingFace { model_id, .. } => model_id.clone(),
            EmbedProvider::ONNX { model_id } => model_id.clone(),
            EmbedProvider::Cloud { model_id, .. } => model_id.clone(),
        };
        let backend = EmbedAnythingBackend::new(config).await?;
        Ok(Self { backend, model_id })
    }

    /// Embed a single text string → `Vec<f32>`.
    ///
    /// Mirrors [`EmbeddingClient::embed`](crate::embed::EmbeddingClient::embed).
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embedding = self.backend.embed(text).await?;
        Ok(embedding.data)
    }

    /// Check whether the backend is available (model loaded, dim > 0).
    ///
    /// Mirrors [`EmbeddingClient::is_available`](crate::embed::EmbeddingClient::is_available).
    pub async fn is_available(&self) -> bool {
        self.backend.is_available()
    }

    /// Return the model identifier string.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}
