use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use storage_neumann::{KnowledgeStore, ViolationSeverity};

use crate::Tool;

pub struct OntologyValidateTool {
    store: Arc<dyn KnowledgeStore>,
}

impl OntologyValidateTool {
    pub fn new(store: Arc<dyn KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for OntologyValidateTool {
    fn name(&self) -> &str {
        "ontology.validate"
    }

    fn description(&self) -> &str {
        "Validate Turtle RDF content against SHACL shapes loaded in the ontology store. \
         Returns whether the content conforms and a list of violations."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "turtle": {
                    "type": "string",
                    "description": "Turtle RDF content to validate"
                },
                "strict": {
                    "type": "boolean",
                    "description": "When true, treat warnings as violations (default: false)"
                }
            },
            "required": ["turtle"]
        })
    }

    async fn call(&self, params: Value) -> anyhow::Result<Value> {
        let turtle = params
            .get("turtle")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("turtle field is required"))?;
        let strict = params
            .get("strict")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let violations = self.store.validate_turtle(turtle).await?;

        let reported: Vec<_> = violations
            .iter()
            .filter(|v| strict || v.severity == ViolationSeverity::Violation)
            .map(|v| {
                json!({
                    "subject": v.subject,
                    "shape": v.shape,
                    "message": v.message,
                    "severity": match v.severity {
                        ViolationSeverity::Violation => "Violation",
                        ViolationSeverity::Warning => "Warning",
                        ViolationSeverity::Info => "Info",
                    }
                })
            })
            .collect();

        Ok(json!({
            "conforms": reported.is_empty(),
            "violations": reported
        }))
    }
}
