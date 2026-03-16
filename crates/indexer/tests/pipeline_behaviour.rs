use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
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

    async fn upsert_facts(&self, facts: Vec<FactRecord>) -> Result<()> {
        self.facts.lock().expect("lock").extend(facts);
        Ok(())
    }

    async fn upsert_edges(&self, _edges: Vec<EdgeRecord>) -> Result<()> {
        Ok(())
    }

    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        self.seen.lock().expect("lock").extend(embeddings);
        Ok(())
    }

    async fn ingest_turtle(&self, _source: &str, _turtle: &str) -> Result<()> {
        Ok(())
    }

    async fn related_objects(&self, _subject: &str, _predicate: &str) -> Result<Vec<String>> {
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
            extraction: Extraction {
                text: "fn alpha() {}".to_string(),
                has_symbols: true,
            },
        },
        &StoreProbe {
            seen: seen.clone(),
            facts: Arc::new(Mutex::new(vec![])),
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
async fn staged_source_metadata_is_persisted_as_facts() {
    let seen = Arc::new(Mutex::new(Vec::<EmbeddingRecord>::new()));
    let facts = Arc::new(Mutex::new(Vec::<FactRecord>::new()));

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
            extraction: Extraction {
                text: "standing data glossary".to_string(),
                has_symbols: false,
            },
        },
        &StoreProbe {
            seen: seen.clone(),
            facts: facts.clone(),
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
}
