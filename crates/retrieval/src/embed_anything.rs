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
use b00t_embed::{EmbedAnythingBackend, EmbedConfig, EmbedProvider};

/// Extract the model identifier from an [`EmbedConfig`] regardless of provider variant.
fn model_id_from_config(config: &EmbedConfig) -> String {
    match &config.provider {
        EmbedProvider::HuggingFace { model_id, .. } => model_id.clone(),
        EmbedProvider::ONNX { model_id } => model_id.clone(),
        EmbedProvider::Cloud { model_id, .. } => model_id.clone(),
    }
}

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
        let model_id = model_id_from_config(&config);
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

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Unit tests for config → model_id extraction (no backend required)
    // -------------------------------------------------------------------------

    #[test]
    fn model_id_from_huggingface_config() {
        let config = EmbedConfig {
            provider: EmbedProvider::HuggingFace {
                model_id: "org/my-model".into(),
                revision: Some("main".into()),
            },
            batch_size: 16,
            chunk_size: 512,
        };
        assert_eq!(model_id_from_config(&config), "org/my-model");
    }

    #[test]
    fn model_id_from_onnx_config() {
        let config = EmbedConfig {
            provider: EmbedProvider::ONNX {
                model_id: "onnx-model-v1".into(),
            },
            batch_size: 8,
            chunk_size: 256,
        };
        assert_eq!(model_id_from_config(&config), "onnx-model-v1");
    }

    #[test]
    fn model_id_from_cloud_config() {
        let config = EmbedConfig {
            provider: EmbedProvider::Cloud {
                model_id: "text-embedding-3".into(),
                api_key: "sk-test".into(),
                base_url: Some("https://api.example.com".into()),
            },
            batch_size: 32,
            chunk_size: 512,
        };
        assert_eq!(model_id_from_config(&config), "text-embedding-3");
    }

    // -------------------------------------------------------------------------
    // Integration tests (require model weights — #[ignore] by default)
    // Run with: cargo test -p retrieval -- --ignored embed_anything
    // -------------------------------------------------------------------------

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_new_defaults() {
        let client = EmbedAnythingClient::new().await.expect("new()");
        assert!(client.model_id().contains("jina"));
    }

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_with_config_huggingface() {
        let config = EmbedConfig {
            provider: EmbedProvider::HuggingFace {
                model_id: "Xenova/all-MiniLM-L6-v2".into(),
                revision: None,
            },
            batch_size: 16,
            chunk_size: 512,
        };
        let client = EmbedAnythingClient::with_config(config)
            .await
            .expect("with_config");
        assert_eq!(client.model_id(), "Xenova/all-MiniLM-L6-v2");
    }

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_embed_returns_vector() {
        let client = EmbedAnythingClient::new().await.expect("new()");
        let embedding = client.embed("hello world").await.expect("embed");
        assert!(!embedding.is_empty(), "expected non-empty embedding vector");
    }

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_is_available_after_construction() {
        let client = EmbedAnythingClient::new().await.expect("new()");
        assert!(client.is_available().await);
    }

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_model_id_accessor() {
        let client = EmbedAnythingClient::new().await.expect("new()");
        assert!(!client.model_id().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires downloading model weights via b00t-embed"]
    async fn embed_anything_empty_text() {
        let client = EmbedAnythingClient::new().await.expect("new()");
        let embedding = client.embed("").await.expect("embed empty string");
        assert!(!embedding.is_empty());
    }
}
