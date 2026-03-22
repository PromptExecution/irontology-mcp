use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use storage_neumann::KnowledgeStore;

use crate::Tool;

pub struct OntologyListClassesTool {
    store: Arc<dyn KnowledgeStore>,
}

impl OntologyListClassesTool {
    pub fn new(store: Arc<dyn KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for OntologyListClassesTool {
    fn name(&self) -> &str {
        "ontology.list_classes"
    }

    fn description(&self) -> &str {
        "List available ontology classes"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn call(&self, _params: Value) -> anyhow::Result<Value> {
        let classes = self.store.list_classes().await?;
        Ok(json!({ "classes": classes }))
    }
}
