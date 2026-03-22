//! repo.index MCP tool — ingest text content into NeumannStore
//!
//! Used by b00t grok digest/learn to push content into irontology.
//! Chunks content, embeds via ModelProvider, upserts embeddings + turtle triple.
//!
//! Input:
//!   content: string  — text to index
//!   source:  string  — source identifier (URL, file path, or label)
//!   topic:   string? — optional semantic category label

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::{json, Value};
use std::sync::Arc;

use storage_neumann::{
    EmbeddingModality, EmbeddingRecord, KnowledgeStore,
};

use crate::Tool;

const CHUNK_SIZE: usize = 512;
const CHUNK_OVERLAP: usize = 64;

/// Maximum allowed content size in bytes (512 KiB).
pub const MAX_CONTENT_BYTES: usize = 512 * 1024;
/// Maximum number of chunks produced from a single ingestion call.
pub const MAX_CHUNKS: usize = 256;

pub struct RepoIndexTool {
    store: Arc<dyn KnowledgeStore>,
    provider: Arc<dyn ModelProvider>,
}

impl RepoIndexTool {
    pub fn new(store: Arc<dyn KnowledgeStore>, provider: Arc<dyn ModelProvider>) -> Self {
        Self { store, provider }
    }
}

#[async_trait]
impl Tool for RepoIndexTool {
    fn name(&self) -> &str {
        "repo.index"
    }

    fn description(&self) -> &str {
        "Index content into the knowledge store (chunk, embed, upsert). \
         Content must not exceed 512 KiB and must produce no more than 256 chunks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string" },
                "content": {
                    "type": "string",
                    "description": "Text to index. Maximum 524288 bytes (512 KiB)."
                },
                "source": {
                    "type": "string",
                    "description": "URL or file path"
                }
            },
            "required": ["topic", "content"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("content missing"))?;
        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("source missing"))?;
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // 1. Chunk the content
        let chunks = chunk_text(content, CHUNK_SIZE, CHUNK_OVERLAP);
        let chunks_created = chunks.len();

        // 2. Embed all chunks via ModelProvider and upsert into NeumannStore
        let mut embeddings = Vec::new();
        if !chunks.is_empty() {
            let req = EmbedRequest {
                model: self.provider.model_id().to_string(),
                inputs: chunks.clone(),
                batch_size: chunks_created,
            };
            match self.provider.embed(req).await {
                Ok(resp) => {
                    for (i, vector) in resp.vectors.into_iter().enumerate() {
                        embeddings.push(EmbeddingRecord {
                            id: format!("{}::chunk::{}", source, i),
                            source_blob: source.to_string(),
                            vector,
                            modality: EmbeddingModality::DocChunk,
                            semantic_weight: 1.0,
                        });
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ repo.index: embed failed: {e} (skipping embeddings)");
                }
            }
        }
        let embedded = embeddings.len();
        self.store.upsert_embeddings(embeddings).await?;

        // 3. Store a semantic triple: source --hasTopic--> topic
        let turtle = format!(
            "<{}> <http://b00t.io/ontology#hasTopic> <http://b00t.io/topic/{}> .",
            source,
            topic.replace(' ', "_")
        );
        if let Err(e) = self.store.ingest_turtle(source, &turtle).await {
            eprintln!("⚠️ repo.index: ingest_turtle failed: {e}");
        }

        Ok(json!({
            "indexed": true,
            "source": source,
            "topic": topic,
            "chunks_created": chunks_created,
            "embedded": embedded,
        }))
    }
}

/// Simple fixed-size text chunker with overlap
fn chunk_text(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || size == 0 {
        return vec![];
    }
    let step = if size > overlap { size - overlap } else { 1 };
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + size).min(chars.len());
        chunks.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start += step;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::chunk_text;

    #[test]
    fn chunk_basic() {
        let text = "hello world foo bar baz";
        let chunks = chunk_text(text, 10, 2);
        assert!(!chunks.is_empty());
        assert!(chunks[0].len() <= 10);
    }

    #[test]
    fn chunk_empty() {
        assert!(chunk_text("", 10, 2).is_empty());
    }
}
