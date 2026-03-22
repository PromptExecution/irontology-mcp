use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct FusionWeights {
    pub vector: f32,
    pub graph: f32,
    pub lexical: f32,
    pub ontology: f32,
}

impl Default for FusionWeights {
    fn default() -> Self {
        Self {
            vector: 0.35,
            graph: 0.30,
            lexical: 0.20,
            ontology: 0.15,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedResult {
    pub id: String,
    pub score: f32,
}

pub trait SearchBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>>;
    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>>;
    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>>;
    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>>;
}

pub fn fusion_search(
    query: &str,
    top_k: usize,
    weights: FusionWeights,
    backend: &dyn SearchBackend,
) -> Result<Vec<RankedResult>> {
    let mut scores: HashMap<String, f32> = HashMap::new();

    for item in backend.search_vector(query, top_k * 3)? {
        *scores.entry(item.id).or_insert(0.0) += item.score * weights.vector;
    }
    for item in backend.search_graph(query, top_k * 3)? {
        *scores.entry(item.id).or_insert(0.0) += item.score * weights.graph;
    }
    for item in backend.search_lexical(query, top_k * 3)? {
        *scores.entry(item.id).or_insert(0.0) += item.score * weights.lexical;
    }
    for item in backend.search_ontology(query, top_k * 3)? {
        *scores.entry(item.id).or_insert(0.0) += item.score * weights.ontology;
    }

    let mut out: Vec<_> = scores
        .into_iter()
        .map(|(id, score)| RankedResult { id, score })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    out.truncate(top_k);
    Ok(out)
}
