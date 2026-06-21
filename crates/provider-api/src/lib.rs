#[cfg(feature = "embed-anything")]
pub mod embed_anything;

use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub model: String,
    pub content: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedRequest {
    pub model: String,
    pub inputs: Vec<String>,
    pub batch_size: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbedResponse {
    pub model: String,
    pub vectors: Vec<Arc<[f32]>>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub healthy: bool,
    pub message: String,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse>;
    async fn health(&self) -> Result<ProviderHealth>;
    fn model_id(&self) -> &str;
}
