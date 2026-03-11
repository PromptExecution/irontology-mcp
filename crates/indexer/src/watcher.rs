use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use tokio::task::JoinHandle;
use watchexec::{error::CriticalError, Watchexec};
use watchexec_signals::Signal;

use crate::{index_file, GitLedger, Handler, ModelProvider, RuleMatcher};
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
    git_ledger: Arc<dyn GitLedger>,
    rules: Arc<dyn RuleMatcher>,
    handler: Arc<dyn Handler>,
    store: Arc<dyn KnowledgeStore>,
    provider: Arc<dyn ModelProvider>,
}

impl IndexingChangeProcessor {
    pub fn new(
        git_ledger: Arc<dyn GitLedger>,
        rules: Arc<dyn RuleMatcher>,
        handler: Arc<dyn Handler>,
        store: Arc<dyn KnowledgeStore>,
        provider: Arc<dyn ModelProvider>,
    ) -> Self {
        Self {
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

pub fn spawn_watchexec(
    config: WatchConfig,
    processor: Arc<dyn ChangeProcessor>,
) -> Result<WatchexecRuntime> {
    let watcher = build_watchexec(config, processor)?;
    let runtime = watcher.clone();
    let task = tokio::spawn(async move { await_watchexec(runtime.main()).await });
    Ok(WatchexecRuntime { task })
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
        reindex_changed_paths, spawn_watchexec, ChangeProcessor, WatchConfig, WatchEvent,
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
}
