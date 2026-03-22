pub mod embed;
pub mod fusion;
pub mod graph;
pub mod lexical;
pub mod ontology;
pub mod store_backend;
pub mod vector;

pub use embed::EmbeddingClient;
pub use fusion::{fusion_search, FusionWeights, RankedResult, SearchBackend};
pub use store_backend::StoreBackedBackend;
pub use vector::VectorBackend;

use std::sync::Arc;

use anyhow::Result;
use storage_neumann::NeumannStore;

/// Synthetic (deterministic) backend — no external dependencies.
/// Used in tests and when NeumannStore/embeddings not available.
#[derive(Default)]
pub struct DeterministicBackend;

impl SearchBackend for DeterministicBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        vector::search(query, top_k)
    }

    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        graph::search(query, top_k)
    }

    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        lexical::search(query, top_k)
    }

    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        ontology::search(query, top_k)
    }
}

/// Real backend: VectorBackend (cosine search) + deterministic fallbacks for graph/lexical/ontology.
/// When EMBEDDING_ENDPOINT is available, vector search uses real embeddings.
pub struct NeumannBackend {
    store: Arc<NeumannStore>,
    vector: VectorBackend,
}

impl NeumannBackend {
    pub fn new(store: Arc<NeumannStore>) -> Self {
        let embedder = Arc::new(EmbeddingClient::new());
        let vector = VectorBackend::new(store.clone(), embedder);
        Self { store, vector }
    }
}

impl SearchBackend for NeumannBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        // Try real embeddings, fall back to deterministic on error (e.g. llama-server down)
        self.vector
            .search_vector_sync(query, top_k)
            .or_else(|_| vector::search(query, top_k))
    }

    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        graph::search(query, top_k)
    }

    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        lexical::search(query, top_k)
    }

    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        ontology::search(query, top_k)
    }
}
