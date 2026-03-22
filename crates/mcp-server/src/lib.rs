pub mod resources;
pub mod tools;

use anyhow::Result;
use async_trait::async_trait;
use axum::{extract::State, routing::post, Json, Router};
use forward_mcp::{DisabledForwarder, McpForwarder, TransportForwarder};
use indexer::{
    spawn_poller, spawn_watchexec, GitLedger, Handler as IndexHandler, IndexingChangeProcessor,
    ModelProvider, PollConfig, PollingRuntime, RuleMatcher, WatchConfig, WatchexecRuntime,
};
use orchestrator::{AgentExecutor, DisabledExecutor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};

use retrieval::SearchBackend;
use storage_neumann::{config::NeumannConfig, KnowledgeStore, NeumannStore};

use crate::tools::{
    agent_forward_mcp::AgentForwardMcpTool, agent_run::AgentRunTool,
    ontology_list_classes::OntologyListClassesTool,
    ontology_related_resources::OntologyRelatedResourcesTool,
    repo_index::RepoIndexTool,
    repo_read_symbol::RepoReadSymbolTool,
    repo_search::RepoSearchTool,
};

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn call(&self, params: Value) -> Result<Value>;
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub uri: String,
    pub mime_type: String,
    pub body: String,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn all(&self) -> impl Iterator<Item = Arc<dyn Tool>> + '_ {
        self.tools.values().cloned()
    }

    pub fn with_phase2_tools(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
    ) -> Self {
        Self::with_phase2_tools_and_execution(
            backend,
            store,
            None,
            Arc::new(DisabledForwarder),
            Arc::new(DisabledExecutor),
        )
    }

    pub fn with_phase2_tools_and_forwarder(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        forwarder: Arc<dyn McpForwarder>,
    ) -> Self {
        Self::with_phase2_tools_and_execution(
            backend,
            store,
            None,
            forwarder,
            Arc::new(DisabledExecutor),
        )
    }

    pub fn with_phase2_tools_and_executor(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        executor: Arc<dyn AgentExecutor>,
    ) -> Self {
        Self::with_phase2_tools_and_execution(
            backend,
            store,
            None,
            Arc::new(DisabledForwarder),
            executor,
        )
    }

    pub fn with_phase2_tools_and_provider(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        provider: Arc<dyn ModelProvider>,
    ) -> Self {
        Self::with_phase2_tools_and_ontology_and_execution(
            backend,
            store,
            Some(provider),
            Arc::new(DisabledForwarder),
            Arc::new(DisabledExecutor),
        )
    }

    pub fn with_phase2_tools_and_execution(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        _provider: Option<Arc<dyn ModelProvider>>,
        forwarder: Arc<dyn McpForwarder>,
        executor: Arc<dyn AgentExecutor>,
    ) -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(RepoSearchTool::new(backend, store.clone())));
        registry.register(Arc::new(RepoReadSymbolTool::new(store.clone())));
        registry.register(Arc::new(OntologyListClassesTool::new(store)));
        registry.register(Arc::new(AgentForwardMcpTool::new(forwarder)));
        registry.register(Arc::new(AgentRunTool::new(executor)));
        registry
    }

    pub fn with_phase2_tools_and_ontology(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
    ) -> Self {
        Self::with_phase2_tools_and_ontology_and_execution(
            backend,
            store,
            None,
            Arc::new(DisabledForwarder),
            Arc::new(DisabledExecutor),
        )
    }

    pub fn with_phase2_tools_and_ontology_and_forwarder(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        forwarder: Arc<dyn McpForwarder>,
    ) -> Self {
        Self::with_phase2_tools_and_ontology_and_execution(
            backend,
            store,
            None,
            forwarder,
            Arc::new(DisabledExecutor),
        )
    }

    pub fn with_phase2_tools_and_ontology_and_executor(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        executor: Arc<dyn AgentExecutor>,
    ) -> Self {
        Self::with_phase2_tools_and_ontology_and_execution(
            backend,
            store,
            None,
            Arc::new(DisabledForwarder),
            executor,
        )
    }

    pub fn with_phase2_tools_and_ontology_and_execution(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        provider: Option<Arc<dyn ModelProvider>>,
        forwarder: Arc<dyn McpForwarder>,
        executor: Arc<dyn AgentExecutor>,
    ) -> Self {
        let mut registry = Self::with_phase2_tools_and_execution(
            backend,
            store.clone(),
            provider.clone(),
            forwarder,
            executor,
        );
        if let Some(provider) = provider {
            registry.register(Arc::new(RepoIndexTool::new(store.clone(), provider)));
        }
        registry.register(Arc::new(OntologyRelatedResourcesTool::new(store)));
        registry
    }
}

#[derive(Default)]
pub struct ResourceRegistry {
    resources: HashMap<String, Resource>,
}

impl ResourceRegistry {
    pub fn register(&mut self, resource: Resource) {
        self.resources.insert(resource.uri.clone(), resource);
    }

    pub fn has(&self, uri: &str) -> bool {
        self.resources.contains_key(uri)
    }

    pub fn get(&self, uri: &str) -> Option<Resource> {
        self.resources.get(uri).cloned()
    }

    pub fn all(&self) -> impl Iterator<Item = &Resource> {
        self.resources.values()
    }

    pub fn with_phase2_resources() -> Self {
        let mut registry = Self::default();
        for resource in crate::resources::ontology::phase2_resources() {
            registry.register(resource);
        }
        registry
    }
}

pub struct McpServerRuntime {
    pub tools: ToolRegistry,
    pub resources: ResourceRegistry,
    pub store: Arc<dyn KnowledgeStore>,
    watcher: Option<WatchexecRuntime>,
    pollers: Vec<PollingRuntime>,
}

pub struct WatchRuntimeConfig {
    pub config: WatchConfig,
    pub git_ledger: Arc<dyn GitLedger>,
    pub rules: Arc<dyn RuleMatcher>,
    pub handler: Arc<dyn IndexHandler>,
    pub provider: Arc<dyn ModelProvider>,
}

pub struct PollRuntimeConfig {
    pub config: PollConfig,
    pub git_ledger: Arc<dyn GitLedger>,
    pub rules: Arc<dyn RuleMatcher>,
    pub handler: Arc<dyn IndexHandler>,
    pub provider: Arc<dyn ModelProvider>,
}

pub struct Phase2RuntimeConfig {
    pub neumann: NeumannConfig,
    pub provider: Option<Arc<dyn ModelProvider>>,
    pub forwarder: Arc<dyn McpForwarder>,
    pub executor: Arc<dyn AgentExecutor>,
    pub watch: Option<WatchRuntimeConfig>,
    pub polls: Vec<PollRuntimeConfig>,
}

impl Phase2RuntimeConfig {
    pub fn new(neumann: NeumannConfig) -> Self {
        Self {
            neumann,
            provider: None,
            forwarder: Arc::new(DisabledForwarder),
            executor: Arc::new(DisabledExecutor),
            watch: None,
            polls: Vec::new(),
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn with_transport_forwarding(mut self) -> Self {
        self.forwarder = Arc::new(TransportForwarder::new());
        self
    }

    pub fn with_forwarder(mut self, forwarder: Arc<dyn McpForwarder>) -> Self {
        self.forwarder = forwarder;
        self
    }

    pub fn with_executor(mut self, executor: Arc<dyn AgentExecutor>) -> Self {
        self.executor = executor;
        self
    }

    pub fn with_watch(mut self, watch: WatchRuntimeConfig) -> Self {
        self.watch = Some(watch);
        self
    }

    pub fn with_poll(mut self, poll: PollRuntimeConfig) -> Self {
        self.polls.push(poll);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl McpServerRuntime {
    pub async fn shutdown(self) -> Result<()> {
        if let Some(watcher) = self.watcher {
            watcher.stop().await?;
        }
        for poller in self.pollers {
            poller.stop().await?;
        }
        Ok(())
    }

    pub fn router(runtime: Arc<Self>) -> Router {
        Router::new()
            .route("/", post(handle_http_jsonrpc))
            .route("/mcp", post(handle_http_jsonrpc))
            .with_state(runtime)
    }

    pub async fn serve_stdio(self: Arc<Self>) -> Result<()> {
        Self::serve_stdio_streams(self, tokio::io::stdin(), tokio::io::stdout()).await
    }

    pub async fn serve_stdio_streams<R, W>(runtime: Arc<Self>, reader: R, writer: W) -> Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut lines = BufReader::new(reader).lines();
        let mut writer = BufWriter::new(writer);

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = serde_json::from_str(&line)?;
            let response = runtime.handle_jsonrpc(request).await;
            let body = serde_json::to_vec(&response)?;
            writer.write_all(&body).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }

        Ok(())
    }

    pub async fn handle_jsonrpc(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();
        match self.dispatch_jsonrpc(request).await {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32000,
                    message: error.to_string(),
                }),
            },
        }
    }

    pub async fn start_phase2(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
    ) -> Result<Self> {
        Self::start_phase2_configured(backend, Phase2RuntimeConfig::new(config)).await
    }

    pub async fn start_phase2_with_transport_forwarding(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
    ) -> Result<Self> {
        Self::start_phase2_configured(
            backend,
            Phase2RuntimeConfig::new(config).with_transport_forwarding(),
        )
        .await
    }

    pub async fn start_phase2_with_forwarder(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
        forwarder: Arc<dyn McpForwarder>,
    ) -> Result<Self> {
        Self::start_phase2_configured(
            backend,
            Phase2RuntimeConfig::new(config).with_forwarder(forwarder),
        )
        .await
    }

    pub async fn start_phase2_with_executor(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
        executor: Arc<dyn AgentExecutor>,
    ) -> Result<Self> {
        Self::start_phase2_configured(
            backend,
            Phase2RuntimeConfig::new(config).with_executor(executor),
        )
        .await
    }

    pub async fn start_phase2_with_execution(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
        forwarder: Arc<dyn McpForwarder>,
        executor: Arc<dyn AgentExecutor>,
    ) -> Result<Self> {
        Self::start_phase2_configured(
            backend,
            Phase2RuntimeConfig::new(config)
                .with_forwarder(forwarder)
                .with_executor(executor),
        )
        .await
    }

    pub async fn start_phase2_with_store(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
        mut config: Phase2RuntimeConfig,
    ) -> Result<Self> {
        let resources = ResourceRegistry::with_phase2_resources();

        for resource in resources.all() {
            if resource.mime_type == "text/turtle" {
                store.ingest_turtle(&resource.uri, &resource.body).await?;
            }
        }

        let tools = ToolRegistry::with_phase2_tools_and_ontology_and_execution(
            backend,
            store.clone(),
            config.provider.clone(),
            config.forwarder,
            config.executor,
        );
        let watcher = if let Some(watch) = config.watch.take() {
            let watch_roots = watch.config.roots.clone();
            Some(spawn_watchexec(
                watch.config,
                Arc::new(IndexingChangeProcessor::new(
                    watch_roots,
                    watch.git_ledger,
                    watch.rules,
                    watch.handler,
                    store.clone(),
                    watch.provider,
                )),
            )?)
        } else {
            None
        };
        let mut pollers = Vec::new();
        for poll in config.polls.drain(..) {
            let poll_roots = poll.config.roots.clone();
            pollers.push(spawn_poller(
                poll.config,
                Arc::new(IndexingChangeProcessor::new(
                    poll_roots,
                    poll.git_ledger,
                    poll.rules,
                    poll.handler,
                    store.clone(),
                    poll.provider,
                )),
            )?);
        }

        Ok(Self {
            tools,
            resources,
            store,
            watcher,
            pollers,
        })
    }

    pub async fn start_phase2_configured(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: Phase2RuntimeConfig,
    ) -> Result<Self> {
        let store: Arc<dyn KnowledgeStore> = Arc::new(NeumannStore::try_new(config.neumann.clone())?);
        Self::start_phase2_with_store(backend, store, config).await
    }

    async fn dispatch_jsonrpc(&self, request: JsonRpcRequest) -> Result<Value> {
        match request.method.as_str() {
            "initialize" => Ok(json!({
                "serverInfo": {
                    "name": "promptexecution-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "tools": {},
                    "resources": {},
                }
            })),
            "ping" => Ok(json!({"ok": true})),
            "tools/list" => Ok(json!({
                "tools": self.tools.all().map(|tool| {
                    json!({
                        "name": tool.name(),
                        "description": tool.description(),
                        "inputSchema": tool.input_schema(),
                    })
                }).collect::<Vec<_>>()
            })),
            "tools/call" => {
                let name = request
                    .params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("tool name missing"))?;
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(Value::Null);
                let tool = self
                    .tools
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("unknown tool: {name}"))?;
                let content = tool.call(arguments).await?;
                Ok(json!({
                    "content": [{
                        "type": "json",
                        "json": content,
                    }],
                    "isError": false,
                }))
            }
            "resources/list" => Ok(json!({
                "resources": self.resources.all().map(|resource| {
                    json!({
                        "uri": resource.uri,
                        "mimeType": resource.mime_type,
                    })
                }).collect::<Vec<_>>()
            })),
            "resources/read" => {
                let uri = request
                    .params
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("resource uri missing"))?;
                let resource = self
                    .resources
                    .get(uri)
                    .ok_or_else(|| anyhow::anyhow!("unknown resource: {uri}"))?;
                Ok(json!({
                    "contents": [{
                        "uri": resource.uri,
                        "mimeType": resource.mime_type,
                        "text": resource.body,
                    }]
                }))
            }
            other => Err(anyhow::anyhow!("unsupported json-rpc method: {other}")),
        }
    }
}

async fn handle_http_jsonrpc(
    State(runtime): State<Arc<McpServerRuntime>>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    Json(runtime.handle_jsonrpc(request).await)
}
