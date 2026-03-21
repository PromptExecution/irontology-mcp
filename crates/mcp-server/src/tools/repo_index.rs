use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use indexer::chunking::chunk_text;
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::{json, Value};
use storage_neumann::{EmbeddingModality, EmbeddingRecord, KnowledgeStore};

use crate::Tool;

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
        "Index content into the knowledge store (chunk, embed, upsert)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string" },
                "content": { "type": "string" },
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

        let chunks = chunk_text(content, 512);
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

        let mut records = Vec::with_capacity(embeddings.vectors.len().min(chunks.len()));
        for (index, (chunk, vector)) in chunks.iter().zip(embeddings.vectors).enumerate() {
            let chunk_id =
                blake3::hash(format!("{topic}\n{source_ref}\n{index}\n{chunk}").as_bytes())
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

        let chunks_created = records.len();
        self.store.upsert_embeddings(records).await?;

        Ok(json!({
            "chunks_created": chunks_created
        }))
    }
}
