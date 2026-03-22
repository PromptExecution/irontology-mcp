use anyhow::Result;

use crate::fusion::RankedResult;
use crate::store_backend::DeterministicBackend;
use crate::SearchBackend;

pub fn search(query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
    DeterministicBackend::default().search_graph(query, top_k)
}
