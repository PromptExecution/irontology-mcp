//! Embedding client for irontology-mcp
//!
//! Calls OpenAI-compatible /v1/embeddings endpoint.
//! Primary: llama-server at http://localhost:8000 (podman CUDA container)
//! Fallback: ollama at http://localhost:11434/api/embeddings
//!
//! Env vars:
//!   EMBEDDING_ENDPOINT  — base URL (default: http://localhost:8000)
//!   EMBEDDING_MODEL     — model name (default: nomic-embed-text)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

const DEFAULT_ENDPOINT: &str = "http://localhost:8000";
const DEFAULT_MODEL: &str = "nomic-embed-text";

#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl EmbeddingClient {
    pub fn new() -> Self {
        let endpoint = std::env::var("EMBEDDING_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        let model = std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self {
            endpoint,
            model,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_endpoint(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Embed a text string → Vec<f32>. Uses OpenAI-compatible /v1/embeddings.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/v1/embeddings", self.endpoint.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "input": text,
        });

        #[derive(Deserialize)]
        struct EmbeddingData {
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct EmbeddingResponse {
            data: Vec<EmbeddingData>,
        }

        let resp: EmbeddingResponse = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow::anyhow!("embed: empty data array from {url}"))
    }

    /// Check if the embedding endpoint is reachable
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/health", self.endpoint.trim_end_matches('/'));
        self.client.get(&url).send().await.is_ok()
    }
}

impl Default for EmbeddingClient {
    fn default() -> Self {
        Self::new()
    }
}
