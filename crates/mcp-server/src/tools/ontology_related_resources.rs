use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use storage_neumann::KnowledgeStore;

use crate::Tool;

pub struct OntologyRelatedResourcesTool {
    store: Arc<dyn KnowledgeStore>,
}

impl OntologyRelatedResourcesTool {
    pub fn new(store: Arc<dyn KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for OntologyRelatedResourcesTool {
    fn name(&self) -> &str {
        "ontology.related_resources"
    }

    fn description(&self) -> &str {
        "Resolve semantic objects related to a subject via a predicate from the ontology graph"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string" },
                "predicate": { "type": "string" }
            },
            "required": ["subject", "predicate"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let subject = params
            .get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("subject missing"))?;
        let predicate = params
            .get("predicate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("predicate missing"))?;

        let objects = self.store.related_objects(subject, predicate).await?;
        // If the predicate is rdf:type or similar class lookup, also return
        // transitive subclasses so callers can reason about the class hierarchy.
        let subclasses = self.store.subclasses_of(subject).await.unwrap_or_default();
        Ok(json!({
            "objects": objects,
            "subclasses": subclasses
        }))
    }
}
