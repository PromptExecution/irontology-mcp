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
    pub anchor_locator: Option<String>,
    pub artifact_uri: Option<String>,
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
    let mut citations: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();

    let merge_citation = |citations: &mut HashMap<String, (Option<String>, Option<String>)>, item: &RankedResult| {
        let entry = citations.entry(item.id.clone()).or_insert((None, None));
        if entry.0.is_none() {
            entry.0 = item.anchor_locator.clone();
        }
        if entry.1.is_none() {
            entry.1 = item.artifact_uri.clone();
        }
    };

    for item in backend.search_vector(query, top_k * 3)? {
        *scores.entry(item.id.clone()).or_insert(0.0) += item.score * weights.vector;
        merge_citation(&mut citations, &item);
    }
    for item in backend.search_graph(query, top_k * 3)? {
        *scores.entry(item.id.clone()).or_insert(0.0) += item.score * weights.graph;
        merge_citation(&mut citations, &item);
    }
    for item in backend.search_lexical(query, top_k * 3)? {
        *scores.entry(item.id.clone()).or_insert(0.0) += item.score * weights.lexical;
        merge_citation(&mut citations, &item);
    }
    for item in backend.search_ontology(query, top_k * 3)? {
        *scores.entry(item.id.clone()).or_insert(0.0) += item.score * weights.ontology;
        merge_citation(&mut citations, &item);
    }

    let mut out: Vec<_> = scores
        .into_iter()
        .map(|(id, score)| {
            let (anchor_locator, artifact_uri) = citations.remove(&id).unwrap_or((None, None));
            RankedResult { id, score, anchor_locator, artifact_uri }
        })
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
