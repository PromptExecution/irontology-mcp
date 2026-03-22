use std::sync::Arc;

use anyhow::Result;
use storage_neumann::{KnowledgeStore, NeumannStore, SemanticQuery};

use crate::embed::EmbeddingClient;
use crate::fusion::RankedResult;

/// Real VectorBackend: embeds query, cosine-searches NeumannStore.
///
/// `search_vector` uses `block_in_place` to bridge sync SearchBackend trait → async embed.
/// Requires tokio multi-thread runtime (mcp-server already uses rt-multi-thread).
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
        let store = self.store.clone();
        let embedder = self.embedder.clone();
        let query = query.to_string();

        tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async move {
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
    let mut out = seed(query, "vec");
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(top_k);
    Ok(out)
}

fn seed(query: &str, ns: &str) -> Vec<RankedResult> {
    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return vec![];
    }

    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| RankedResult {
            id: format!("{ns}:{t}"),
            score: 1.0 / ((i + 1) as f32),
        })
        .collect()
}
