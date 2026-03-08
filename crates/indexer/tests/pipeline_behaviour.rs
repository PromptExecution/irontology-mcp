use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use indexer::{
    embedding::Modality,
    index_file,
    pipeline::{EmbedResponse, EmbeddingRecord, Extraction},
    EmbedRequest, GitLedger, Handler, IntakeFile, KnowledgeStore, ModelProvider, RuleMatcher,
};

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
    existing: bool,
}

#[async_trait]
impl KnowledgeStore for StoreProbe {
    async fn has_blob(&self, _blob_id: &str) -> Result<bool> {
        Ok(self.existing)
    }

    async fn upsert_embeddings(&self, embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        self.seen.lock().expect("lock").extend(embeddings);
        Ok(())
    }
}

struct ProviderProbe {
    calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl ModelProvider for ProviderProbe {
    async fn embed(&self, req: EmbedRequest) -> Result<EmbedResponse> {
        *self.calls.lock().expect("lock") += 1;
        let vectors = req
            .inputs
            .iter()
            .map(|_| Arc::from([0.1_f32, 0.2_f32]))
            .collect();
        Ok(EmbedResponse { vectors })
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
    assert!(rows.iter().all(|r| r.modality == Modality::CodeSymbol));
}
