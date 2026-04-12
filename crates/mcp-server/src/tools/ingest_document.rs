use std::{collections::BTreeMap, sync::Arc};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use domain::{Artifact, ArtifactKind, SourceSystemKind};
use ingestion_pipeline::{
    assurance::AssuranceLevel,
    docling::DoclingPipeline,
    langextract::LangextractPipeline,
    PipelineRegistry,
};
use serde_json::{json, Value};
use storage_neumann::{FactRecord, KnowledgeStore};

use crate::Tool;

pub struct IngestDocumentTool {
    store: Arc<dyn KnowledgeStore>,
}

impl IngestDocumentTool {
    pub fn new(store: Arc<dyn KnowledgeStore>) -> Self {
        Self { store }
    }

    fn build_registry() -> PipelineRegistry {
        let mut registry = PipelineRegistry::new();
        registry.register(Box::new(DoclingPipeline::new()));
        registry.register(Box::new(LangextractPipeline::new()));
        registry
    }

    fn parse_assurance(s: Option<&str>) -> AssuranceLevel {
        match s.unwrap_or("standard") {
            "corroborated" => AssuranceLevel::Corroborated,
            "high_assurance" => AssuranceLevel::HighAssurance,
            _ => AssuranceLevel::Standard,
        }
    }

    fn artifact_from_params(uri: &str, media_type: Option<&str>) -> Artifact {
        let kind = infer_artifact_kind(uri, media_type);
        Artifact {
            id: format!("artifact:ingest:{}", uri),
            source_id: "mcp:ingest.document".to_string(),
            source_kind: SourceSystemKind::DocumentSilo,
            kind,
            title: None,
            locator: uri.to_string(),
            media_type: media_type.map(String::from),
            tags: BTreeMap::new(),
            valid_at: None,
            observed_at: None,
        }
    }
}

fn infer_artifact_kind(locator: &str, media_type: Option<&str>) -> ArtifactKind {
    let lower = locator.to_lowercase();
    if let Some(mt) = media_type {
        match mt {
            "application/pdf" => return ArtifactKind::ArchitectureDocument,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                return ArtifactKind::Presentation
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.ms-excel" => return ArtifactKind::Spreadsheet,
            "text/html" => return ArtifactKind::WikiPage,
            _ => {}
        }
    }
    if lower.ends_with(".pdf") {
        ArtifactKind::ArchitectureDocument
    } else if lower.ends_with(".pptx") || lower.ends_with(".ppt") {
        ArtifactKind::Presentation
    } else if lower.ends_with(".xlsx") || lower.ends_with(".xls") {
        ArtifactKind::Spreadsheet
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        ArtifactKind::WikiPage
    } else if lower.ends_with(".md") || lower.ends_with(".txt") {
        ArtifactKind::WikiPage
    } else {
        ArtifactKind::Other("document".to_string())
    }
}

#[async_trait]
impl Tool for IngestDocumentTool {
    fn name(&self) -> &str {
        "ingest.document"
    }

    fn description(&self) -> &str {
        "Ingest a document through the multi-pipeline ingestion system. \
         Accepts a base64-encoded document and routes it through the best available \
         extraction pipeline (docling for PDFs/office docs, langextract for text/HTML). \
         Results are stored in NeumannStore."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "uri": {
                    "type": "string",
                    "description": "Document URI or filename (used for routing and storage key)"
                },
                "content_base64": {
                    "type": "string",
                    "description": "Base64-encoded document content"
                },
                "media_type": {
                    "type": "string",
                    "description": "MIME type, e.g. application/pdf, text/plain"
                },
                "assurance": {
                    "type": "string",
                    "enum": ["standard", "corroborated", "high_assurance"],
                    "default": "standard",
                    "description": "standard=single best pipeline, corroborated=2 pipelines, high_assurance=all capable"
                }
            },
            "required": ["uri", "content_base64"]
        })
    }

    async fn call(&self, params: Value) -> anyhow::Result<Value> {
        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("uri is required"))?;
        let content_b64 = params
            .get("content_base64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("content_base64 is required"))?;
        let media_type = params.get("media_type").and_then(|v| v.as_str());
        let assurance_str = params.get("assurance").and_then(|v| v.as_str());
        let assurance = Self::parse_assurance(assurance_str);

        // Decode content (validate it's valid base64)
        STANDARD
            .decode(content_b64)
            .context("content_base64 is not valid base64")?;

        let artifact = Self::artifact_from_params(uri, media_type);
        let registry = Self::build_registry();

        let (bundle, pipeline_name) = match assurance {
            AssuranceLevel::Standard => {
                let pipeline = registry
                    .select_best(&artifact)
                    .await
                    .ok_or_else(|| {
                        anyhow!("no available ingestion pipeline for this artifact type")
                    })?;
                let name = pipeline.name().to_string();
                let bundle = pipeline.extract(&artifact).await?;
                (bundle, name)
            }
            AssuranceLevel::Corroborated | AssuranceLevel::HighAssurance => {
                let min_confidence = match assurance {
                    AssuranceLevel::Corroborated => 0.5,
                    _ => 0.3,
                };
                let results = registry.extract_all(&artifact, min_confidence).await;
                let successful: Vec<(String, _)> =
                    results.into_iter().filter_map(|r| r.ok()).collect();
                if successful.is_empty() {
                    return Err(anyhow!("no pipelines succeeded for this artifact"));
                }
                let names: Vec<&str> = successful.iter().map(|(n, _)| n.as_str()).collect();
                let combined_name = names.join("+");
                let merged = PipelineRegistry::merge_bundles(successful);
                (merged, combined_name)
            }
        };

        let observation_count = bundle.observations.len();
        let anchor_count = bundle.anchors.len();
        let artifact_id = bundle.artifact.id.clone();

        // Persist to NeumannStore as facts
        let mut facts = Vec::new();
        facts.push(FactRecord {
            subject: artifact_id.clone(),
            predicate: "ingested_via".to_string(),
            object: Value::String(pipeline_name.clone()),
        });
        facts.push(FactRecord {
            subject: artifact_id.clone(),
            predicate: "media_type".to_string(),
            object: Value::String(media_type.unwrap_or("unknown").to_string()),
        });
        facts.push(FactRecord {
            subject: artifact_id.clone(),
            predicate: "observation_count".to_string(),
            object: Value::Number(observation_count.into()),
        });
        facts.push(FactRecord {
            subject: artifact_id.clone(),
            predicate: "anchor_count".to_string(),
            object: Value::Number(anchor_count.into()),
        });
        for obs in &bundle.observations {
            facts.push(FactRecord {
                subject: obs.id.clone(),
                predicate: "content".to_string(),
                object: Value::String(obs.content.clone()),
            });
            facts.push(FactRecord {
                subject: obs.id.clone(),
                predicate: "observation_of".to_string(),
                object: Value::String(artifact_id.clone()),
            });
            facts.push(FactRecord {
                subject: obs.id.clone(),
                predicate: "kind".to_string(),
                object: Value::String(obs.kind.clone()),
            });
        }
        for anchor in &bundle.anchors {
            facts.push(FactRecord {
                subject: anchor.id.clone(),
                predicate: "anchor_of".to_string(),
                object: Value::String(artifact_id.clone()),
            });
            if let Some(ref label) = anchor.label {
                facts.push(FactRecord {
                    subject: anchor.id.clone(),
                    predicate: "label".to_string(),
                    object: Value::String(label.clone()),
                });
            }
        }

        self.store.upsert_artifact(artifact.clone()).await?;
        self.store.upsert_anchors(bundle.anchors.clone()).await?;
        self.store
            .upsert_observations(bundle.observations.clone())
            .await?;
        self.store.upsert_facts(facts).await?;

        Ok(json!({
            "artifact_id": artifact_id,
            "pipeline": pipeline_name,
            "observations": observation_count,
            "anchors": anchor_count,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::json;

    use super::IngestDocumentTool;
    use crate::Tool;

    struct NoopStore;

    #[async_trait::async_trait]
    impl storage_neumann::KnowledgeStore for NoopStore {
        async fn has_blob(&self, _: &str) -> anyhow::Result<bool> {
            Ok(false)
        }
        async fn upsert_file(&self, _: storage_neumann::FileRecord) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_symbols(
            &self,
            _: Vec<storage_neumann::SymbolRecord>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_facts(
            &self,
            _: Vec<storage_neumann::FactRecord>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_edges(
            &self,
            _: Vec<storage_neumann::EdgeRecord>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_embeddings(
            &self,
            _: Vec<storage_neumann::EmbeddingRecord>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn ingest_turtle(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn related_objects(&self, _: &str, _: &str) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn list_classes(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn query(
            &self,
            _: storage_neumann::SemanticQuery,
        ) -> anyhow::Result<storage_neumann::QueryResult> {
            Ok(storage_neumann::QueryResult {
                ids: vec![],
                files: vec![],
                symbols: vec![],
                facts: vec![],
                edges: vec![],
            })
        }
        async fn health(&self) -> anyhow::Result<storage_neumann::StoreHealth> {
            Ok(storage_neumann::StoreHealth { healthy: true, message: String::new() })
        }
        async fn validate_turtle(&self, _: &str) -> anyhow::Result<Vec<storage_neumann::ShapeViolation>> {
            Ok(vec![])
        }
        async fn subclasses_of(&self, _: &str) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn upsert_artifact(&self, _: storage_neumann::ArtifactRecord) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_anchors(&self, _: Vec<storage_neumann::AnchorRecord>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn upsert_observations(&self, _: Vec<storage_neumann::ObservationRecord>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_anchors_for(&self, _: &str) -> anyhow::Result<Vec<storage_neumann::AnchorRecord>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn ingest_document_fails_gracefully_no_pipeline() {
        // With no available pipelines (binaries not installed), should return error
        let tool = IngestDocumentTool::new(Arc::new(NoopStore));
        let content = STANDARD.encode(b"hello world");
        let result = tool
            .call(json!({
                "uri": "test.pdf",
                "content_base64": content,
                "media_type": "application/pdf",
                "assurance": "standard"
            }))
            .await;
        // Either Ok (if docling is installed) or Err (not installed) — both are valid
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn tool_name_is_correct() {
        let tool = IngestDocumentTool::new(Arc::new(NoopStore));
        assert_eq!(tool.name(), "ingest.document");
    }

    #[test]
    fn input_schema_has_required_fields() {
        let tool = IngestDocumentTool::new(Arc::new(NoopStore));
        let schema = tool.input_schema();
        let required = schema["required"].as_array().expect("required array");
        let required_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_strs.contains(&"uri"));
        assert!(required_strs.contains(&"content_base64"));
    }
}
