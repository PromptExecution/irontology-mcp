use std::collections::HashMap;

use anyhow::Result;
use storage_neumann::{NeumannStore, StoreSnapshot};

use crate::fusion::{RankedResult, SearchBackend};

#[derive(Debug, Clone)]
pub struct StoreBackedBackend {
    snapshot: StoreSnapshot,
}

impl StoreBackedBackend {
    pub fn from_snapshot(snapshot: StoreSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn from_store(store: &NeumannStore) -> Self {
        Self {
            snapshot: store.snapshot(),
        }
    }

    pub fn snapshot(&self) -> &StoreSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone)]
pub struct DeterministicBackend {
    inner: StoreBackedBackend,
}

impl Default for DeterministicBackend {
    fn default() -> Self {
        Self {
            inner: StoreBackedBackend::from_snapshot(sample_snapshot()),
        }
    }
}

impl DeterministicBackend {
    pub fn from_snapshot(snapshot: StoreSnapshot) -> Self {
        Self {
            inner: StoreBackedBackend::from_snapshot(snapshot),
        }
    }

    pub fn from_store(store: &NeumannStore) -> Self {
        Self {
            inner: StoreBackedBackend::from_store(store),
        }
    }
}

impl SearchBackend for DeterministicBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        self.inner.search_vector(query, top_k)
    }

    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        self.inner.search_graph(query, top_k)
    }

    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        self.inner.search_lexical(query, top_k)
    }

    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        self.inner.search_ontology(query, top_k)
    }
}

impl SearchBackend for StoreBackedBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        let terms = tokenize(query);
        let mut scored: Vec<_> = self
            .snapshot
            .embeddings
            .iter()
            .filter_map(|embedding| {
                let query_vector = query_vector(&terms, embedding.vector.len());
                let score = cosine(&embedding.vector, &query_vector) * embedding.semantic_weight.max(0.0);
                (score > 0.0).then(|| {
                    let artifact_uri = embedding.artifact_locator.as_deref()
                        .and_then(|locator| {
                            self.snapshot.artifacts.iter()
                                .find(|a| a.locator == locator)
                                .map(|a| a.source_uri.clone())
                        });
                    RankedResult {
                        id: embedding.id.clone(),
                        score,
                        anchor_locator: embedding.anchor_id.as_deref()
                            .and_then(|aid| {
                                self.snapshot.anchors.iter()
                                    .find(|a| a.id == aid)
                                    .map(|a| a.locator.clone())
                            }),
                        artifact_uri,
                    }
                })
            })
            .collect();
        sort_and_truncate(&mut scored, top_k);
        Ok(scored)
    }

    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        let terms = tokenize(query);
        let mut scores: HashMap<String, f32> = HashMap::new();

        for edge in &self.snapshot.edges {
            let direct = score_text(&terms, &edge.from)
                .max(score_text(&terms, &edge.to))
                .max(score_text(&terms, &format!("{:?}", edge.kind)));
            if direct <= 0.0 {
                continue;
            }

            let edge_boost = 1.0 + (edge.weight as f32 / 100.0);
            *scores.entry(edge.from.clone()).or_insert(0.0) += direct * edge_boost;
            *scores.entry(edge.to.clone()).or_insert(0.0) += direct * edge_boost;
        }

        for fact in &self.snapshot.facts {
            let direct = score_text(&terms, &fact.subject)
                .max(score_text(&terms, &fact.predicate))
                .max(score_text(&terms, &fact.object.to_string()));
            if direct <= 0.0 {
                continue;
            }
            *scores.entry(fact.subject.clone()).or_insert(0.0) += direct;
        }

        for symbol in &self.snapshot.symbols {
            let direct = score_text(&terms, &symbol.id)
                .max(score_text(&terms, &symbol.path))
                .max(score_text(&terms, &symbol.name))
                .max(score_text(&terms, &symbol.kind))
                .max(score_text(&terms, &symbol.content));
            if direct <= 0.0 {
                continue;
            }
            *scores.entry(symbol.id.clone()).or_insert(0.0) += direct * 1.25;
            if let Some(existing) = scores.get(&symbol.id).copied() {
                *scores.entry(symbol.path.clone()).or_insert(0.0) += existing * 0.25;
            }
        }

        neighborhood_boost(&self.snapshot.edges, &mut scores);

        Ok(ranked(scores, top_k))
    }

    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        let terms = tokenize(query);
        let mut scores: HashMap<String, f32> = HashMap::new();

        for file in &self.snapshot.files {
            let score = score_text(&terms, &file.path)
                .max(score_text(&terms, &file.id))
                .max(score_text(&terms, &file.media_type));
            if score > 0.0 {
                *scores.entry(file.id.clone()).or_insert(0.0) += score * 1.5;
            }
        }

        for symbol in &self.snapshot.symbols {
            let score = score_text(&terms, &symbol.id)
                .max(score_text(&terms, &symbol.path))
                .max(score_text(&terms, &symbol.name))
                .max(score_text(&terms, &symbol.kind))
                .max(score_text(&terms, &symbol.signature.clone().unwrap_or_default()))
                .max(score_text(&terms, &symbol.content));
            if score > 0.0 {
                *scores.entry(symbol.id.clone()).or_insert(0.0) += score * 1.75;
            }
        }

        for fact in &self.snapshot.facts {
            let score = score_text(&terms, &fact.subject)
                .max(score_text(&terms, &fact.predicate))
                .max(score_text(&terms, &fact.object.to_string()));
            if score > 0.0 {
                *scores.entry(fact.subject.clone()).or_insert(0.0) += score;
            }
        }

        for triple in &self.snapshot.semantic_triples {
            let score = score_text(&terms, &triple.source)
                .max(score_text(&terms, &triple.subject))
                .max(score_text(&terms, &triple.predicate))
                .max(score_text(&terms, &triple.object));
            if score > 0.0 {
                *scores.entry(triple.subject.clone()).or_insert(0.0) += score * 1.2;
            }
        }

        Ok(ranked(scores, top_k))
    }

    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
        let terms = tokenize(query);
        let mut scores: HashMap<String, f32> = HashMap::new();

        for class in self.snapshot.ontology_classes() {
            let score = score_text(&terms, &class);
            if score > 0.0 {
                scores.insert(class, score * 2.0);
            }
        }

        for symbol in &self.snapshot.symbols {
            let score = score_text(&terms, &symbol.kind);
            if score > 0.0 {
                *scores.entry(symbol.kind.clone()).or_insert(0.0) += score;
            }
        }

        for triple in &self.snapshot.semantic_triples {
            let score = score_text(&terms, &triple.predicate)
                .max(score_text(&terms, &triple.object));
            if score > 0.0 {
                *scores.entry(triple.subject.clone()).or_insert(0.0) += score;
            }
        }

        for fact in &self.snapshot.facts {
            if matches!(fact.predicate.as_str(), "class" | "shape" | "ontology_ref") {
                let score = score_text(&terms, &fact.object.to_string());
                if score > 0.0 {
                    *scores.entry(fact.subject.clone()).or_insert(0.0) += score;
                }
            }
        }

        Ok(ranked(scores, top_k))
    }
}

fn sample_snapshot() -> StoreSnapshot {
    StoreSnapshot {
        files: vec![],
        symbols: vec![
            storage_neumann::SymbolRecord {
                id: "sym:alpha".to_string(),
                blob: "blob-alpha".to_string(),
                path: "src/alpha.rs".to_string(),
                name: "alpha".to_string(),
                kind: "Function".to_string(),
                start_line: 1,
                end_line: 3,
                signature: Some("fn alpha()".to_string()),
                content: "alpha beta".to_string(),
            },
            storage_neumann::SymbolRecord {
                id: "sym:beta".to_string(),
                blob: "blob-beta".to_string(),
                path: "src/beta.rs".to_string(),
                name: "beta".to_string(),
                kind: "Type".to_string(),
                start_line: 1,
                end_line: 3,
                signature: Some("struct Beta".to_string()),
                content: "beta".to_string(),
            },
        ],
        embeddings: vec![
            storage_neumann::EmbeddingRecord {
                id: "sym:alpha".to_string(),
                source_blob: "blob-alpha".to_string(),
                vector: std::sync::Arc::from([1.0_f32, 0.0_f32]),
                modality: storage_neumann::EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
                anchor_id: None,
                artifact_locator: None,
            },
            storage_neumann::EmbeddingRecord {
                id: "sym:beta".to_string(),
                source_blob: "blob-beta".to_string(),
                vector: std::sync::Arc::from([0.0_f32, 1.0_f32]),
                modality: storage_neumann::EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
                anchor_id: None,
                artifact_locator: None,
            },
        ],
        facts: vec![storage_neumann::FactRecord {
            subject: "sym:alpha".to_string(),
            predicate: "class".to_string(),
            object: serde_json::json!("Function"),
        }],
        edges: vec![storage_neumann::EdgeRecord {
            from: "sym:alpha".to_string(),
            to: "sym:beta".to_string(),
            kind: storage_neumann::EdgeKind::Calls,
            weight: 100,
        }],
        semantic_triples: vec![storage_neumann::SemanticTriple {
            source: "ontology://sample".to_string(),
            subject: "https://example.org/pe/Topic".to_string(),
            predicate: "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
            object: "https://example.org/pe/Concept".to_string(),
        }],
        artifacts: vec![],
        anchors: vec![],
        observations: vec![],
    }
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn query_vector(tokens: &[String], dims: usize) -> Vec<f32> {
    let dims = dims.max(1);
    let mut vector = vec![0.0_f32; dims];

    for (index, token) in tokens.iter().enumerate() {
        let hashed = stable_hash(token) as usize;
        let slot = hashed % dims;
        let weight = 1.0 / ((index + 1) as f32);
        vector[slot] += weight;
    }

    vector
}

fn score_text(tokens: &[String], text: &str) -> f32 {
    let haystack = text.to_ascii_lowercase();
    let mut score = 0.0;

    for (index, token) in tokens.iter().enumerate() {
        if haystack.contains(token) {
            score += 1.0 / ((index + 1) as f32);
        }
    }

    if score > 0.0 {
        score / (tokens.len().max(1) as f32)
    } else {
        0.0
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

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn neighborhood_boost(edges: &[storage_neumann::EdgeRecord], scores: &mut HashMap<String, f32>) {
    let mut neighbors: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in edges {
        neighbors.entry(&edge.from).or_default().push(&edge.to);
        neighbors.entry(&edge.to).or_default().push(&edge.from);
    }

    let mut seeds: Vec<String> = scores.keys().cloned().collect();
    seeds.sort();
    for seed in seeds {
        let Some(neighbor_list) = neighbors.get(seed.as_str()) else {
            continue;
        };

        let seed_score = scores.get(&seed).copied().unwrap_or_default();
        let mut sorted_neighbors: Vec<&str> = neighbor_list.clone();
        sorted_neighbors.sort_unstable();
        for neighbor in sorted_neighbors {
            *scores.entry(neighbor.to_string()).or_insert(0.0) += seed_score * 0.25;
        }
    }
}

fn ranked(scores: HashMap<String, f32>, top_k: usize) -> Vec<RankedResult> {
    let mut out: Vec<_> = scores
        .into_iter()
        .filter(|(_, score)| *score > 0.0)
        .map(|(id, score)| RankedResult {
            id,
            score,
            anchor_locator: None,
            artifact_uri: None,
        })
        .collect();
    sort_and_truncate(&mut out, top_k);
    out
}

fn sort_and_truncate(results: &mut Vec<RankedResult>, top_k: usize) {
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    results.truncate(top_k);
}
