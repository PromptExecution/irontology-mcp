pub mod fusion;
pub mod graph;
pub mod lexical;
pub mod ontology;
pub mod vector;

pub use fusion::{fusion_search, FusionWeights, RankedResult, SearchBackend};

use anyhow::Result;

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
