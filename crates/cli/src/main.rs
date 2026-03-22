mod config;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use config::{
    load_phase2d_config, resolve_config_relative, ExtractorKind, LoadedPhase2dConfig,
    ProviderSettings,
};
use domain::{Artifact, Claim, EvidenceBundle, Observation, Relation};
use forward_mcp::{DisabledForwarder, ForwardRequest, McpForwarder, ReturnMode, TransportForwarder};
use indexer::{
    Extraction, GitLedger, Handler, IntakeFile, ModelProvider, PollConfig, RuleMatcher,
    WatchConfig,
};
use intake::{load_directory_sources, DirectorySource, PollMode};
use mcp_server::{McpServerRuntime, Phase2RuntimeConfig, PollRuntimeConfig, WatchRuntimeConfig};
use orchestrator::{AgentExecutor, SimpleAgentExecutor};
use provider_local::{LocalProvider, LocalProviderConfig, MistralRsConfig};
use provider_openai::{OpenAiCompatConfig, OpenAiCompatProvider};
use provider_test::FixtureProvider;
use retrieval::{SearchBackend, StoreBackedBackend};
use storage_neumann::{KnowledgeStore, NeumannStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    backend: Box<dyn SearchBackend + Send + Sync>,
    phase2: Phase2RuntimeConfig,
    store: Arc<dyn KnowledgeStore>,
}

#[derive(Clone)]
struct SharedSearchBackend(Arc<dyn SearchBackend + Send + Sync>);

impl SearchBackend for SharedSearchBackend {
    fn search_vector(&self, query: &str, top_k: usize) -> Result<Vec<retrieval::RankedResult>> {
        self.0.search_vector(query, top_k)
    }

    fn search_graph(&self, query: &str, top_k: usize) -> Result<Vec<retrieval::RankedResult>> {
        self.0.search_graph(query, top_k)
    }

    fn search_lexical(&self, query: &str, top_k: usize) -> Result<Vec<retrieval::RankedResult>> {
        self.0.search_lexical(query, top_k)
    }

    fn search_ontology(&self, query: &str, top_k: usize) -> Result<Vec<retrieval::RankedResult>> {
        self.0.search_ontology(query, top_k)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args(env::args().skip(1))?;
    let loaded = load_phase2d_config(args.config_path.as_deref())?;
    let bootstrap = build_runtime_bootstrap(&loaded, &args.watch_roots)?;
    let runtime = Arc::new(
        McpServerRuntime::start_phase2_with_store(bootstrap.backend, bootstrap.store, bootstrap.phase2).await?,
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
    let forwarder = build_forwarder(loaded.config.forwarding.transport_forwarding);
    let neumann = NeumannConfig::from(loaded.config.neumann.clone());
    let store = Arc::new(NeumannStore::try_new(neumann.clone())?);
    let backend: Arc<dyn SearchBackend + Send + Sync> =
        Arc::new(StoreBackedBackend::from_store(store.as_ref()));
    let executor: Arc<dyn AgentExecutor> =
        Arc::new(SimpleAgentExecutor::new(backend.clone(), provider.clone()));

    let ledger: Arc<dyn GitLedger> = Arc::new(ContentHashLedger);
    let rules: Arc<dyn RuleMatcher> = Arc::new(MatchAllRules);
    let handler: Arc<dyn Handler> = Arc::new(ConfiguredFileContentHandler::new(
        sources.clone(),
        registries.clone(),
        forwarder.clone(),
    ));

    let mut phase2 = Phase2RuntimeConfig::new(NeumannConfig::from(loaded.config.neumann.clone()))
        .with_provider(provider.clone())
        .with_forwarder(forwarder)
        .with_executor(executor);

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
        backend: Box::new(SharedSearchBackend(backend)),
        phase2,
        store,
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

fn build_forwarder(transport_enabled: bool) -> Arc<dyn McpForwarder> {
    if transport_enabled {
        Arc::new(TransportForwarder::new())
    } else {
        Arc::new(DisabledForwarder)
    }
}

fn resolve_executor_target(target: &str, config_path: Option<&Path>) -> String {
    let Some(command) = target.strip_prefix("stdio://child:") else {
        return target.to_string();
    };
    let Some(config_path) = config_path else {
        return target.to_string();
    };
    let Some(root) = config_path.parent() else {
        return target.to_string();
    };
    let Some(parts) = shlex::split(command) else {
        return target.to_string();
    };
    let rewritten = parts
        .into_iter()
        .map(|part| resolve_executor_arg(&part, root))
        .collect::<Vec<_>>();
    format!("stdio://child:{}", rewritten.join(" "))
}

fn resolve_executor_arg(arg: &str, root: &Path) -> String {
    let path = PathBuf::from(arg);
    if path.is_absolute() {
        return arg.to_string();
    }
    let candidate = root.join(&path);
    if candidate.exists() {
        return candidate.display().to_string();
    }
    arg.to_string()
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
        .map(|registration| {
            (
                registration.name.clone(),
                resolve_executor_target(&registration.target, loaded.path.as_deref()),
            )
        })
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
    forwarder: Arc<dyn McpForwarder>,
}

impl ConfiguredFileContentHandler {
    fn new(
        sources: Vec<DirectorySource>,
        registries: RuntimeRegistries,
        forwarder: Arc<dyn McpForwarder>,
    ) -> Self {
        Self {
            sources,
            registries,
            forwarder,
        }
    }

    fn resolve_source(&self, path: &Path) -> Option<&DirectorySource> {
        let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.sources
            .iter()
            .find(|source| source.contains_path(&absolute))
    }

    async fn run_executor(
        &self,
        source: &DirectorySource,
        executor_name: &str,
        path: &Path,
        bytes: &[u8],
    ) -> Result<ExecutorExtraction> {
        let target = self
            .registries
            .executors
            .get(executor_name)
            .ok_or_else(|| anyhow!("source {} references missing executor {}", source.root_dir.display(), executor_name))?;
        let response = self
            .forwarder
            .forward(ForwardRequest {
                target: target.clone(),
                task: format!("Extract structured facts from {}", path.display()),
                allowed_tools: vec![],
                allowed_resources: vec![],
                allowed_prompts: vec![],
                context: vec![
                    source.source.id.clone(),
                    source.root_dir.display().to_string(),
                ],
                budget_tokens: Some(8_000),
                timeout_ms: Some(30_000),
                return_mode: ReturnMode::Structured,
                payload: json!({
                    "path_hint": path.display().to_string(),
                    "media_type": infer_media_type(path),
                    "bytes_b64": STANDARD.encode(bytes),
                    "source_id": source.source.id.clone(),
                    "source_kind": source_kind(&source.source),
                    "tags": source.metadata_tags.clone(),
                    "ontology_refs": source.ontology_refs.clone(),
                }),
            })
            .await?;
        Ok(serde_json::from_value(response.output)?)
    }
}

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
        let mut extraction = Extraction {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            has_symbols: matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rs" | "py" | "ts" | "tsx" | "js" | "sql")
            ),
            fields: BTreeMap::new(),
            class: None,
            shape: None,
            claims: vec![],
            relations: vec![],
            notes: vec![],
        };

        if let Some(source) = source {
            for executor_name in &source.adapters.executors {
                let external = self
                    .run_executor(source, executor_name, path, &bytes)
                    .await?;
                merge_executor_extraction(&mut extraction, external);
            }

            if let Some(module_names) = self.registries.source_modules.get(&source.root_dir) {
                let rules = module_names
                    .iter()
                    .filter_map(|name| self.registries.rhai_modules.get(name))
                    .cloned()
                    .collect::<Vec<_>>();
                if !rules.is_empty() {
                    let staged = source.stage_artifact(path)?;
                    let semantic_runtime = SemanticRuntime::new();
                    let outcome = semantic_runtime
                        .correlate(&synthetic_bundle(staged.artifact, &extraction.text), &rules)?;
                    extraction.claims.extend(outcome.claims);
                    extraction.relations.extend(outcome.relations);
                    extraction.notes.extend(outcome.notes);
                }
            }
        }

        Ok(extraction)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ExecutorExtraction {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    fields: BTreeMap<String, Value>,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    shape: Option<String>,
    #[serde(default)]
    claims: Vec<Claim>,
    #[serde(default)]
    relations: Vec<Relation>,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(default)]
    has_symbols: Option<bool>,
}

fn merge_executor_extraction(extraction: &mut Extraction, external: ExecutorExtraction) {
    if let Some(text) = external.text {
        if extraction.text.trim().is_empty() {
            extraction.text = text;
        } else if !text.trim().is_empty() && text != extraction.text {
            extraction.text = format!("{}\n\n{}", extraction.text, text);
        }
    }
    extraction.fields.extend(external.fields);
    extraction.class = extraction.class.take().or(external.class);
    extraction.shape = extraction.shape.take().or(external.shape);
    extraction.claims.extend(external.claims);
    extraction.relations.extend(external.relations);
    extraction.notes.extend(external.notes);
    extraction.has_symbols |= external.has_symbols.unwrap_or(false);
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

fn infer_media_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("csv") => "text/csv",
        Some("docx") => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        Some("drawio") => "application/vnd.jgraph.mxfile",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("pptx") => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        Some("rs" | "py" | "toml" | "md" | "txt" | "yaml" | "yml") => "text/plain",
        _ => "",
    }
}

fn source_kind(source: &intake::SourceSystem) -> String {
    match &source.kind {
        domain::SourceSystemKind::GitRepository => "git_repository".to_string(),
        domain::SourceSystemKind::SharePoint => "sharepoint".to_string(),
        domain::SourceSystemKind::DatabaseSchema => "database_schema".to_string(),
        domain::SourceSystemKind::DocumentSilo => "document_silo".to_string(),
        domain::SourceSystemKind::ProcessCatalog => "process_catalog".to_string(),
        domain::SourceSystemKind::Other(value) => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, sync::Arc};

    use anyhow::Result;
    use async_trait::async_trait;
    use forward_mcp::{ForwardResponse, McpForwarder};
    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        build_registries, build_runtime_bootstrap, config::load_phase2d_config,
        ConfiguredFileContentHandler, ExecutorExtraction,
    };

    #[derive(Default)]
    struct MockForwarder;

    #[async_trait]
    impl McpForwarder for MockForwarder {
        async fn forward(&self, request: forward_mcp::ForwardRequest) -> Result<ForwardResponse> {
            Ok(ForwardResponse {
                target: request.target,
                output: json!(ExecutorExtraction {
                    text: Some("executor extracted standing data".to_string()),
                    fields: BTreeMap::from([("curated_by".to_string(), json!("python"))]),
                    class: Some("doc:ArchitectureDocument".to_string()),
                    shape: Some("shape:ArchitectureDocument".to_string()),
                    claims: vec![],
                    relations: vec![],
                    notes: vec!["executor note".to_string()],
                    has_symbols: Some(false),
                }),
                trace: vec!["mock-forwarder".to_string()],
                artifacts: vec![],
            })
        }
    }

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

    #[test]
    fn registry_resolves_executor_targets_relative_to_runtime_config() {
        let root = tempdir().expect("tempdir");
        fs::create_dir(root.path().join("executors")).expect("mkdir executors");
        fs::write(
            root.path().join("executors/python_curator.py"),
            "print('ok')\n",
        )
        .expect("write executor");
        let config_path = root.path().join("phase2d.toml");
        fs::write(
            &config_path,
            r#"
            [[registries.executors]]
            name = "python_curator"
            target = "stdio://child:python3 executors/python_curator.py"
            "#,
        )
        .expect("write config");

        let loaded = load_phase2d_config(Some(&config_path)).expect("load config");
        let registries = build_registries(&loaded).expect("build registries");

        assert!(registries.executors["python_curator"].contains("executors/python_curator.py"));
        assert!(registries.executors["python_curator"].contains(root.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn configured_handler_merges_executor_and_rhai_outputs() {
        let root = tempdir().expect("tempdir");
        let source_root = root.path().join("docs");
        fs::create_dir(&source_root).expect("mkdir source");
        fs::write(
            source_root.join(".promptexecution.toml"),
            r#"
            [source]
            id = "sharepoint://acme"
            kind = "sharepoint"

            [adapters]
            extractors = ["text"]
            executors = ["python_curator"]
            "#,
        )
        .expect("write source config");
        let script_path = root.path().join("latent.rhai");
        fs::write(
            &script_path,
            r#"
            let texts = observation_texts(ctx, "extracted_text");
            if texts.len > 0 && texts[0].contains("standing data") {
                emit_claim(
                    ctx,
                    "concept:acme:rhai",
                    "semantic:correlates_with",
                    "view:acme:graph",
                    "ctx:acme",
                    0.55
                );
                emit_note(ctx, "rhai note");
            }
            "#,
        )
        .expect("write rhai");
        let config_path = root.path().join("phase2d.toml");
        fs::write(
            &config_path,
            format!(
                r#"
                [[sources]]
                root = "{}"
                rhai_modules = ["latent"]

                [[registries.extractors]]
                name = "text"
                kind = "builtin_text"

                [[registries.executors]]
                name = "python_curator"
                target = "stdio://child:python3 not-used.py"

                [[registries.rhai_modules]]
                name = "latent"
                path = "{}"
                "#,
                source_root.display(),
                script_path.display()
            ),
        )
        .expect("write runtime config");
        let file = source_root.join("standing-data.md");
        fs::write(&file, "standing data architecture notes").expect("write file");

        let loaded = load_phase2d_config(Some(&config_path)).expect("load config");
        let registries = build_registries(&loaded).expect("registries");
        let handler = ConfiguredFileContentHandler::new(
            vec![intake::DirectorySource::load(&source_root).expect("source")],
            registries,
            Arc::new(MockForwarder),
        );

        let extraction = indexer::Handler::extract(
            &handler,
            &indexer::IntakeFile::from_path(&file),
        )
        .await
        .expect("extract");

        assert_eq!(
            extraction.fields["curated_by"],
            serde_json::json!("python")
        );
        assert_eq!(extraction.class.as_deref(), Some("doc:ArchitectureDocument"));
        assert!(extraction
            .claims
            .iter()
            .any(|claim| claim.subject == "concept:acme:rhai"));
        assert!(extraction.notes.iter().any(|note| note == "executor note"));
        assert!(extraction.notes.iter().any(|note| note == "rhai note"));
    }
}
