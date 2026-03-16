mod config;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use config::{
    load_phase2d_config, resolve_config_relative, ExtractorKind, LoadedPhase2dConfig,
    ProviderSettings,
};
use domain::{Artifact, EvidenceBundle, Observation};
use indexer::{
    Extraction, GitLedger, Handler, IntakeFile, ModelProvider, PollConfig, RuleMatcher, WatchConfig,
};
use intake::{load_directory_sources, DirectorySource, PollMode};
use mcp_server::{McpServerRuntime, Phase2RuntimeConfig, PollRuntimeConfig, WatchRuntimeConfig};
use provider_local::{LocalProvider, LocalProviderConfig, MistralRsConfig};
use provider_openai::{OpenAiCompatConfig, OpenAiCompatProvider};
use provider_test::FixtureProvider;
use retrieval::DeterministicBackend;
use semantic_runtime::{CorrelationRule, SemanticRuntime};
use storage_neumann::config::NeumannConfig;
use tokio::net::TcpListener;

struct Args {
    mode: Mode,
    http_addr: Option<String>,
    config_path: Option<PathBuf>,
    watch_roots: Vec<String>,
}

enum Mode {
    Stdio,
    Http,
}

#[derive(Clone)]
struct RuntimeRegistries {
    extractors: BTreeMap<String, ExtractorKind>,
    executors: BTreeMap<String, String>,
    rhai_modules: BTreeMap<String, CorrelationRule>,
    source_modules: BTreeMap<PathBuf, Vec<String>>,
}

struct RuntimeBootstrap {
    http_addr: String,
    phase2: Phase2RuntimeConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let loaded = load_phase2d_config(args.config_path.as_deref())?;
    let bootstrap = build_runtime_bootstrap(&loaded, &args.watch_roots)?;
    let runtime = Arc::new(
        McpServerRuntime::start_phase2_configured(Box::new(DeterministicBackend), bootstrap.phase2)
            .await?,
    );

    match &args.mode {
        Mode::Stdio => runtime.clone().serve_stdio().await?,
        Mode::Http => {
            let addr = args
                .http_addr
                .clone()
                .unwrap_or_else(|| bootstrap.http_addr.clone());
            let listener = TcpListener::bind(&addr).await?;
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

fn build_runtime_bootstrap(
    loaded: &LoadedPhase2dConfig,
    cli_watch_roots: &[String],
) -> Result<RuntimeBootstrap> {
    let provider = build_provider(&loaded.config.provider)?;
    let registries = build_registries(loaded)?;
    let sources = load_sources(loaded, cli_watch_roots)?;
    validate_sources(&sources, &registries)?;

    let ledger: Arc<dyn GitLedger> = Arc::new(ContentHashLedger);
    let rules: Arc<dyn RuleMatcher> = Arc::new(MatchAllRules);
    let handler: Arc<dyn Handler> = Arc::new(ConfiguredFileContentHandler::new(
        sources.clone(),
        registries.clone(),
    ));

    let mut phase2 = Phase2RuntimeConfig::new(NeumannConfig::from(loaded.config.neumann.clone()));
    if loaded.config.forwarding.transport_forwarding {
        phase2 = phase2.with_transport_forwarding();
    }

    let watch_roots = sources
        .iter()
        .filter(|source| matches!(source.poll.mode, PollMode::Watch))
        .map(|source| source.root_dir.display().to_string())
        .collect::<Vec<_>>();
    if !watch_roots.is_empty() {
        phase2 = phase2.with_watch(WatchRuntimeConfig {
            config: WatchConfig { roots: watch_roots },
            git_ledger: ledger.clone(),
            rules: rules.clone(),
            handler: handler.clone(),
            provider: provider.clone(),
        });
    }

    for source in sources
        .iter()
        .filter(|source| matches!(source.poll.mode, PollMode::Interval))
    {
        phase2 = phase2.with_poll(PollRuntimeConfig {
            config: PollConfig {
                roots: vec![source.root_dir.display().to_string()],
                interval_seconds: source.poll.interval_seconds.unwrap_or(60),
            },
            git_ledger: ledger.clone(),
            rules: rules.clone(),
            handler: handler.clone(),
            provider: provider.clone(),
        });
    }

    Ok(RuntimeBootstrap {
        http_addr: loaded.config.server.http_addr.clone(),
        phase2,
    })
}

fn build_provider(settings: &ProviderSettings) -> Result<Arc<dyn ModelProvider>> {
    Ok(match settings {
        ProviderSettings::Fixture {
            model_id,
            chat_content,
            embedding_dim,
        } => Arc::new(
            FixtureProvider::new(model_id.clone())
                .with_chat_content(chat_content.clone())
                .with_embedding_dim(*embedding_dim),
        ),
        ProviderSettings::OpenAi {
            base_url,
            model_id,
            api_key,
            retries,
            retry_backoff_ms,
        } => {
            let mut config = OpenAiCompatConfig::new(base_url.clone(), model_id.clone());
            config.api_key = api_key.clone();
            config.retries = *retries;
            config.retry_backoff_ms = *retry_backoff_ms;
            Arc::new(OpenAiCompatProvider::new(config))
        }
        ProviderSettings::Local {
            model_id,
            base_url,
            api_key,
            managed_program,
            managed_host,
            managed_port,
            managed_model,
            extra_args,
        } => {
            let provider = if let Some(model) = managed_model {
                let port = managed_port
                    .ok_or_else(|| anyhow!("provider.local.managed_port is required"))?;
                let mut config = MistralRsConfig::new(port, model_id.clone(), model.clone());
                config.program = managed_program
                    .clone()
                    .unwrap_or_else(|| "mistralrs-server".to_string());
                config.host = managed_host.clone();
                config.extra_args = extra_args.clone();
                config.api_key = api_key.clone();
                LocalProvider::new(LocalProviderConfig::managed_mistral_rs(config))
            } else {
                let base_url = base_url.clone().ok_or_else(|| {
                    anyhow!("provider.local.base_url is required when not using managed_model")
                })?;
                let mut config = LocalProviderConfig::new(base_url, model_id.clone());
                if let Some(api_key) = api_key {
                    config = config.with_api_key(api_key.clone());
                }
                LocalProvider::new(config)
            };
            Arc::new(provider)
        }
    })
}

fn build_registries(loaded: &LoadedPhase2dConfig) -> Result<RuntimeRegistries> {
    let extractors = loaded
        .config
        .registries
        .extractors
        .iter()
        .map(|registration| (registration.name.clone(), registration.kind.clone()))
        .collect::<BTreeMap<_, _>>();
    let executors = loaded
        .config
        .registries
        .executors
        .iter()
        .map(|registration| (registration.name.clone(), registration.target.clone()))
        .collect::<BTreeMap<_, _>>();

    let semantic_runtime = SemanticRuntime::new();
    let rhai_modules = loaded
        .config
        .registries
        .rhai_modules
        .iter()
        .map(|registration| {
            let path = resolve_config_relative(&registration.path, loaded.path.as_deref());
            Ok((
                registration.name.clone(),
                CorrelationRule {
                    name: registration.name.clone(),
                    script: fs::read_to_string(&path)?,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    semantic_runtime.validate(&rhai_modules.values().cloned().collect::<Vec<_>>())?;

    let source_modules = loaded
        .config
        .source_modules()
        .into_iter()
        .map(|(root, modules)| {
            let resolved = resolve_config_relative(&root, loaded.path.as_deref())
                .canonicalize()
                .unwrap_or_else(|_| resolve_config_relative(&root, loaded.path.as_deref()));
            (resolved, modules)
        })
        .collect::<BTreeMap<_, _>>();

    Ok(RuntimeRegistries {
        extractors,
        executors,
        rhai_modules,
        source_modules,
    })
}

fn load_sources(
    loaded: &LoadedPhase2dConfig,
    cli_watch_roots: &[String],
) -> Result<Vec<DirectorySource>> {
    let mut roots = BTreeSet::new();
    for root in loaded.config.source_roots() {
        let resolved = resolve_config_relative(&root, loaded.path.as_deref());
        roots.insert(resolved);
    }
    for root in cli_watch_roots {
        roots.insert(PathBuf::from(root));
    }

    load_directory_sources(roots.iter())
}

fn validate_sources(sources: &[DirectorySource], registries: &RuntimeRegistries) -> Result<()> {
    for source in sources {
        for extractor in &source.adapters.extractors {
            if !registries.extractors.contains_key(extractor) {
                return Err(anyhow!(
                    "source {} references unknown extractor {}",
                    source.root_dir.display(),
                    extractor
                ));
            }
        }
        for executor in &source.adapters.executors {
            if !registries.executors.contains_key(executor) {
                return Err(anyhow!(
                    "source {} references unknown executor {}",
                    source.root_dir.display(),
                    executor
                ));
            }
        }
        if let Some(modules) = registries.source_modules.get(&source.root_dir) {
            for module in modules {
                if !registries.rhai_modules.contains_key(module) {
                    return Err(anyhow!(
                        "source {} references unknown rhai module {}",
                        source.root_dir.display(),
                        module
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut mode = Mode::Stdio;
    let mut http_addr = None;
    let mut config_path = None;
    let mut watch_roots = Vec::new();
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "stdio" => mode = Mode::Stdio,
            "http" => mode = Mode::Http,
            "--addr" => {
                http_addr = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--addr requires a value"))?,
                );
            }
            "--watch" => watch_roots.push(
                iter.next()
                    .ok_or_else(|| anyhow!("--watch requires a path"))?,
            ),
            "--config" => {
                config_path = Some(PathBuf::from(
                    iter.next()
                        .ok_or_else(|| anyhow!("--config requires a path"))?,
                ));
            }
            other => return Err(anyhow!("unsupported argument: {other}")),
        }
    }

    Ok(Args {
        mode,
        http_addr,
        config_path,
        watch_roots,
    })
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

struct ConfiguredFileContentHandler {
    sources: Vec<DirectorySource>,
    registries: RuntimeRegistries,
}

impl ConfiguredFileContentHandler {
    fn new(sources: Vec<DirectorySource>, registries: RuntimeRegistries) -> Self {
        Self {
            sources,
            registries,
        }
    }

    fn resolve_source(&self, path: &Path) -> Option<&DirectorySource> {
        let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.sources
            .iter()
            .find(|source| source.contains_path(&absolute))
    }
}

static SEMANTIC_RUNTIME: LazyLock<SemanticRuntime> = LazyLock::new(SemanticRuntime::new);

#[async_trait]
impl Handler for ConfiguredFileContentHandler {
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction> {
        let path = Path::new(&file.path);
        let source = self.resolve_source(path);
        if let Some(source) = source {
            for extractor in &source.adapters.extractors {
                match self.registries.extractors.get(extractor) {
                    Some(ExtractorKind::BuiltinText) => {}
                    None => {
                        return Err(anyhow!(
                            "source {} references missing extractor {}",
                            source.root_dir.display(),
                            extractor
                        ))
                    }
                }
            }
        }

        let bytes = tokio::fs::read(path).await?;
        let text = String::from_utf8_lossy(&bytes).into_owned();

        if let Some(source) = source {
            if let Some(module_names) = self.registries.source_modules.get(&source.root_dir) {
                let rules = module_names
                    .iter()
                    .filter_map(|name| self.registries.rhai_modules.get(name))
                    .cloned()
                    .collect::<Vec<_>>();
                if !rules.is_empty() {
                    let staged = source.stage_artifact(path)?;
                    let _ = SEMANTIC_RUNTIME
                        .correlate(&synthetic_bundle(staged.artifact, &text), &rules)?;
                }
            }
        }

        Ok(Extraction {
            text,
            has_symbols: matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rs" | "py" | "ts" | "tsx" | "js" | "sql")
            ),
        })
    }
}

fn synthetic_bundle(artifact: Artifact, text: &str) -> EvidenceBundle {
    EvidenceBundle {
        artifact: artifact.clone(),
        namespaces: vec![],
        anchors: vec![],
        observations: vec![Observation {
            id: format!("obs:{}", artifact.id),
            artifact_id: artifact.id,
            anchor_id: None,
            kind: "extracted_text".to_string(),
            content: text.to_string(),
            attributes: BTreeMap::new(),
            confidence: 0.7,
            namespace: None,
        }],
        claims: vec![],
        concepts: vec![],
        entities: vec![],
        relations: vec![],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{build_runtime_bootstrap, config::load_phase2d_config};

    #[test]
    fn bootstrap_splits_watch_and_poll_sources_from_directory_configs() {
        let root = tempdir().expect("tempdir");
        let watch_root = root.path().join("repo");
        let poll_root = root.path().join("docs");
        fs::create_dir(&watch_root).expect("mkdir watch root");
        fs::create_dir(&poll_root).expect("mkdir poll root");
        fs::write(
            watch_root.join(".promptexecution.toml"),
            r#"
            [source]
            id = "git://workspace"
            kind = "git"

            [metadata]
            tags = { domain = "platform" }

            [adapters]
            extractors = ["text"]
            "#,
        )
        .expect("write watch config");
        fs::write(
            poll_root.join(".promptexecution.toml"),
            r#"
            [source]
            id = "sharepoint://delivery"
            kind = "sharepoint"

            [poll]
            mode = "interval"
            interval_seconds = 120

            [adapters]
            extractors = ["text"]
            "#,
        )
        .expect("write poll config");
        let config_path = root.path().join("phase2d.toml");
        fs::write(
            &config_path,
            format!(
                r#"
                [[sources]]
                root = "{}"

                [[sources]]
                root = "{}"

                [[registries.extractors]]
                name = "text"
                kind = "builtin_text"
                "#,
                watch_root.display(),
                poll_root.display()
            ),
        )
        .expect("write phase2d config");

        let loaded = load_phase2d_config(Some(&config_path)).expect("load config");
        let bootstrap = build_runtime_bootstrap(&loaded, &[]).expect("build bootstrap");

        let watch = bootstrap.phase2.watch.as_ref().expect("watch config");
        assert_eq!(watch.config.roots.len(), 1);
        assert!(watch.config.roots[0].ends_with("repo"));
        assert_eq!(bootstrap.phase2.polls.len(), 1);
        assert_eq!(bootstrap.phase2.polls[0].config.interval_seconds, 120);
        assert!(bootstrap.phase2.polls[0].config.roots[0].ends_with("docs"));
    }
}
