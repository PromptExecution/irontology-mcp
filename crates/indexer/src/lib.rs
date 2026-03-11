pub mod chunking;
pub mod dsl_matcher;
pub mod embedding;
pub mod pipeline;
pub mod watcher;

pub use dsl_matcher::DslRuleMatcherAdapter;
pub use pipeline::{index_file, Extraction, GitLedger, Handler, IntakeFile, RuleMatcher};
pub use provider_api::{EmbedRequest, EmbedResponse, ModelProvider};
pub use storage_neumann::{
    EdgeKind, EdgeRecord, EmbeddingModality, EmbeddingRecord, FactRecord, FileRecord,
    KnowledgeStore, SemanticQuery, StoreHealth,
};
