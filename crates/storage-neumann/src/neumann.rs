use std::{
    cmp::Ordering,
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEmbeddingRecord {
    id: String,
    source_blob: String,
    vector: Vec<f32>,
    modality: EmbeddingModality,
    semantic_weight: f32,
}

impl From<EmbeddingRecord> for StoredEmbeddingRecord {
    fn from(value: EmbeddingRecord) -> Self {
        Self {
            id: value.id,
            source_blob: value.source_blob,
            vector: value.vector.as_ref().to_vec(),
            modality: value.modality,
            semantic_weight: value.semantic_weight,
        }
    }
}

impl From<StoredEmbeddingRecord> for EmbeddingRecord {
    fn from(value: StoredEmbeddingRecord) -> Self {
        Self {
            id: value.id,
            source_blob: value.source_blob,
            vector: Arc::from(value.vector.into_boxed_slice()),
            modality: value.modality,
            semantic_weight: value.semantic_weight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTriple {
    pub source: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredSemanticTriple {
    source: String,
    subject: String,
    predicate: String,
    object: String,
}

impl From<SemanticTriple> for StoredSemanticTriple {
    fn from(value: SemanticTriple) -> Self {
        Self {
            source: value.source,
            subject: value.subject,
            predicate: value.predicate,
            object: value.object,
        }
    }
}

impl From<StoredSemanticTriple> for SemanticTriple {
    fn from(value: StoredSemanticTriple) -> Self {
        Self {
            source: value.source,
            subject: value.subject,
            predicate: value.predicate,
            object: value.object,
        }
    }
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
    config: NeumannConfig,
    db: sled::Db,
    embeddings: sled::Tree,
    blobs: sled::Tree,
    semantic_triples: sled::Tree,
    files: RwLock<HashMap<String, FileRecord>>,
    facts: RwLock<Vec<FactRecord>>,
    edges: RwLock<Vec<EdgeRecord>>,
}

impl NeumannStore {
    pub fn try_new(config: NeumannConfig) -> anyhow::Result<Self> {
        let db = sled::open(resolve_data_path(&config))?;
        let embeddings = db.open_tree("embeddings")?;
        let blobs = db.open_tree("blobs")?;
        let semantic_triples = db.open_tree("semantic_triples")?;
        Ok(Self {
            config,
            db,
            embeddings,
            blobs,
            semantic_triples,
            files: RwLock::new(HashMap::new()),
            facts: RwLock::new(Vec::new()),
            edges: RwLock::new(Vec::new()),
        })
    }
}

#[async_trait]
impl KnowledgeStore for NeumannStore {
    async fn has_blob(&self, blob_id: &str) -> Result<bool> {
        Ok(self.blobs.contains_key(blob_id.as_bytes())?)
    }

    async fn upsert_file(&self, file: FileRecord) -> Result<()> {
        self.blobs.insert(file.blob.as_bytes(), &[1])?;
        self.files
            .write()
            .expect("files")
            .insert(file.id.clone(), file);
        self.db.flush_async().await?;
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
            if let Some(existing) = stored.iter_mut().find(|candidate| {
                candidate.from == edge.from
                    && candidate.to == edge.to
                    && candidate.kind == edge.kind
            }) {
                existing.weight = edge.weight;
            } else {
                stored.push(edge);
            }
        }
        Ok(())
    }

    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        for emb in embeddings {
            self.blobs.insert(emb.source_blob.as_bytes(), &[1])?;
            let stored = StoredEmbeddingRecord::from(emb);
            self.embeddings.insert(
                stored.id.as_bytes(),
                serde_json::to_vec(&stored)?,
            )?;
        }
        self.db.flush()?;
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

        for triple in parsed {
            let stored = StoredSemanticTriple::from(triple);
            let key = triple_key(&stored.subject, &stored.predicate, &stored.object);

            if let Some(existing_bytes) = self.semantic_triples.get(&key)? {
                // Merge provenance: keep a combined, de-duplicated list of sources
                let mut existing: StoredSemanticTriple =
                    serde_json::from_slice(&existing_bytes)?;

                let new_source = &stored.source;
                let already_present = existing
                    .source
                    .split(';')
                    .any(|s| s == new_source);

                if !already_present {
                    if existing.source.is_empty() {
                        existing.source = new_source.clone();
                    } else {
                        existing.source.push(';');
                        existing.source.push_str(new_source);
                    }
                    self.semantic_triples
                        .insert(&key, serde_json::to_vec(&existing)?)?;
                }
            } else {
                self.semantic_triples
                    .insert(&key, serde_json::to_vec(&stored)?)?;
            }
        }
        self.db.flush()?;
        Ok(())
    }

    async fn related_objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for item in self
            .semantic_triples
            .scan_prefix(triple_prefix(subject, predicate))
        {
            let (_, value) = item?;
            let triple: StoredSemanticTriple = serde_json::from_slice(&value)?;
            out.push(triple.object);
        }
        Ok(out)
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
                    .iter()
                    .values()
                    .filter_map(|value| value.ok())
                    .filter_map(|value| serde_json::from_slice::<StoredEmbeddingRecord>(&value).ok())
                    .map(EmbeddingRecord::from)
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
            message: format!(
                "ready: {}",
                resolve_data_path(&self.config).display()
            ),
        })
    }
}

fn resolve_data_path(config: &NeumannConfig) -> PathBuf {
    if let Some(explicit) = config.data_path.clone() {
        return explicit;
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, prefer roaming AppData, then LocalAppData, then USERPROFILE.
        // Fall back to a stable system-wide location if none are set, instead of CWD.
        let base = std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("LOCALAPPDATA"))
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));

        return base.join("b00t").join("neumann").join(&config.namespace);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On Unix-like systems, follow XDG base directory specification when possible.
        // Use $XDG_DATA_HOME, then ~/.local/share, and finally a stable system directory.
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local").join("share"))
            })
            .unwrap_or_else(|| PathBuf::from("/var/lib"));

        return base.join("b00t").join("neumann").join(&config.namespace);
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

fn triple_prefix(subject: &str, predicate: &str) -> Vec<u8> {
    format!(
        "triple::{}::{}::",
        encode_key_part(subject),
        encode_key_part(predicate)
    )
    .into_bytes()
}

fn triple_key(subject: &str, predicate: &str, object: &str) -> Vec<u8> {
    format!(
        "triple::{}::{}::{}",
        encode_key_part(subject),
        encode_key_part(predicate),
        encode_key_part(object)
    )
    .into_bytes()
}

fn encode_key_part(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value)
}
