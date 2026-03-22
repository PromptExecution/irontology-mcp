use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    sync::{Arc, RwLock},
};

use anyhow::{anyhow, Result};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    pub id: String,
    pub blob: String,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: Option<String>,
    pub content: String,
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

/// Serde-compatible form of EmbeddingRecord (Vec<f32> instead of Arc<[f32]>)
#[derive(Serialize, Deserialize)]
struct StoredEmbedding {
    id: String,
    source_blob: String,
    vector: Vec<f32>,
    modality: EmbeddingModality,
    semantic_weight: f32,
}

impl From<&EmbeddingRecord> for StoredEmbedding {
    fn from(r: &EmbeddingRecord) -> Self {
        Self {
            id: r.id.clone(),
            source_blob: r.source_blob.clone(),
            vector: r.vector.to_vec(),
            modality: r.modality,
            semantic_weight: r.semantic_weight,
        }
    }
}

impl From<StoredEmbedding> for EmbeddingRecord {
    fn from(s: StoredEmbedding) -> Self {
        Self {
            id: s.id,
            source_blob: s.source_blob,
            vector: s.vector.into(),
            modality: s.modality,
            semantic_weight: s.semantic_weight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticTriple {
    pub source: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

/// A point-in-time snapshot of all in-memory NeumannStore state.
/// Used by retrieval backends for pure-function search without taking locks.
#[derive(Debug, Clone, Default)]
pub struct StoreSnapshot {
    pub files: Vec<FileRecord>,
    pub symbols: Vec<SymbolRecord>,
    pub embeddings: Vec<EmbeddingRecord>,
    pub facts: Vec<FactRecord>,
    pub edges: Vec<EdgeRecord>,
    pub semantic_triples: Vec<SemanticTriple>,
}

impl StoreSnapshot {
    pub fn ontology_classes(&self) -> Vec<String> {
        let mut classes = BTreeSet::new();

        for fact in &self.facts {
            if matches!(fact.predicate.as_str(), "class" | "shape" | "ontology_ref") {
                if let Some(value) = fact.object.as_str() {
                    classes.insert(value.to_string());
                }
            }
        }

        for triple in &self.semantic_triples {
            if triple.predicate.ends_with("#type")
                || triple.predicate.ends_with("/type")
                || triple.predicate.ends_with(":type")
            {
                if triple.object.ends_with("#Class")
                    || triple.object.ends_with("/Class")
                    || triple.object.ends_with(":Class")
                {
                    classes.insert(triple.subject.clone());
                } else {
                    classes.insert(triple.object.clone());
                }
            }
        }

        // Include symbol kinds as ontology classes
        for symbol in &self.symbols {
            classes.insert(symbol.kind.clone());
        }

        classes.into_iter().collect()
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
    Symbols {
        id: Option<String>,
        path: Option<String>,
        name: Option<String>,
        kind: Option<String>,
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
    pub symbols: Vec<SymbolRecord>,
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
    async fn upsert_symbols(&self, symbols: Vec<SymbolRecord>) -> Result<()>;
    async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()>;
    async fn upsert_edges(&self, edges: Vec<EdgeRecord>) -> Result<()>;
    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()>;
    async fn ingest_turtle(&self, source: &str, turtle: &str) -> Result<()>;
    async fn related_objects(&self, subject: &str, predicate: &str) -> Result<Vec<String>>;
    async fn list_classes(&self) -> Result<Vec<String>>;
    async fn query(&self, q: SemanticQuery) -> Result<QueryResult>;
    async fn health(&self) -> Result<StoreHealth>;
}

pub struct NeumannStore {
    _config: NeumannConfig,
    files: RwLock<HashMap<String, FileRecord>>,
    symbols: RwLock<HashMap<String, SymbolRecord>>,
    embeddings: RwLock<HashMap<String, EmbeddingRecord>>,
    blobs: RwLock<HashMap<String, bool>>,
    facts: RwLock<Vec<FactRecord>>,
    edges: RwLock<Vec<EdgeRecord>>,
    semantic_triples: RwLock<Vec<SemanticTriple>>,
    /// Some(db) if data_path was set — write-through persistence via sled
    db: Option<Arc<sled::Db>>,
}

impl NeumannStore {
    /// Try to open (or create) a NeumannStore at `config.data_path`.
    /// Returns `Err` if sled fails to open, allowing callers to fail fast
    /// rather than silently losing data.
    pub fn try_new(config: NeumannConfig) -> Result<Self> {
        let db = if let Some(dir) = &config.data_path {
            Some(Arc::new(
                sled::open(dir)
                    .map_err(|e| anyhow!("NeumannStore: failed to open sled at {}: {e}", dir.display()))?,
            ))
        } else {
            None
        };

        let mut store = Self {
            _config: config,
            files: RwLock::new(HashMap::new()),
            symbols: RwLock::new(HashMap::new()),
            embeddings: RwLock::new(HashMap::new()),
            blobs: RwLock::new(HashMap::new()),
            facts: RwLock::new(Vec::new()),
            edges: RwLock::new(Vec::new()),
            semantic_triples: RwLock::new(Vec::new()),
            db,
        };

        if store.db.is_some() {
            store.restore_from_sled()?;
        }

        Ok(store)
    }

    /// Open a NeumannStore, falling back to in-memory if sled fails to open.
    /// Prefer `try_new` when you need to detect and propagate persistence errors.
    pub fn new(config: NeumannConfig) -> Self {
        match Self::try_new(config.clone()) {
            Ok(store) => store,
            Err(e) => {
                eprintln!("⚠️ NeumannStore: {e}; falling back to in-memory mode (data will not be persisted)");
                let mut fallback = config;
                fallback.data_path = None;
                Self::try_new(fallback).expect("in-memory NeumannStore cannot fail")
            }
        }
    }

    /// Restore in-memory state from sled.
    /// Returns an error if any sled tree cannot be opened or if any
    /// record fails to deserialize, so callers can detect corruption or
    /// schema drift and remediate rather than silently operating on partial data.
    fn restore_from_sled(&mut self) -> Result<()> {
        let db = self.db.as_ref().expect("db must be Some when restore_from_sled is called");
        let mut failures = 0usize;

        // Restore semantic triples
        let tree = db.open_tree("triples")
            .map_err(|e| anyhow!("cannot open 'triples' tree: {e}"))?;
        {
            let mut triples = self.semantic_triples.write().expect("semantic_triples");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading triple: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<SemanticTriple>(&v) {
                        Ok(triple) => triples.push(triple),
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize triple: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        // Restore files
        let tree = db.open_tree("files")
            .map_err(|e| anyhow!("cannot open 'files' tree: {e}"))?;
        {
            let mut files = self.files.write().expect("files");
            let mut blobs = self.blobs.write().expect("blobs");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading file: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<FileRecord>(&v) {
                        Ok(file) => {
                            blobs.insert(file.blob.clone(), true);
                            files.insert(file.id.clone(), file);
                        }
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize file: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        // Restore symbols
        let tree = db.open_tree("symbols")
            .map_err(|e| anyhow!("cannot open 'symbols' tree: {e}"))?;
        {
            let mut symbols = self.symbols.write().expect("symbols");
            let mut blobs = self.blobs.write().expect("blobs");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading symbol: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<SymbolRecord>(&v) {
                        Ok(symbol) => {
                            blobs.insert(symbol.blob.clone(), true);
                            symbols.insert(symbol.id.clone(), symbol);
                        }
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize symbol: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        // Restore facts
        let tree = db.open_tree("facts")
            .map_err(|e| anyhow!("cannot open 'facts' tree: {e}"))?;
        {
            let mut facts = self.facts.write().expect("facts");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading fact: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<FactRecord>(&v) {
                        Ok(fact) => facts.push(fact),
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize fact: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        // Restore edges
        let tree = db.open_tree("edges")
            .map_err(|e| anyhow!("cannot open 'edges' tree: {e}"))?;
        {
            let mut edges = self.edges.write().expect("edges");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading edge: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<EdgeRecord>(&v) {
                        Ok(edge) => edges.push(edge),
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize edge: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        // Restore embeddings
        let tree = db.open_tree("embeddings")
            .map_err(|e| anyhow!("cannot open 'embeddings' tree: {e}"))?;
        {
            let mut embeddings = self.embeddings.write().expect("embeddings");
            let mut blobs = self.blobs.write().expect("blobs");
            for item in tree.iter() {
                match item {
                    Err(e) => {
                        eprintln!("⚠️ NeumannStore: sled error reading embedding: {e}");
                        failures += 1;
                    }
                    Ok((_, v)) => match serde_json::from_slice::<StoredEmbedding>(&v) {
                        Ok(stored) => {
                            blobs.insert(stored.source_blob.clone(), true);
                            let record: EmbeddingRecord = stored.into();
                            embeddings.insert(record.id.clone(), record);
                        }
                        Err(e) => {
                            eprintln!("⚠️ NeumannStore: failed to deserialize embedding: {e}");
                            failures += 1;
                        }
                    },
                }
            }
        }

        if failures > 0 {
            return Err(anyhow!(
                "{failures} record(s) failed to restore from sled; \
                 the database may be corrupted or the schema has changed. \
                 Inspect and repair the sled DB at the configured data_path."
            ));
        }

        Ok(())
    }

    /// Return a point-in-time snapshot of all in-memory state.
    pub fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            files: self
                .files
                .read()
                .expect("files")
                .values()
                .cloned()
                .collect(),
            symbols: self
                .symbols
                .read()
                .expect("symbols")
                .values()
                .cloned()
                .collect(),
            embeddings: self
                .embeddings
                .read()
                .expect("embeddings")
                .values()
                .cloned()
                .collect(),
            facts: self.facts.read().expect("facts").clone(),
            edges: self.edges.read().expect("edges").clone(),
            semantic_triples: self
                .semantic_triples
                .read()
                .expect("semantic_triples")
                .clone(),
        }
    }

    pub fn ontology_classes(&self) -> Vec<String> {
        self.snapshot().ontology_classes()
    }
}

#[async_trait]
impl KnowledgeStore for NeumannStore {
    async fn has_blob(&self, blob_id: &str) -> Result<bool> {
        Ok(self.blobs.read().expect("blobs").contains_key(blob_id))
    }

    async fn upsert_file(&self, file: FileRecord) -> Result<()> {
        if let Some(db) = &self.db {
            let tree = db.open_tree("files")?;
            tree.insert(file.id.as_bytes(), serde_json::to_vec(&file)?)?;
        }
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

    async fn upsert_symbols(&self, symbols: Vec<SymbolRecord>) -> Result<()> {
        let mut stored = self.symbols.write().expect("symbols");
        let mut blobs = self.blobs.write().expect("blobs");
        for symbol in symbols {
            if let Some(db) = &self.db {
                let tree = db.open_tree("symbols")?;
                tree.insert(symbol.id.as_bytes(), serde_json::to_vec(&symbol)?)?;
            }
            blobs.insert(symbol.blob.clone(), true);
            stored.insert(symbol.id.clone(), symbol);
        }
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
                if let Some(db) = &self.db {
                    let tree = db.open_tree("facts")?;
                    let key = format!("{}::{}::{}", fact.subject, fact.predicate, stored.len());
                    tree.insert(key.as_bytes(), serde_json::to_vec(&fact)?)?;
                }
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
                if let Some(db) = &self.db {
                    let tree = db.open_tree("edges")?;
                    let key = format!("{}::{}::{:?}", edge.from, edge.to, edge.kind);
                    tree.insert(key.as_bytes(), serde_json::to_vec(existing)?)?;
                }
            } else {
                if let Some(db) = &self.db {
                    let tree = db.open_tree("edges")?;
                    let key = format!("{}::{}::{:?}", edge.from, edge.to, edge.kind);
                    tree.insert(key.as_bytes(), serde_json::to_vec(&edge)?)?;
                }
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
            if let Some(db) = &self.db {
                let tree = db.open_tree("embeddings")?;
                let stored = StoredEmbedding::from(&emb);
                tree.insert(emb.id.as_bytes(), serde_json::to_vec(&stored)?)?;
            }
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

        if let Some(db) = &self.db {
            let tree = db.open_tree("triples")?;
            for triple in &parsed {
                let key = format!(
                    "{}::{}::{}::{}",
                    triple.source, triple.subject, triple.predicate, triple.object
                );
                tree.insert(key.as_bytes(), serde_json::to_vec(triple)?)?;
            }
        }

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

    async fn list_classes(&self) -> Result<Vec<String>> {
        Ok(self.snapshot().ontology_classes())
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
            SemanticQuery::Symbols {
                id,
                path,
                name,
                kind,
            } => Ok(QueryResult {
                symbols: {
                    let mut results: Vec<_> = self
                        .symbols
                        .read()
                        .expect("symbols")
                        .values()
                        .filter(|symbol| match id.as_ref() {
                            Some(candidate) => &symbol.id == candidate,
                            None => true,
                        })
                        .filter(|symbol| match path.as_ref() {
                            Some(candidate) => symbol.path == *candidate,
                            None => true,
                        })
                        .filter(|symbol| match name.as_ref() {
                            Some(candidate) => symbol.name == *candidate,
                            None => true,
                        })
                        .filter(|symbol| match kind.as_ref() {
                            Some(candidate) => symbol.kind == *candidate,
                            None => true,
                        })
                        .cloned()
                        .collect();
                    results.sort_by(|a, b| {
                        a.path.cmp(&b.path).then(a.start_line.cmp(&b.start_line))
                    });
                    results
                },
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
        let persistent = self.db.is_some();
        Ok(StoreHealth {
            healthy: true,
            message: if persistent {
                "ready (persistent)".to_string()
            } else {
                "ready (in-memory)".to_string()
            },
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
