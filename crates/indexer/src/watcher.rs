use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use domain::SourceSystemKind;
use intake::{load_directory_sources, DirectorySource, DIRECTORY_CONFIG_FILE};
use tokio::{task::{block_in_place, JoinHandle}, time::sleep};
use watchexec::{error::CriticalError, Watchexec};
use watchexec_signals::Signal;

use crate::{
    index_file, index_intake_file, GitLedger, Handler, IntakeFile, ModelProvider, RuleMatcher,
};
use storage_neumann::KnowledgeStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchConfig {
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchSummary {
    pub seen_paths: usize,
    pub indexed_paths: usize,
    pub skipped_paths: usize,
}

#[async_trait]
pub trait ChangeProcessor: Send + Sync {
    async fn process_path(&self, path: &Path) -> Result<bool>;
}

pub struct IndexingChangeProcessor {
    roots: Vec<PathBuf>,
    git_ledger: Arc<dyn GitLedger>,
    rules: Arc<dyn RuleMatcher>,
    handler: Arc<dyn Handler>,
    store: Arc<dyn KnowledgeStore>,
    provider: Arc<dyn ModelProvider>,
}

impl IndexingChangeProcessor {
    pub fn new(
        roots: Vec<String>,
        git_ledger: Arc<dyn GitLedger>,
        rules: Arc<dyn RuleMatcher>,
        handler: Arc<dyn Handler>,
        store: Arc<dyn KnowledgeStore>,
        provider: Arc<dyn ModelProvider>,
    ) -> Self {
        Self {
            roots: roots.into_iter().map(PathBuf::from).collect(),
            git_ledger,
            rules,
            handler,
            store,
            provider,
        }
    }
}

#[async_trait]
impl ChangeProcessor for IndexingChangeProcessor {
    async fn process_path(&self, path: &Path) -> Result<bool> {
        if path.file_name().and_then(|value| value.to_str()) == Some(DIRECTORY_CONFIG_FILE) {
            return Ok(false);
        }

        if let Some(source) = self.resolve_source(path)? {
            let staged = source.stage_artifact(path)?;
            let mut tags = staged.artifact.tags.clone();
            tags.insert(
                "source_rel_path".to_string(),
                staged.relative_path.display().to_string(),
            );
            let intake = IntakeFile {
                path: staged.absolute_path.display().to_string(),
                extension: staged
                    .absolute_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| format!(".{value}"))
                    .unwrap_or_default(),
                media_type: staged.artifact.media_type.clone().unwrap_or_default(),
                fields: vec![],
                class: None,
                shape: None,
                source_id: Some(staged.source.source.id.clone()),
                source_kind: Some(source_kind_label(&staged.source.source.kind)),
                tags,
                ontology_refs: staged.source.ontology_refs.clone(),
            };
            return index_intake_file(
                path,
                intake,
                self.git_ledger.as_ref(),
                self.rules.as_ref(),
                self.handler.as_ref(),
                self.store.as_ref(),
                self.provider.as_ref(),
            )
            .await;
        }

        index_file(
            path,
            self.git_ledger.as_ref(),
            self.rules.as_ref(),
            self.handler.as_ref(),
            self.store.as_ref(),
            self.provider.as_ref(),
        )
        .await
    }
}

impl IndexingChangeProcessor {
    fn resolve_source(&self, path: &Path) -> Result<Option<DirectorySource>> {
        let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut matches = load_directory_sources(self.roots.iter())?
            .into_iter()
            .filter(|source| source.contains_path(&absolute))
            .collect::<Vec<_>>();
        matches.sort_by_key(|source| std::cmp::Reverse(source.root_dir.components().count()));
        Ok(matches.into_iter().next())
    }
}

fn source_kind_label(kind: &SourceSystemKind) -> String {
    match kind {
        SourceSystemKind::GitRepository => "git_repository".to_string(),
        SourceSystemKind::SharePoint => "sharepoint".to_string(),
        SourceSystemKind::DatabaseSchema => "database_schema".to_string(),
        SourceSystemKind::DocumentSilo => "document_silo".to_string(),
        SourceSystemKind::ProcessCatalog => "process_catalog".to_string(),
        SourceSystemKind::Other(value) => value.clone(),
    }
}

pub struct WatchexecRuntime {
    task: JoinHandle<Result<()>>,
}

impl WatchexecRuntime {
    pub async fn stop(self) -> Result<()> {
        self.task.abort();
        match self.task.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(anyhow!("watchexec task failed: {error}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollConfig {
    pub roots: Vec<String>,
    pub interval_seconds: u64,
}

pub struct PollingRuntime {
    task: JoinHandle<Result<()>>,
}

impl PollingRuntime {
    pub async fn stop(self) -> Result<()> {
        self.task.abort();
        match self.task.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(anyhow!("polling task failed: {error}")),
        }
    }
}

pub async fn reindex_changed_paths(
    events: impl IntoIterator<Item = WatchEvent>,
    processor: &dyn ChangeProcessor,
) -> Result<WatchSummary> {
    let mut unique = BTreeSet::new();
    for event in events {
        unique.insert(event.path);
    }

    let mut summary = WatchSummary::default();
    for path in unique {
        summary.seen_paths += 1;
        if processor.process_path(&path).await? {
            summary.indexed_paths += 1;
        } else {
            summary.skipped_paths += 1;
        }
    }

    Ok(summary)
}

pub async fn run_watchexec(config: WatchConfig, processor: Arc<dyn ChangeProcessor>) -> Result<()> {
    let watcher = build_watchexec(config, processor)?;
    await_watchexec(watcher.main()).await
}

pub async fn run_poll_loop(config: PollConfig, processor: Arc<dyn ChangeProcessor>) -> Result<()> {
    let interval = Duration::from_secs(config.interval_seconds.max(1));
    loop {
        let events = match scan_poll_events(&config.roots) {
            Ok(events) => events,
            Err(err) => {
                eprintln!("polling: failed to scan events: {err}");
                sleep(interval).await;
                continue;
            }
        };

        match reindex_changed_paths(events, processor.as_ref()).await {
            Ok(_summary) => {
                // Successful iteration; nothing else to do here.
            }
            Err(err) => {
                eprintln!("polling: failed to reindex changed paths: {err}");
                // Fall through to sleep before next iteration.
            }
        }

        sleep(interval).await;
    }
}

pub fn spawn_watchexec(
    config: WatchConfig,
    processor: Arc<dyn ChangeProcessor>,
) -> Result<WatchexecRuntime> {
    let watcher = build_watchexec(config, processor)?;
    let runtime = watcher.clone();
    let task = tokio::spawn(async move { await_watchexec(runtime.main()).await });
    Ok(WatchexecRuntime { task })
}

pub fn spawn_poller(
    config: PollConfig,
    processor: Arc<dyn ChangeProcessor>,
) -> Result<PollingRuntime> {
    let task = tokio::spawn(async move { run_poll_loop(config, processor).await });
    Ok(PollingRuntime { task })
}

fn build_watchexec(
    config: WatchConfig,
    processor: Arc<dyn ChangeProcessor>,
) -> Result<Arc<Watchexec>> {
    let watcher = Watchexec::new_async(move |mut action| {
        let processor = processor.clone();
        Box::new(async move {
            if action.signals().any(|signal| signal == Signal::Interrupt) {
                action.quit();
                return action;
            }

            let events = action
                .paths()
                .map(|(path, _)| WatchEvent {
                    path: path.to_path_buf(),
                })
                .collect::<Vec<_>>();
            if !events.is_empty() {
                tokio::spawn(async move {
                    let _ = reindex_changed_paths(events, processor.as_ref()).await;
                });
            }
            action
        })
    })
    .context("create watchexec runtime")?;
    watcher.config.pathset(config.roots);
    Ok(watcher)
}

async fn await_watchexec(main: JoinHandle<Result<(), CriticalError>>) -> Result<()> {
    match main.await {
        Ok(Ok(())) | Ok(Err(CriticalError::Exit)) => Ok(()),
        Ok(Err(error)) => Err(anyhow!("watchexec runtime failed: {error}")),
        Err(error) => Err(anyhow!("watchexec join failed: {error}")),
    }
}

fn scan_poll_events(roots: &[String]) -> Result<Vec<WatchEvent>> {
    block_in_place(|| {
        let mut events = Vec::new();
        for root in roots {
            let root = PathBuf::from(root);
            if !root.exists() {
                continue;
            }
            collect_poll_events(&root, &mut events)?;
        }
        Ok(events)
    })
}

fn is_ignored_dir_name(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
    )
}

fn collect_poll_events(root: &Path, events: &mut Vec<WatchEvent>) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip common VCS and build directories to avoid indexing internal artifacts.
            if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                if is_ignored_dir_name(name) {
                    continue;
                }
            }
            collect_poll_events(&path, events)?;
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some(DIRECTORY_CONFIG_FILE) {
            continue;
        }
        events.push(WatchEvent { path });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use anyhow::Result;
    use async_trait::async_trait;
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use crate::watcher::{
        reindex_changed_paths, scan_poll_events, spawn_watchexec, ChangeProcessor, PollConfig,
        WatchConfig, WatchEvent, DIRECTORY_CONFIG_FILE,
    };

    struct ProbeProcessor {
        seen: Arc<Mutex<Vec<String>>>,
        notify: Option<Arc<Notify>>,
    }

    #[async_trait]
    impl ChangeProcessor for ProbeProcessor {
        async fn process_path(&self, path: &std::path::Path) -> Result<bool> {
            let display = path.display().to_string();
            self.seen.lock().expect("seen").push(display.clone());
            if let Some(notify) = &self.notify {
                notify.notify_waiters();
            }
            Ok(!display.ends_with(".tmp"))
        }
    }

    #[tokio::test]
    async fn reindex_deduplicates_and_runs_serially() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let summary = reindex_changed_paths(
            vec![
                WatchEvent {
                    path: "src/lib.rs".into(),
                },
                WatchEvent {
                    path: "src/lib.rs".into(),
                },
                WatchEvent {
                    path: "tmp/cache.tmp".into(),
                },
            ],
            &ProbeProcessor {
                seen: seen.clone(),
                notify: None,
            },
        )
        .await
        .expect("watch summary");

        assert_eq!(summary.seen_paths, 2);
        assert_eq!(summary.indexed_paths, 1);
        assert_eq!(summary.skipped_paths, 1);
        assert_eq!(
            seen.lock().expect("seen").as_slice(),
            &["src/lib.rs".to_string(), "tmp/cache.tmp".to_string()]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn watchexec_runtime_observes_file_changes() {
        let root = tempdir().expect("tempdir");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());
        let runtime = spawn_watchexec(
            WatchConfig {
                roots: vec![root.path().display().to_string()],
            },
            Arc::new(ProbeProcessor {
                seen: seen.clone(),
                notify: Some(notify.clone()),
            }),
        )
        .expect("spawn watcher");

        tokio::time::sleep(Duration::from_millis(250)).await;
        let changed = root.path().join("receipt.tomllm");
        tokio::fs::write(&changed, "version = 1\n")
            .await
            .expect("write");

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                notify.notified().await;
                if seen
                    .lock()
                    .expect("seen")
                    .iter()
                    .any(|path| path.ends_with("receipt.tomllm"))
                {
                    break;
                }
            }
        })
        .await
        .expect("observe file change");

        runtime.stop().await.expect("stop watcher");
    }

    #[test]
    fn poll_scan_discovers_files_under_roots() {
        let root = tempdir().expect("tempdir");
        std::fs::write(root.path().join("alpha.md"), "alpha").expect("write file");
        std::fs::create_dir(root.path().join("nested")).expect("mkdir");
        std::fs::write(root.path().join("nested/beta.md"), "beta").expect("write nested file");
        std::fs::write(
            root.path().join(DIRECTORY_CONFIG_FILE),
            "[source]\nid='x'\nkind='documents'\n",
        )
        .expect("write config");

        let events = scan_poll_events(&[root.path().display().to_string()]).expect("scan events");
        let mut seen = events
            .into_iter()
            .map(|event| {
                event
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        seen.sort();

        assert_eq!(seen, vec!["alpha.md".to_string(), "beta.md".to_string()]);
        let _ = PollConfig {
            roots: vec![root.path().display().to_string()],
            interval_seconds: 5,
        };
    }
}
