pub mod resources;
pub mod tools;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use retrieval::SearchBackend;

use crate::tools::{
    ontology_list_classes::OntologyListClassesTool, repo_read_symbol::RepoReadSymbolTool,
    repo_search::RepoSearchTool,
};

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn call(&self, params: Value) -> Result<Value>;
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
}
