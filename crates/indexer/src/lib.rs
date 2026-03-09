pub mod chunking;
pub mod dsl_matcher;
pub mod embedding;
pub mod pipeline;
pub mod watcher;

pub use dsl_matcher::DslRuleMatcherAdapter;
pub use pipeline::{
    index_file, EmbedRequest, EmbeddingRecord, Extraction, GitLedger, Handler, IntakeFile,
    KnowledgeStore, ModelProvider, RuleMatcher,
};
