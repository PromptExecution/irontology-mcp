use std::sync::Arc;

use anyhow::Result;
use storage_neumann::{KnowledgeStore, NeumannStore, SemanticQuery};

use crate::embed::EmbeddingClient;
use crate::fusion::{RankedResult, SearchBackend};
use crate::store_backend::DeterministicBackend;

/// Real VectorBackend: embeds query, cosine-searches NeumannStore.
///
/// `search_vector_sync` uses `block_in_place` to bridge sync SearchBackend trait → async embed.
/// Returns an error if called outside a Tokio multi-thread runtime.
pub struct VectorBackend {
    store: Arc<NeumannStore>,
    embedder: Arc<EmbeddingClient>,
}

impl VectorBackend {
    pub fn new(store: Arc<NeumannStore>, embedder: Arc<EmbeddingClient>) -> Self {
        Self { store, embedder }
    }
}

/// SearchBackend impl for VectorBackend — bridges sync trait to async embedding call
impl VectorBackend {
    pub fn search_vector_sync(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| anyhow::anyhow!("search_vector_sync requires a Tokio multi-thread runtime"))?;
        let store = self.store.clone();
        let embedder = self.embedder.clone();
        let query = query.to_string();

        tokio::task::block_in_place(move || {
            handle.block_on(async move {
                let embedding: Arc<[f32]> = embedder.embed(&query).await?.into();
                let result = store
                    .query(SemanticQuery::Vector {
                        embedding,
                        top_k,
                        modality: None,
                    })
                    .await?;

                Ok(result
                    .ids
                    .into_iter()
                    .enumerate()
                    .map(|(i, id)| RankedResult {
                        id,
                        score: 1.0 / ((i + 1) as f32),
                    })
                    .collect())
            })
        })
    }
}

/// Deterministic (synthetic) fallback — used when no NeumannStore is available
pub fn search(query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
    DeterministicBackend::default().search_vector(query, top_k)
}
