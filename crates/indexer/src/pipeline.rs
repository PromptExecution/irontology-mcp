use std::{path::Path, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;

use crate::{chunking::chunk_text, embedding::Modality};

#[derive(Debug, Clone)]
pub struct IntakeFile {
    pub path: String,
}

impl IntakeFile {
    pub fn from_path(path: &Path) -> Self {
        Self {
            path: path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Extraction {
    pub text: String,
    pub has_symbols: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EmbedRequest {
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EmbedResponse {
    pub vectors: Vec<Arc<[f32]>>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRecord {
    pub source_blob: String,
    pub vector: Arc<[f32]>,
    pub modality: Modality,
}

#[async_trait]
pub trait GitLedger: Send + Sync {
    async fn blob_id(&self, path: &Path) -> Result<String>;
}

pub trait RuleMatcher: Send + Sync {
    fn match_file(&self, file: &IntakeFile) -> bool;
}

#[async_trait]
pub trait Handler: Send + Sync {
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction>;
}

#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn has_blob(&self, blob_id: &str) -> Result<bool>;
    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()>;
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse>;
}

pub async fn index_file(
    path: &Path,
    git_ledger: &dyn GitLedger,
    rules: &dyn RuleMatcher,
    handler: &dyn Handler,
    store: &dyn KnowledgeStore,
    provider: &dyn ModelProvider,
) -> Result<bool> {
    let blob_id = git_ledger.blob_id(path).await?;
    if store.has_blob(&blob_id).await? {
        return Ok(false);
    }

    let intake = IntakeFile::from_path(path);
    if !rules.match_file(&intake) {
        return Ok(false);
    }

    let extraction = handler.extract(&intake).await?;
    let chunks = chunk_text(&extraction.text, 512);
    if chunks.is_empty() {
        return Ok(false);
    }

    let embeddings = provider.embed(EmbedRequest { inputs: chunks }).await?;
    let mut records = Vec::new();
    for vector in embeddings.vectors {
        records.push(EmbeddingRecord {
            source_blob: blob_id.clone(),
            vector,
            modality: if extraction.has_symbols {
                Modality::CodeSymbol
            } else {
                Modality::DocChunk
            },
        });
    }
    store.upsert_embeddings(records).await?;
    Ok(true)
}
