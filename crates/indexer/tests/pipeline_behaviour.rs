use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use domain::{Claim, Relation};
use indexer::{
    index_file, index_intake_file, EdgeRecord, EmbedRequest, EmbedResponse, EmbeddingModality,
    EmbeddingRecord, Extraction, FactRecord, FileRecord, GitLedger, Handler, IntakeFile,
    KnowledgeStore, ModelProvider, RuleMatcher, SemanticQuery, StoreHealth,
};
use provider_api::{ChatRequest, ChatResponse, ProviderHealth, TokenUsage};

struct FakeLedger {
    blob: &'static str,
}

#[async_trait]
impl GitLedger for FakeLedger {
    async fn blob_id(&self, _path: &Path) -> Result<String> {
        Ok(self.blob.to_string())
    }
}

struct MatchNone;
impl RuleMatcher for MatchNone {
    fn match_file(&self, _file: &IntakeFile) -> bool {
        false
    }
}

struct MatchAll;
impl RuleMatcher for MatchAll {
    fn match_file(&self, _file: &IntakeFile) -> bool {
        true
    }
}

struct PanicHandler;
#[async_trait]
impl Handler for PanicHandler {
    async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
        panic!("handler should not be called")
    }
}

struct StubHandler {
    extraction: Extraction,
}

#[async_trait]
impl Handler for StubHandler {
    async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
        Ok(self.extraction.clone())
    }
}

struct StoreProbe {
    seen: Arc<Mutex<Vec<EmbeddingRecord>>>,
    facts: Arc<Mutex<Vec<FactRecord>>>,
    edges: Arc<Mutex<Vec<EdgeRecord>>>,
    existing: bool,
}

#[async_trait]
impl KnowledgeStore for StoreProbe {
    async fn has_blob(&self, _blob_id: &str) -> Result<bool> {
        Ok(self.existing)
    }

    async fn upsert_file(&self, _file: FileRecord) -> Result<()> {
        Ok(())
    }

    async fn upsert_symbols(&self, _symbols: Vec<storage_neumann::SymbolRecord>) -> Result<()> {
        Ok(())
    }

    async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()> {
        self.facts.lock().expect("lock").extend(facts);
        Ok(())
    }

    async fn upsert_edges(&self, edges: Vec<EdgeRecord>) -> Result<()> {
        self.edges.lock().expect("lock").extend(edges);
        Ok(())
    }

    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        self.seen.lock().expect("lock").extend(embeddings);
        Ok(())
    }

    async fn ingest_turtle(&self, _source: &str, _turtle: &str) -> Result<()> {
        Ok(())
    }
    async fn validate_turtle(&self, _turtle: &str) -> Result<Vec<storage_neumann::ShapeViolation>> {
        Ok(vec![])
    }
    async fn subclasses_of(&self, _class_iri: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn related_objects(&self, _subject: &str, _predicate: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn list_classes(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn query(&self, _q: SemanticQuery) -> Result<storage_neumann::QueryResult> {
        Ok(storage_neumann::QueryResult::default())
    }

    async fn health(&self) -> Result<StoreHealth> {
        Ok(StoreHealth {
            healthy: true,
            message: "ok".to_string(),
        })
    }
}

struct ProviderProbe {
    calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl ModelProvider for ProviderProbe {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse {
            model: "probe".to_string(),
            content: "unused".to_string(),
            usage: TokenUsage::default(),
        })
    }

    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        *self.calls.lock().expect("lock") += 1;
        let vectors = req
            .inputs
            .iter()
            .map(|_| Arc::from([0.1_f32, 0.2_f32]))
            .collect();
        Ok(EmbedResponse {
            model: "probe".to_string(),
            vectors,
            usage: TokenUsage::default(),
        })
    }

    async fn health(&self) -> Result<ProviderHealth> {
        Ok(ProviderHealth {
            healthy: true,
            message: "ok".to_string(),
        })
    }

    fn model_id(&self) -> &str {
        "probe"
    }
}

fn sample_extraction(text: &str, has_symbols: bool) -> Extraction {
    Extraction {
        text: text.to_string(),
        has_symbols,
        fields: BTreeMap::new(),
        class: None,
        shape: None,
        claims: vec![],
        relations: vec![],
        notes: vec![],
    }
}

#[tokio::test]
async fn rule_miss_skips_handler_and_provider() {
    let calls = Arc::new(Mutex::new(0_usize));
    let result = index_file(
        Path::new("src/lib.rs"),
        &FakeLedger { blob: "blob-noop" },
        &MatchNone,
        &PanicHandler,
        &StoreProbe {
            seen: Arc::new(Mutex::new(vec![])),
            facts: Arc::new(Mutex::new(vec![])),
            edges: Arc::new(Mutex::new(vec![])),
            existing: false,
        },
        &ProviderProbe {
            calls: calls.clone(),
        },
    )
    .await
    .expect("index");

    assert!(!result);
    assert_eq!(*calls.lock().expect("lock"), 0);
}

#[tokio::test]
async fn changed_file_upserts_code_symbol_embeddings() {
    let seen = Arc::new(Mutex::new(Vec::<EmbeddingRecord>::new()));
    let calls = Arc::new(Mutex::new(0_usize));

    let result = index_file(
        Path::new("src/lib.rs"),
        &FakeLedger { blob: "blob-code" },
        &MatchAll,
        &StubHandler {
            extraction: sample_extraction("fn alpha() {}", true),
        },
        &StoreProbe {
            seen: seen.clone(),
            facts: Arc::new(Mutex::new(vec![])),
            edges: Arc::new(Mutex::new(vec![])),
            existing: false,
        },
        &ProviderProbe {
            calls: calls.clone(),
        },
    )
    .await
    .expect("index");

    assert!(result);
    assert_eq!(*calls.lock().expect("lock"), 1);
    let rows = seen.lock().expect("lock");
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.source_blob == "blob-code"));
    assert!(rows
        .iter()
        .all(|r| r.modality == EmbeddingModality::CodeSymbol));
}

#[tokio::test]
async fn changed_file_persists_symbol_graph_and_symbol_embeddings() {
    let seen = Arc::new(Mutex::new(Vec::<EmbeddingRecord>::new()));
    let facts = Arc::new(Mutex::new(Vec::<FactRecord>::new()));
    let edges = Arc::new(Mutex::new(Vec::<EdgeRecord>::new()));
    let calls = Arc::new(Mutex::new(0_usize));

    let result = index_file(
        Path::new("src/lib.rs"),
        &FakeLedger {
            blob: "blob-symbols",
        },
        &MatchAll,
        &StubHandler {
            extraction: sample_extraction(
                r#"
use std::fmt::Debug;

fn alpha() {
    beta();
}

fn beta() {}
"#,
                true,
            ),
        },
        &StoreProbe {
            seen: seen.clone(),
            facts: facts.clone(),
            edges: edges.clone(),
            existing: false,
        },
        &ProviderProbe {
            calls: calls.clone(),
        },
    )
    .await
    .expect("index");

    assert!(result);
    assert_eq!(*calls.lock().expect("lock"), 1);

    let embeddings = seen.lock().expect("lock");
    assert_eq!(embeddings.len(), 2);
    assert!(embeddings
        .iter()
        .all(|record| record.modality == EmbeddingModality::CodeSymbol));
    assert!(embeddings
        .iter()
        .any(|record| record.id == "git:blob:blob-symbols:alpha"));
    assert!(embeddings
        .iter()
        .any(|record| record.id == "git:blob:blob-symbols:beta"));

    let facts = facts.lock().expect("facts");
    assert!(facts.iter().any(|fact| {
        fact.subject == "git:blob:blob-symbols:alpha"
            && fact.predicate == "symbol_kind"
            && fact.object == serde_json::json!("Function")
    }));
    assert!(facts.iter().any(|fact| {
        fact.subject == "git:blob:blob-symbols:beta" && fact.predicate == "symbol_span_start_line"
    }));

    let edges = edges.lock().expect("edges");
    assert!(edges.iter().any(|edge| {
        edge.from == "file:git:blob:blob-symbols"
            && edge.to == "git:blob:blob-symbols:alpha"
            && edge.kind == indexer::EdgeKind::Defines
    }));
    assert!(edges.iter().any(|edge| {
        edge.from == "git:blob:blob-symbols:alpha"
            && edge.to == "git:blob:blob-symbols:beta"
            && edge.kind == indexer::EdgeKind::Calls
    }));
    assert!(edges.iter().any(|edge| {
        edge.from == "file:git:blob:blob-symbols"
            && edge.to == "git:blob:blob-symbols:std::fmt::Debug"
            && edge.kind == indexer::EdgeKind::DependsOn
    }));
}

#[tokio::test]
async fn staged_source_metadata_is_persisted_as_facts() {
    let seen = Arc::new(Mutex::new(Vec::<EmbeddingRecord>::new()));
    let facts = Arc::new(Mutex::new(Vec::<FactRecord>::new()));
    let edges = Arc::new(Mutex::new(Vec::<EdgeRecord>::new()));

    let changed = index_intake_file(
        Path::new("docs/architecture/standing-data.md"),
        IntakeFile {
            path: "docs/architecture/standing-data.md".to_string(),
            extension: ".md".to_string(),
            media_type: "text/plain".to_string(),
            fields: vec![],
            class: None,
            shape: None,
            source_id: Some("sharepoint://enterprise-arch".to_string()),
            source_kind: Some("sharepoint".to_string()),
            tags: BTreeMap::from([
                ("portfolio".to_string(), "architecture".to_string()),
                (
                    "source_rel_path".to_string(),
                    "standing-data.md".to_string(),
                ),
            ]),
            ontology_refs: vec!["ontology://enterprise-arch".to_string()],
        },
        &FakeLedger {
            blob: "blob-standing-data",
        },
        &MatchAll,
        &StubHandler {
            extraction: sample_extraction("standing data glossary", false),
        },
        &StoreProbe {
            seen: seen.clone(),
            facts: facts.clone(),
            edges: edges.clone(),
            existing: false,
        },
        &ProviderProbe {
            calls: Arc::new(Mutex::new(0_usize)),
        },
    )
    .await
    .expect("index");

    assert!(changed);
    let stored_facts = facts.lock().expect("facts");
    assert!(stored_facts.iter().any(|fact| {
        fact.predicate == "source_id"
            && fact.object == serde_json::json!("sharepoint://enterprise-arch")
    }));
    assert!(stored_facts.iter().any(|fact| {
        fact.predicate == "source_kind" && fact.object == serde_json::json!("sharepoint")
    }));
    assert!(stored_facts.iter().any(|fact| {
        fact.predicate == "tag:portfolio" && fact.object == serde_json::json!("architecture")
    }));
    assert!(stored_facts.iter().any(|fact| {
        fact.predicate == "ontology_ref"
            && fact.object == serde_json::json!("ontology://enterprise-arch")
    }));
    assert!(edges.lock().expect("edges").is_empty());
}

#[tokio::test]
async fn semantic_enrichment_is_persisted_as_facts_and_edges() {
    let facts = Arc::new(Mutex::new(Vec::<FactRecord>::new()));
    let edges = Arc::new(Mutex::new(Vec::<EdgeRecord>::new()));

    let changed = index_intake_file(
        Path::new("docs/meeting-notes.md"),
        IntakeFile {
            path: "docs/meeting-notes.md".to_string(),
            extension: ".md".to_string(),
            media_type: "text/plain".to_string(),
            fields: vec![],
            class: None,
            shape: None,
            source_id: Some("sharepoint://ops".to_string()),
            source_kind: Some("sharepoint".to_string()),
            tags: BTreeMap::new(),
            ontology_refs: vec![],
        },
        &FakeLedger {
            blob: "blob-semantic",
        },
        &MatchAll,
        &StubHandler {
            extraction: Extraction {
                text: "standing data has hidden dependencies".to_string(),
                has_symbols: false,
                fields: BTreeMap::from([("vendor".to_string(), serde_json::json!("Acme Finance"))]),
                class: Some("doc:MeetingNotes".to_string()),
                shape: Some("shape:MeetingNotes".to_string()),
                claims: vec![Claim {
                    id: "claim:1".to_string(),
                    subject: "concept:acme:latent-knowledge".to_string(),
                    predicate: "semantic:may_depend_on".to_string(),
                    object: "view:acme:cross-document-correlation".to_string(),
                    evidence: vec!["obs:file".to_string()],
                    confidence: 0.61,
                    namespace: Some("ctx:acme".to_string()),
                }],
                relations: vec![Relation {
                    id: "relation:1".to_string(),
                    subject_id: "concept:standing-data:ops".to_string(),
                    predicate: "semantic:overlaps_with".to_string(),
                    object_id: "concept:standing-data:arch".to_string(),
                    evidence: vec!["obs:file".to_string()],
                    confidence: 0.72,
                    namespace: Some("ctx:acme".to_string()),
                }],
                notes: vec!["latent dependency hint observed".to_string()],
            },
        },
        &StoreProbe {
            seen: Arc::new(Mutex::new(vec![])),
            facts: facts.clone(),
            edges: edges.clone(),
            existing: false,
        },
        &ProviderProbe {
            calls: Arc::new(Mutex::new(0_usize)),
        },
    )
    .await
    .expect("index");

    assert!(changed);
    let facts = facts.lock().expect("facts");
    assert!(facts.iter().any(|fact| {
        fact.predicate == "class" && fact.object == serde_json::json!("doc:MeetingNotes")
    }));
    assert!(facts.iter().any(|fact| {
        fact.predicate == "shape" && fact.object == serde_json::json!("shape:MeetingNotes")
    }));
    assert!(facts.iter().any(|fact| {
        fact.predicate == "field:vendor" && fact.object == serde_json::json!("Acme Finance")
    }));
    assert!(facts.iter().any(|fact| {
        fact.predicate == "semantic_note"
            && fact.object == serde_json::json!("latent dependency hint observed")
    }));
    assert!(facts.iter().any(|fact| {
        fact.subject == "concept:acme:latent-knowledge"
            && fact.predicate == "semantic:may_depend_on"
            && fact.object == serde_json::json!("view:acme:cross-document-correlation")
    }));
    let edges = edges.lock().expect("edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].kind, indexer::EdgeKind::Related);
}
