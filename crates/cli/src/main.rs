use std::{env, path::Path, sync::Arc};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use indexer::{Extraction, GitLedger, Handler, IntakeFile, RuleMatcher, WatchConfig};
use mcp_server::{McpServerRuntime, Phase2RuntimeConfig, WatchRuntimeConfig};
use provider_test::FixtureProvider;
use retrieval::DeterministicBackend;
use storage_neumann::config::NeumannConfig;
use tokio::net::TcpListener;

struct Args {
    mode: Mode,
    watch_root: Option<String>,
}

enum Mode {
    Stdio,
    Http(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let runtime = Arc::new(build_runtime(args.watch_root).await?);

    match &args.mode {
        Mode::Stdio => runtime.clone().serve_stdio().await?,
        Mode::Http(addr) => {
            let listener = TcpListener::bind(addr).await?;
            eprintln!("phase2d listening on http://{}/mcp", listener.local_addr()?);
            axum::serve(listener, McpServerRuntime::router(runtime.clone()))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
    }

    if let Ok(runtime) = Arc::try_unwrap(runtime) {
        runtime.shutdown().await?;
    }

    Ok(())
}

async fn build_runtime(watch_root: Option<String>) -> Result<McpServerRuntime> {
    let mut config = Phase2RuntimeConfig::new(NeumannConfig::default()).with_transport_forwarding();
    if let Some(root) = watch_root {
        config = config.with_watch(WatchRuntimeConfig {
            config: WatchConfig { roots: vec![root] },
            git_ledger: Arc::new(ContentHashLedger),
            rules: Arc::new(MatchAllRules),
            handler: Arc::new(FileContentHandler),
            provider: Arc::new(FixtureProvider::new("fixture-embed")),
        });
    }

    McpServerRuntime::start_phase2_configured(Box::new(DeterministicBackend), config).await
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut mode = Mode::Stdio;
    let mut watch_root = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "stdio" => mode = Mode::Stdio,
            "http" => mode = Mode::Http("127.0.0.1:3000".to_string()),
            "--addr" => {
                let addr = iter
                    .next()
                    .ok_or_else(|| anyhow!("--addr requires a value"))?;
                mode = Mode::Http(addr);
            }
            "--watch" => {
                watch_root = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--watch requires a path"))?,
                );
            }
            other => return Err(anyhow!("unsupported argument: {other}")),
        }
    }

    Ok(Args { mode, watch_root })
}

struct ContentHashLedger;

#[async_trait]
impl GitLedger for ContentHashLedger {
    async fn blob_id(&self, path: &Path) -> Result<String> {
        let bytes = tokio::fs::read(path).await?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

struct MatchAllRules;

impl RuleMatcher for MatchAllRules {
    fn match_file(&self, _file: &IntakeFile) -> bool {
        true
    }
}

struct FileContentHandler;

#[async_trait]
impl Handler for FileContentHandler {
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction> {
        let text = tokio::fs::read_to_string(&file.path).await?;
        Ok(Extraction {
            text,
            has_symbols: matches!(file.extension.as_str(), ".rs" | ".py"),
        })
    }
}
