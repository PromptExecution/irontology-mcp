use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use dsl::compile_rule;
use indexer::{
    index_file, DslRuleMatcherAdapter, EdgeRecord, EmbedRequest, EmbedResponse, EmbeddingRecord,
    Extraction, FactRecord, FileRecord, GitLedger, Handler, IntakeFile, KnowledgeStore,
    ModelProvider, RuleMatcher, SemanticQuery, StoreHealth,
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

struct PanicHandler;

#[async_trait]
impl Handler for PanicHandler {
    async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
        panic!("handler should not run when DSL rules do not match")
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

    async fn upsert_facts(&self, _facts: Vec<FactRecord>) -> Result<()> {
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

#[test]
fn compiled_dsl_rule_matches_rust_file_metadata() {
    let rule = compile_rule(include_str!("../../dsl/rules/rust.tomllm")).expect("parse");
    let matcher = DslRuleMatcherAdapter::new(vec![rule]);
    let file = IntakeFile::from_path(Path::new("src/lib.rs"));

    assert!(matcher.match_file(&file));
}

#[tokio::test]
async fn dsl_rule_miss_short_circuits_indexing() {
    let rule = compile_rule(include_str!("../../dsl/rules/rust.tomllm")).expect("parse");
    let matcher = DslRuleMatcherAdapter::new(vec![rule]);
    let calls = Arc::new(Mutex::new(0_usize));

    let changed = index_file(
        Path::new("docs/readme.md"),
        &FakeLedger { blob: "blob-md" },
        &matcher,
        &PanicHandler,
        &StoreProbe {
            seen: Arc::new(Mutex::new(vec![])),
            existing: false,
        },
        &ProviderProbe {
            calls: calls.clone(),
        },
    )
    .await
    .expect("index");

    assert!(!changed);
    assert_eq!(*calls.lock().expect("lock"), 0);
}

#[tokio::test]
async fn dsl_rule_match_indexes_rust_file() {
    let rule = compile_rule(include_str!("../../dsl/rules/rust.tomllm")).expect("parse");
    let matcher = DslRuleMatcherAdapter::new(vec![rule]);
    let seen = Arc::new(Mutex::new(Vec::<EmbeddingRecord>::new()));
    let calls = Arc::new(Mutex::new(0_usize));

    let changed = index_file(
        Path::new("src/lib.rs"),
        &FakeLedger { blob: "blob-rs" },
        &matcher,
        &StubHandler {
            extraction: Extraction {
                text: "fn alpha() {}".to_string(),
                has_symbols: true,
                fields: Default::default(),
                class: None,
                shape: None,
                claims: vec![],
                relations: vec![],
                notes: vec![],
            },
        },
        &StoreProbe {
            seen: seen.clone(),
            existing: false,
        },
        &ProviderProbe {
            calls: calls.clone(),
        },
    )
    .await
    .expect("index");

    assert!(changed);
    assert_eq!(*calls.lock().expect("lock"), 1);
    assert!(!seen.lock().expect("lock").is_empty());
}
