//! irontology-mcp — semantic graph/RAG MCP server
//!
//! Stdio transport: JSON-RPC 2.0 over stdin/stdout
//! Tools: repo.search, repo.read_symbol, ontology.list_classes, ontology.related_resources

use anyhow::Result;
use rmcp::{
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, ErrorData, Implementation,
        JsonObject, ListToolsResult, PaginatedRequestParam, ProtocolVersion,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
    transport::io::stdio,
    ServiceExt,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::signal::ctrl_c;

use mcp_server::McpServerRuntime;
use retrieval::{DeterministicBackend, NeumannBackend};
use storage_neumann::{config::NeumannConfig, NeumannStore};

pub struct IrontologyMcpServer {
    runtime: McpServerRuntime,
}

impl IrontologyMcpServer {
    pub async fn new() -> Result<Self> {
        let backend = Box::new(DeterministicBackend);
        // 🤓 data_dir from env: NEUMANN_DATA_DIR (e.g. ~/.b00t/neumann/default)
        let data_dir = std::env::var("NEUMANN_DATA_DIR").ok();
        let config = NeumannConfig {
            endpoint: "http://localhost:7777".into(),
            namespace: "default".into(),
            data_dir,
        };

        // 🤓 NEUMANN_BACKEND=neumann → real embeddings (requires EMBEDDING_ENDPOINT)
        //      default → DeterministicBackend (synthetic, no external deps)
        let use_neumann = std::env::var("NEUMANN_BACKEND")
            .map(|v| v == "neumann")
            .unwrap_or(false);

        let runtime = if use_neumann {
            let store = Arc::new(NeumannStore::new(config.clone()));
            let backend = Box::new(NeumannBackend::new(store));
            McpServerRuntime::start_phase2(backend, config).await?
        } else {
            let backend = Box::new(DeterministicBackend);
            McpServerRuntime::start_phase2(backend, config).await?
        };
        eprintln!("✅ irontology-mcp: runtime initialized");
        Ok(Self { runtime })
    }
}

impl ServerHandler for IrontologyMcpServer {
    async fn ping(&self, _context: RequestContext<RoleServer>) -> Result<(), ErrorData> {
        Ok(())
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            server_info: Implementation {
                name: "irontology-mcp".into(),
                version: "0.1.0".into(),
                ..Default::default()
            },
            instructions: Some(
                "irontology-mcp: semantic graph/RAG MCP server. \
                 Phase 2: NeumannStore + 4-way fusion retrieval (vector 0.35, graph 0.30, lexical 0.20, ontology 0.15)."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools = Vec::new();

        let tool_names = [
            "repo.search",
            "repo.read_symbol",
            "repo.index",
            "ontology.list_classes",
            "ontology.related_resources",
        ];
        for name in &tool_names {
            if let Some(tool) = self.runtime.tools.get(name) {
                let schema: Arc<JsonObject> = match tool.input_schema() {
                    Value::Object(map) => Arc::new(map),
                    _ => Arc::new(serde_json::Map::new()),
                };
                tools.push(Tool::new(
                    tool.name().to_string(),
                    tool.description().to_string(),
                    schema,
                ));
            }
        }

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tool_name = request.name.as_ref();
        let params = Value::Object(request.arguments.unwrap_or_default());

        if let Some(tool) = self.runtime.tools.get(tool_name) {
            match tool.call(params).await {
                Ok(result) => Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )])),
                Err(e) => Ok(CallToolResult::success(vec![Content::text(format!(
                    "Error: {}",
                    e
                ))])),
            }
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Tool {} not found",
                tool_name
            ))]))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = IrontologyMcpServer::new().await?;
    eprintln!("📡 irontology-mcp: listening on stdio");

    let running_service = server.serve(stdio()).await?;

    tokio::select! {
        _ = ctrl_c() => {
            eprintln!("🛑 irontology-mcp: shutdown");
        }
        _ = running_service.waiting() => {
            eprintln!("🛑 irontology-mcp: client disconnected");
        }
    }

    Ok(())
}
