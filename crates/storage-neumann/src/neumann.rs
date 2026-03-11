use std::{
    cmp::Ordering,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use async_trait::async_trait;
use rio_api::{
    model::{Literal, Subject, Term},
    parser::TriplesParser,
};
use rio_turtle::TurtleParser;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::NeumannConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: String,
    pub blob: String,
    pub path: String,
    pub media_type: String,
    pub size: u64,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactRecord {
    pub subject: String,
    pub predicate: String,
    pub object: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Defines,
    Calls,
    DependsOn,
    Tests,
    Contains,
    ClassifiedAs,
    StoredIn,
    Related,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingModality {
    CodeSymbol,
    CodeChunk,
    DocChunk,
    OntologyNode,
    TestCase,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRecord {
    pub id: String,
    pub source_blob: String,
    pub vector: Arc<[f32]>,
    pub modality: EmbeddingModality,
    pub semantic_weight: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTriple {
    pub source: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

pub enum SemanticQuery {
    Vector {
        embedding: Arc<[f32]>,
        top_k: usize,
        modality: Option<EmbeddingModality>,
    },
    Files {
        path: Option<String>,
        blob: Option<String>,
    },
    Facts {
        subject: Option<String>,
        predicate: Option<String>,
    },
    Edges {
        from: Option<String>,
        kind: Option<EdgeKind>,
    },
}

#[derive(Default)]
pub struct QueryResult {
    pub ids: Vec<String>,
    pub files: Vec<FileRecord>,
    pub facts: Vec<FactRecord>,
    pub edges: Vec<EdgeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreHealth {
    pub healthy: bool,
    pub message: String,
}

#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    async fn has_blob(&self, blob_id: &str) -> Result<bool>;
    async fn upsert_file(&self, file: FileRecord) -> Result<()>;
    async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()>;
    async fn upsert_edges(&self, edges: Vec<EdgeRecord>) -> Result<()>;
    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()>;
    async fn ingest_turtle(&self, source: &str, turtle: &str) -> Result<()>;
    async fn related_objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>>;
    async fn query(&self, q: SemanticQuery) -> Result<QueryResult>;
    async fn health(&self) -> Result<StoreHealth>;
}

pub struct NeumannStore {
    _config: NeumannConfig,
    files: RwLock<HashMap<String, FileRecord>>,
    embeddings: RwLock<HashMap<String, EmbeddingRecord>>,
    blobs: RwLock<HashMap<String, bool>>,
    facts: RwLock<Vec<FactRecord>>,
    edges: RwLock<Vec<EdgeRecord>>,
    semantic_triples: RwLock<Vec<SemanticTriple>>,
}

impl NeumannStore {
    pub fn new(config: NeumannConfig) -> Self {
        Self {
            _config: config,
            files: RwLock::new(HashMap::new()),
            embeddings: RwLock::new(HashMap::new()),
            blobs: RwLock::new(HashMap::new()),
            facts: RwLock::new(Vec::new()),
            edges: RwLock::new(Vec::new()),
            semantic_triples: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl KnowledgeStore for NeumannStore {
    async fn has_blob(&self, blob_id: &str) -> Result<bool> {
        Ok(self.blobs.read().expect("blobs").contains_key(blob_id))
    }

    async fn upsert_file(&self, file: FileRecord) -> Result<()> {
        self.blobs
            .write()
            .expect("blobs")
            .insert(file.blob.clone(), true);
        self.files
            .write()
            .expect("files")
            .insert(file.id.clone(), file);
        Ok(())
    }

    async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()> {
        let mut stored = self.facts.write().expect("facts");
        for fact in facts {
            if !stored.iter().any(|candidate| {
                candidate.subject == fact.subject
                    && candidate.predicate == fact.predicate
                    && candidate.object == fact.object
            }) {
                stored.push(fact);
            }
        }
        Ok(())
    }

    async fn upsert_edges(&self, edges: Vec<EdgeRecord>) -> Result<()> {
        let mut stored = self.edges.write().expect("edges");
        for edge in edges {
            if !stored.iter().any(|candidate| {
                candidate.from == edge.from
                    && candidate.to == edge.to
                    && candidate.kind == edge.kind
            }) {
                stored.push(edge);
            }
        }
        Ok(())
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

    async fn ingest_turtle(&self, source: &str, turtle: &str) -> Result<()> {
        let mut parsed = Vec::new();
        TurtleParser::new(turtle.as_bytes(), None).parse_all(&mut |triple| {
            parsed.push(SemanticTriple {
                source: source.to_string(),
                subject: subject_to_string(&triple.subject),
                predicate: triple.predicate.iri.to_string(),
                object: term_to_string(&triple.object),
            });
            Ok(()) as Result<(), rio_turtle::TurtleError>
        })?;

        self.semantic_triples
            .write()
            .expect("semantic_triples")
            .extend(parsed);
        Ok(())
    }

    async fn related_objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>> {
        Ok(self
            .semantic_triples
            .read()
            .expect("semantic_triples")
            .iter()
            .filter(|triple| triple.subject == subject && triple.predicate == predicate)
            .map(|triple| triple.object.clone())
            .collect())
    }

    async fn query(&self, q: SemanticQuery) -> Result<QueryResult> {
        match q {
            SemanticQuery::Vector {
                embedding,
                top_k,
                modality,
            } => {
                let mut scored: Vec<(String, f32)> = self
                    .embeddings
                    .read()
                    .expect("embeddings")
                    .values()
                    .filter(|record| match modality {
                        Some(candidate) => candidate == record.modality,
                        None => true,
                    })
                    .map(|record| (record.id.clone(), cosine(&record.vector, &embedding)))
                    .collect();

                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
                scored.truncate(top_k);
                Ok(QueryResult {
                    ids: scored.into_iter().map(|(id, _)| id).collect(),
                    ..QueryResult::default()
                })
            }
            SemanticQuery::Files { path, blob } => Ok(QueryResult {
                files: self
                    .files
                    .read()
                    .expect("files")
                    .values()
                    .filter(|file| match path.as_ref() {
                        Some(candidate) => &file.path == candidate,
                        None => true,
                    })
                    .filter(|file| match blob.as_ref() {
                        Some(candidate) => &file.blob == candidate,
                        None => true,
                    })
                    .cloned()
                    .collect(),
                ..QueryResult::default()
            }),
            SemanticQuery::Facts { subject, predicate } => Ok(QueryResult {
                facts: self
                    .facts
                    .read()
                    .expect("facts")
                    .iter()
                    .filter(|fact| match subject.as_ref() {
                        Some(candidate) => &fact.subject == candidate,
                        None => true,
                    })
                    .filter(|fact| match predicate.as_ref() {
                        Some(candidate) => &fact.predicate == candidate,
                        None => true,
                    })
                    .cloned()
                    .collect(),
                ..QueryResult::default()
            }),
            SemanticQuery::Edges { from, kind } => Ok(QueryResult {
                edges: self
                    .edges
                    .read()
                    .expect("edges")
                    .iter()
                    .filter(|edge| match from.as_ref() {
                        Some(candidate) => &edge.from == candidate,
                        None => true,
                    })
                    .filter(|edge| match kind {
                        Some(candidate) => candidate == edge.kind,
                        None => true,
                    })
                    .cloned()
                    .collect(),
                ..QueryResult::default()
            }),
        }
    }

    async fn health(&self) -> Result<StoreHealth> {
        Ok(StoreHealth {
            healthy: true,
            message: "ready".to_string(),
        })
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

fn subject_to_string(subject: &Subject<'_>) -> String {
    match subject {
        Subject::NamedNode(node) => node.iri.to_string(),
        Subject::BlankNode(node) => format!("_:{}", node.id),
        Subject::Triple(_) => "<<embedded-subject>>".to_string(),
    }
}

fn term_to_string(term: &Term<'_>) -> String {
    match term {
        Term::NamedNode(node) => node.iri.to_string(),
        Term::BlankNode(node) => format!("_:{}", node.id),
        Term::Literal(literal) => literal_to_string(literal),
        Term::Triple(_) => "<<embedded-object>>".to_string(),
    }
}

fn literal_to_string(literal: &Literal<'_>) -> String {
    match literal {
        Literal::Simple { value } => value.to_string(),
        Literal::LanguageTaggedString { value, .. } => value.to_string(),
        Literal::Typed { value, datatype } => format!("{value}^^{}", datatype.iri),
    }
}
