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

        // Only traverse the class hierarchy when the request is explicitly about
        // class membership — i.e. the predicate is rdfs:subClassOf, rdf:type, or
        // one of the well-known OWL class-relationship predicates.
        const CLASS_PREDICATES: &[&str] = &[
            "http://www.w3.org/2000/01/rdf-schema#subClassOf",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            "http://www.w3.org/2002/07/owl#equivalentClass",
            "http://www.w3.org/2002/07/owl#disjointWith",
            "rdfs:subClassOf",
            "rdf:type",
            "owl:equivalentClass",
        ];

        let subclasses = if CLASS_PREDICATES.contains(&predicate) {
            self.store.subclasses_of(subject).await?
        } else {
            Vec::new()
        };

        Ok(json!({
            "objects": objects,
            "subclasses": subclasses
        }))
    }
}
