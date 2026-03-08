use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use indexer::{
    index_file,
    pipeline::{EmbedResponse, EmbeddingRecord, Extraction},
    EmbedRequest, GitLedger, Handler, IntakeFile, KnowledgeStore, ModelProvider, RuleMatcher,
};

struct FakeLedger;
#[async_trait]
impl GitLedger for FakeLedger {
    async fn blob_id(&self, _path: &Path) -> Result<String> {
        Ok("blob-1".into())
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
        panic!("extract should not be called for unchanged file")
    }
}

struct FakeStore;
#[async_trait]
impl KnowledgeStore for FakeStore {
    async fn has_blob(&self, _blob_id: &str) -> Result<bool> {
        Ok(true)
    }
    async fn upsert_embeddings(&self, _embeddings: Vec<EmbeddingRecord>) -> Result<()> {
        Ok(())
    }
}

struct ProbeProvider {
    called: Arc<Mutex<bool>>,
}

#[async_trait]
impl ModelProvider for ProbeProvider {
    async fn embed(&self, _req: EmbedRequest) -> Result<EmbedResponse> {
        *self.called.lock().expect("lock") = true;
        Ok(EmbedResponse { vectors: vec![] })
    }
}

#[tokio::test]
async fn unchanged_file_is_skipped() {
    let called = Arc::new(Mutex::new(false));
    let provider = ProbeProvider {
        called: called.clone(),
    };
    let changed = index_file(
        Path::new("src/lib.rs"),
        &FakeLedger,
        &MatchAll,
        &PanicHandler,
        &FakeStore,
        &provider,
    )
    .await
    .expect("index");

    assert!(!changed);
    assert!(!*called.lock().expect("lock"));
}
