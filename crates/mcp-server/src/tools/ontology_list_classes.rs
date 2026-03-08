use async_trait::async_trait;
use serde_json::{json, Value};

use crate::Tool;

pub struct OntologyListClassesTool;

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
        Ok(crate::resources::ontology::list_classes())
    }
}
