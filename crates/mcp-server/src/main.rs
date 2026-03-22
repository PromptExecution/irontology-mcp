//! irontology-mcp — semantic graph/RAG MCP server
//!
//! Stdio transport: JSON-RPC 2.0 over stdin/stdout
//! Tools: repo.search, repo.read_symbol, ontology.list_classes, ontology.related_resources
//! (repo.index is available only when a provider is configured, e.g., via the CLI runtime)

use anyhow::Result;
use rmcp::{
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, Implementation, ListToolsResult,
        PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::io::stdio,
    ErrorData as McpError, RoleServer, ServiceExt,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::signal::ctrl_c;

use mcp_server::McpServerRuntime;
use retrieval::{DeterministicBackend, NeumannBackend};
use storage_neumann::{NeumannConfig, NeumannStore};

/// The set of tools exposed through MCP list_tools and callable via call_tool.
/// Any tool registered in the runtime but absent from this list is intentionally hidden.
const EXPOSED_TOOLS: &[&str] = &[
    "repo.search",
    "repo.read_symbol",
    "ontology.list_classes",
    "ontology.related_resources",
];

pub struct IrontologyMcpServer {
    runtime: McpServerRuntime,
}

impl IrontologyMcpServer {
    pub async fn new() -> Result<Self> {
        // 🤓 data_path from env: NEUMANN_DATA_DIR (e.g. ~/.b00t/neumann/default)
        let data_path = std::env::var("NEUMANN_DATA_DIR").ok().map(Into::into);
        let config = NeumannConfig {
            endpoint: "http://localhost:7777".into(),
            namespace: "default".into(),
            data_path,
        };

        // 🤓 NEUMANN_BACKEND=neumann → real embeddings (requires EMBEDDING_ENDPOINT)
        //      default → DeterministicBackend (synthetic, no external deps)
        let use_neumann = std::env::var("NEUMANN_BACKEND")
            .map(|v| v == "neumann")
            .unwrap_or(false);

        let runtime = if use_neumann {
            let store = Arc::new(NeumannStore::try_new(config.clone())?);
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
    fn ping(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send + '_ {
        std::future::ready(Ok(()))
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

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            let mut tools = Vec::new();

            for name in EXPOSED_TOOLS {
                if let Some(tool) = self.runtime.tools.get(name) {
                    let input_schema = match tool.input_schema() {
                        Value::Object(schema) => Arc::new(schema),
                        other => {
                            return Err(McpError::invalid_request(
                                format!("tool {} returned non-object input schema", tool.name()),
                                Some(other),
                            ));
                        }
                    };

                    tools.push(Tool {
                        name: tool.name().to_string().into(),
                        title: None,
                        description: Some(tool.description().to_string().into()),
                        input_schema,
                        output_schema: None,
                        annotations: None,
                        icons: None,
                    });
                }
            }

            Ok(ListToolsResult {
                tools,
                next_cursor: None,
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let tool_name = request.name.as_ref();
            let params = Value::Object(request.arguments.unwrap_or_default());

            if !EXPOSED_TOOLS.contains(&tool_name) {
                return Err(McpError::invalid_request(
                    format!("tool {} not found", tool_name),
                    None,
                ));
            }

            if let Some(tool) = self.runtime.tools.get(tool_name) {
                match tool.call(params).await {
                    Ok(result) => Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )])),
                    Err(e) => Err(McpError::internal_error(
                        format!("tool {} failed: {}", tool_name, e),
                        None,
                    )),
                }
            } else {
                Err(McpError::invalid_request(
                    format!("tool {} not found", tool_name),
                    None,
                ))
            }
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
