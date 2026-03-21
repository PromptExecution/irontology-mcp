use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use indexer::chunking::chunk_text;
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::{json, Value};
use storage_neumann::{EmbeddingModality, EmbeddingRecord, KnowledgeStore};

use crate::Tool;

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
        let topic = params
            .get("topic")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("topic missing"))?;
        let content = params
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("content missing"))?;
        let source = params.get("source").and_then(Value::as_str);

        if content.len() > MAX_CONTENT_BYTES {
            return Err(anyhow!(
                "content exceeds maximum allowed size of {} bytes (UTF-8 byte size; got {} bytes)",
                MAX_CONTENT_BYTES,
                content.len()
            ));
        }

        let chunks = chunk_text(content, 512);

        if chunks.len() > MAX_CHUNKS {
            return Err(anyhow!(
                "content produces {} chunks which exceeds the maximum of {}",
                chunks.len(),
                MAX_CHUNKS
            ));
        }
        if chunks.is_empty() {
            return Ok(json!({ "chunks_created": 0 }));
        }

        let source_ref = source.unwrap_or("inline");
        let source_blob = blake3::hash(format!("{topic}\n{source_ref}\n{content}").as_bytes())
            .to_hex()
            .to_string();
        let embeddings = self
            .provider
            .embed(EmbedRequest {
                model: self.provider.model_id().to_string(),
                inputs: chunks.clone(),
                batch_size: 32,
            })
            .await?;

        let mut records = Vec::with_capacity(embeddings.vectors.len());
        for (index, vector) in embeddings.vectors.into_iter().enumerate() {
            let chunk_id = blake3::hash(
                format!("{topic}\n{source_ref}\n{index}\n{}", chunks[index]).as_bytes(),
            )
            .to_hex()
            .to_string();
            records.push(EmbeddingRecord {
                id: format!("repo.index:{topic}:{chunk_id}"),
                source_blob: source_blob.clone(),
                vector,
                modality: EmbeddingModality::DocChunk,
                semantic_weight: 1.0,
            });
        }

        self.store.upsert_embeddings(records).await?;

        Ok(json!({
            "chunks_created": chunks.len()
        }))
    }
}
