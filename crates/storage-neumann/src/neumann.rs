use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use async_trait::async_trait;

use crate::config::NeumannConfig;

#[derive(Debug, Clone)]
pub struct EmbeddingRecord {
    pub id: String,
    pub source_blob: String,
    pub vector: Arc<[f32]>,
}

pub enum SemanticQuery {
    Vector { embedding: Arc<[f32]>, top_k: usize },
}

pub struct QueryResult {
    pub ids: Vec<String>,
}

#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn has_blob(&self, blob_id: &str) -> Result<bool>;
    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()>;
    async fn query(&self, q: SemanticQuery) -> Result<QueryResult>;
}

pub struct NeumannStore {
    _config: NeumannConfig,
    embeddings: RwLock<HashMap<String, EmbeddingRecord>>,
    blobs: RwLock<HashMap<String, bool>>,
}

impl NeumannStore {
    pub fn new(config: NeumannConfig) -> Self {
        Self {
            _config: config,
            embeddings: RwLock::new(HashMap::new()),
            blobs: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl KnowledgeStore for NeumannStore {
    async fn has_blob(&self, blob_id: &str) -> Result<bool> {
        Ok(self.blobs.read().expect("blobs").contains_key(blob_id))
    }

    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        let mut blobs = self.blobs.write().expect("blobs");
        let mut map = self.embeddings.write().expect("embeddings");
        for emb in embeddings {
            blobs.insert(emb.source_blob.clone(), true);
            map.insert(emb.id.clone(), emb);
        }
        Ok(())
    }

    async fn query(&self, q: SemanticQuery) -> Result<QueryResult> {
        match q {
            SemanticQuery::Vector { embedding, top_k } => {
                let mut scored: Vec<(String, f32)> = self
                    .embeddings
                    .read()
                    .expect("embeddings")
                    .values()
                    .map(|record| (record.id.clone(), cosine(&record.vector, &embedding)))
                    .collect();

                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
                scored.truncate(top_k);
                Ok(QueryResult {
                    ids: scored.into_iter().map(|(id, _)| id).collect(),
                })
            }
        }
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0, 0.0, 0.0);
    for i in 0..len {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}
