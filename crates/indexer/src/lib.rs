pub mod chunking;
pub mod distillation;
pub mod dsl_matcher;
pub mod embedding;
pub mod pipeline;
pub mod watcher;

pub use dsl_matcher::DslRuleMatcherAdapter;
pub use pipeline::{
    index_file, index_intake_file, Extraction, GitLedger, Handler, IntakeFile, RuleMatcher,
};
pub use provider_api::{EmbedRequest, EmbedResponse, ModelProvider};
pub use storage_neumann::{
    EdgeKind, EdgeRecord, EmbeddingModality, EmbeddingRecord, FactRecord, FileRecord,
    KnowledgeStore, SemanticQuery, StoreHealth,
};
pub use watcher::{
    reindex_changed_paths, run_poll_loop, run_watchexec, spawn_poller, spawn_watchexec,
    ChangeProcessor, IndexingChangeProcessor, PollConfig, PollingRuntime, WatchConfig, WatchEvent,
    WatchSummary, WatchexecRuntime,
};
