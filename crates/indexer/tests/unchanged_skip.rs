use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use indexer::{
    index_file, AnchorRecord, ArtifactRecord, EdgeRecord, EmbedRequest, EmbedResponse,
    EmbeddingRecord, Extraction, FactRecord, FileRecord, GitLedger, Handler, IntakeFile,
    KnowledgeStore, ModelProvider, ObservationRecord, RuleMatcher, SemanticQuery, StoreHealth,
};
use provider_api::{ChatRequest, ChatResponse, ProviderHealth, TokenUsage};

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
    async fn upsert_file(&self, _file: FileRecord) -> Result<()> {
        Ok(())
    }
    async fn upsert_symbols(&self, _symbols: Vec<storage_neumann::SymbolRecord>) -> Result<()> {
        Ok(())
    }
    async fn upsert_facts(&self, _facts: Vec<FactRecord>) -> Result<()> {
        Ok(())
    }
    async fn upsert_edges(&self, _edges: Vec<EdgeRecord>) -> Result<()> {
        Ok(())
    }
    async fn upsert_embeddings(&self, _embeddings: Vec<EmbeddingRecord>) -> Result<()> {
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
    async fn upsert_artifact(&self, _artifact: ArtifactRecord) -> Result<()> {
        Ok(())
    }
    async fn upsert_anchors(&self, _anchors: Vec<AnchorRecord>) -> Result<()> {
        Ok(())
    }
    async fn upsert_observations(&self, _obs: Vec<ObservationRecord>) -> Result<()> {
        Ok(())
    }
    async fn get_anchors_for(&self, _artifact_id: &str) -> Result<Vec<AnchorRecord>> {
        Ok(vec![])
    }
}

struct ProbeProvider {
    called: Arc<Mutex<bool>>,
}

#[async_trait]
impl ModelProvider for ProbeProvider {
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse {
            model: "probe".to_string(),
            content: "unused".to_string(),
            usage: TokenUsage::default(),
        })
    }

    async fn embed(&self, _req: EmbedRequest) -> Result<EmbedResponse> {
        *self.called.lock().expect("lock") = true;
        Ok(EmbedResponse {
            model: "probe".to_string(),
            vectors: vec![],
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
