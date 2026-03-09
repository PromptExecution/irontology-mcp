pub mod resources;
pub mod tools;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use retrieval::SearchBackend;
use storage_neumann::{config::NeumannConfig, KnowledgeStore, NeumannStore};

use crate::tools::{
    ontology_list_classes::OntologyListClassesTool,
    ontology_related_resources::OntologyRelatedResourcesTool, repo_read_symbol::RepoReadSymbolTool,
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

    pub fn with_phase2_tools(backend: Box<dyn SearchBackend + Send + Sync>) -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(RepoSearchTool::new(backend)));
        registry.register(Arc::new(RepoReadSymbolTool));
        registry.register(Arc::new(OntologyListClassesTool));
        registry
    }

    pub fn with_phase2_tools_and_ontology(
        backend: Box<dyn SearchBackend + Send + Sync>,
        store: Arc<dyn KnowledgeStore>,
    ) -> Self {
        let mut registry = Self::with_phase2_tools(backend);
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
}

impl McpServerRuntime {
    pub async fn start_phase2(
        backend: Box<dyn SearchBackend + Send + Sync>,
        config: NeumannConfig,
    ) -> Result<Self> {
        let resources = ResourceRegistry::with_phase2_resources();
        let store: Arc<dyn KnowledgeStore> = Arc::new(NeumannStore::new(config));

        for resource in resources.all() {
            if resource.mime_type == "text/turtle" {
                store.ingest_turtle(&resource.uri, &resource.body).await?;
            }
        }

        let tools = ToolRegistry::with_phase2_tools_and_ontology(backend, store);
        Ok(Self { tools, resources })
    }
}
