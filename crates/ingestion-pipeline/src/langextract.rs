use std::{
    collections::BTreeMap,
    io::Write,
    process::Command,
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use domain::{Artifact, ArtifactKind, EvidenceBundle, Observation, SourceSystemKind};
use serde_json::Value;

use crate::{ExternalIngestionPipeline, PipelineConfidence};

pub struct LangextractPipeline {
    python_binary: String,
    endpoint: Option<String>,
}

impl Default for LangextractPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl LangextractPipeline {
    pub fn new() -> Self {
        Self {
            python_binary: "python3".to_string(),
            endpoint: None,
        }
    }

    pub fn with_python(python_binary: impl Into<String>) -> Self {
        Self {
            python_binary: python_binary.into(),
            endpoint: None,
        }
    }

    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            python_binary: "python3".to_string(),
            endpoint: Some(endpoint.into()),
        }
    }

    fn base_confidence(artifact: &Artifact) -> f32 {
        let locator_lower = artifact.locator.to_lowercase();
        let from_extension: f32 = if locator_lower.ends_with(".txt") || locator_lower.ends_with(".md") {
            0.80
        } else if locator_lower.ends_with(".pdf") || locator_lower.ends_with(".html") {
            0.70
        } else {
            0.30
        };

        // Check media type as well
        let from_media = match artifact.media_type.as_deref() {
            Some("text/plain") => 0.80,
            Some("text/markdown") => 0.80,
            Some("application/pdf") => 0.70,
            Some("text/html") => 0.70,
            _ => 0.0,
        };

        from_extension.max(from_media)
    }

    fn source_kind_bonus(artifact: &Artifact) -> f32 {
        // +0.10 if artifact kind is WikiPage or source kind is DocumentSilo
        let wiki_bonus = matches!(artifact.kind, ArtifactKind::WikiPage);
        let silo_bonus = matches!(artifact.source_kind, SourceSystemKind::DocumentSilo);
        if wiki_bonus || silo_bonus {
            0.10
        } else {
            0.0
        }
    }

    fn parse_langextract_json(artifact: &Artifact, json: &Value) -> EvidenceBundle {
        let artifact_id = artifact.id.clone();
        let mut observations = Vec::new();

        // Map entities
        if let Some(entities) = json.get("entities").and_then(|v| v.as_array()) {
            for (i, entity) in entities.iter().enumerate() {
                let content = entity
                    .get("text")
                    .or_else(|| entity.get("value"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| serde_json::to_string(entity).unwrap_or_default());
                if content.is_empty() {
                    continue;
                }
                let mut attributes = BTreeMap::new();
                if let Some(kind) = entity.get("type").and_then(|v| v.as_str()) {
                    attributes.insert("entity_type".to_string(), Value::String(kind.to_string()));
                }
                observations.push(Observation {
                    id: format!("obs:langextract:{}:entity:{}", artifact_id, i),
                    artifact_id: artifact_id.clone(),
                    anchor_id: None,
                    kind: "entity".to_string(),
                    content,
                    attributes,
                    confidence: 0.75,
                    namespace: None,
                });
            }
        }

        // Map claims
        if let Some(claims) = json.get("claims").and_then(|v| v.as_array()) {
            for (i, claim) in claims.iter().enumerate() {
                let content = claim
                    .get("text")
                    .or_else(|| claim.get("statement"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| serde_json::to_string(claim).unwrap_or_default());
                if content.is_empty() {
                    continue;
                }
                observations.push(Observation {
                    id: format!("obs:langextract:{}:claim:{}", artifact_id, i),
                    artifact_id: artifact_id.clone(),
                    anchor_id: None,
                    kind: "claim".to_string(),
                    content,
                    attributes: BTreeMap::new(),
                    confidence: 0.70,
                    namespace: None,
                });
            }
        }

        // Map concepts
        if let Some(concepts) = json.get("concepts").and_then(|v| v.as_array()) {
            for (i, concept) in concepts.iter().enumerate() {
                let content = concept
                    .get("label")
                    .or_else(|| concept.get("name"))
                    .or_else(|| concept.get("text"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| serde_json::to_string(concept).unwrap_or_default());
                if content.is_empty() {
                    continue;
                }
                observations.push(Observation {
                    id: format!("obs:langextract:{}:concept:{}", artifact_id, i),
                    artifact_id: artifact_id.clone(),
                    anchor_id: None,
                    kind: "concept".to_string(),
                    content,
                    attributes: BTreeMap::new(),
                    confidence: 0.65,
                    namespace: None,
                });
            }
        }

        EvidenceBundle {
            artifact: artifact.clone(),
            namespaces: Vec::new(),
            anchors: Vec::new(),
            observations,
            claims: Vec::new(),
            concepts: Vec::new(),
            entities: Vec::new(),
            relations: Vec::new(),
        }
    }
}

/// Python script template for langextract extraction.
const LANGEXTRACT_SCRIPT: &str = r#"
import sys, json
import langextract as lx

text = open(sys.argv[1]).read()
result = lx.extract(text, schema={"entities": [], "claims": [], "concepts": []})
print(json.dumps(result))
"#;

#[async_trait]
impl ExternalIngestionPipeline for LangextractPipeline {
    fn name(&self) -> &str {
        "langextract"
    }

    fn can_handle(&self, artifact: &Artifact) -> PipelineConfidence {
        let base = Self::base_confidence(artifact);
        let bonus = Self::source_kind_bonus(artifact);
        PipelineConfidence((base + bonus).min(1.0))
    }

    async fn extract(&self, artifact: &Artifact) -> anyhow::Result<EvidenceBundle> {
        if let Some(ref _endpoint) = self.endpoint {
            return Err(anyhow!("langextract HTTP endpoint mode not yet implemented"));
        }

        // Write the Python script to a temp file
        let mut script_tmp = tempfile::Builder::new()
            .suffix(".py")
            .tempfile()
            .context("failed to create script temp file")?;
        script_tmp
            .write_all(LANGEXTRACT_SCRIPT.as_bytes())
            .context("failed to write script")?;
        script_tmp.flush().context("failed to flush script")?;
        let script_path = script_tmp.path().to_string_lossy().to_string();

        // Materialize artifact content into a temp file so the Python script always
        // receives real bytes regardless of whether the locator is a live file path.
        let source_path = std::path::Path::new(&artifact.locator);
        let mut content_tmp = tempfile::Builder::new()
            .suffix(".txt")
            .tempfile()
            .context("failed to create content temp file")?;
        if source_path.is_file() {
            let mut source = std::fs::File::open(source_path).with_context(|| {
                format!(
                    "failed to open artifact source file '{}'",
                    source_path.display()
                )
            })?;
            std::io::copy(&mut source, &mut content_tmp).with_context(|| {
                format!(
                    "failed to copy artifact source file '{}' into temp file",
                    source_path.display()
                )
            })?;
            content_tmp.flush().context("failed to flush content temp file")?;
        } else {
            return Err(anyhow!(
                "artifact locator '{}' is not a readable file path; langextract requires a real document file",
                artifact.locator
            ));
        }
        let content_path = content_tmp.path().to_string_lossy().to_string();

        let output = Command::new(&self.python_binary)
            .args([&script_path, &content_path])
            .output()
            .with_context(|| {
                format!(
                    "failed to run python binary '{}'",
                    self.python_binary
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "langextract script exited with status {}: {}",
                output.status,
                stderr
            ));
        }

        let stdout =
            String::from_utf8(output.stdout).context("langextract output was not valid UTF-8")?;
        let json: Value =
            serde_json::from_str(&stdout).context("failed to parse langextract JSON output")?;

        Ok(Self::parse_langextract_json(artifact, &json))
    }

    async fn is_available(&self) -> bool {
        Command::new(&self.python_binary)
            .args(["-c", "import langextract"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::{Artifact, ArtifactKind, SourceSystemKind};

    use super::LangextractPipeline;
    use crate::ExternalIngestionPipeline;

    fn make_artifact(
        locator: &str,
        media_type: Option<&str>,
        kind: ArtifactKind,
        source_kind: SourceSystemKind,
    ) -> Artifact {
        Artifact {
            id: format!("artifact:test:{}", locator),
            source_id: "test-source".to_string(),
            source_kind,
            kind,
            title: None,
            locator: locator.to_string(),
            media_type: media_type.map(String::from),
            tags: BTreeMap::new(),
            valid_at: None,
            observed_at: None,
        }
    }

    #[test]
    fn txt_confidence() {
        let p = LangextractPipeline::new();
        let artifact = make_artifact(
            "notes.txt",
            None,
            ArtifactKind::MeetingNotes,
            SourceSystemKind::GitRepository,
        );
        assert!(p.can_handle(&artifact).0 >= 0.75);
    }

    #[test]
    fn pdf_confidence() {
        let p = LangextractPipeline::new();
        let artifact = make_artifact(
            "report.pdf",
            Some("application/pdf"),
            ArtifactKind::ArchitectureDocument,
            SourceSystemKind::SharePoint,
        );
        let c = p.can_handle(&artifact).0;
        assert!(c >= 0.65 && c < 0.85);
    }

    #[test]
    fn wiki_page_bonus() {
        let p = LangextractPipeline::new();
        let artifact = make_artifact(
            "article.html",
            Some("text/html"),
            ArtifactKind::WikiPage,
            SourceSystemKind::GitRepository,
        );
        // base 0.70 + 0.10 bonus = 0.80
        assert!(p.can_handle(&artifact).0 >= 0.79);
    }

    #[test]
    fn document_silo_bonus() {
        let p = LangextractPipeline::new();
        let artifact = make_artifact(
            "doc.txt",
            None,
            ArtifactKind::MeetingNotes,
            SourceSystemKind::DocumentSilo,
        );
        // base 0.80 + 0.10 bonus = 0.90
        assert!(p.can_handle(&artifact).0 >= 0.89);
    }

    #[test]
    fn unknown_type_low_confidence() {
        let p = LangextractPipeline::new();
        let artifact = make_artifact(
            "binary.bin",
            None,
            ArtifactKind::SourceCode,
            SourceSystemKind::GitRepository,
        );
        assert_eq!(p.can_handle(&artifact).0, 0.30);
    }

    #[test]
    fn parse_langextract_json_maps_entities_and_claims() {
        let artifact = make_artifact(
            "doc.txt",
            None,
            ArtifactKind::MeetingNotes,
            SourceSystemKind::GitRepository,
        );
        let json = serde_json::json!({
            "entities": [{"text": "ACME Corp", "type": "ORG"}],
            "claims": [{"text": "ACME Corp is a vendor"}],
            "concepts": [{"label": "procurement"}]
        });
        let bundle = LangextractPipeline::parse_langextract_json(&artifact, &json);
        assert_eq!(bundle.observations.len(), 3);
        assert_eq!(bundle.observations[0].kind, "entity");
        assert_eq!(bundle.observations[1].kind, "claim");
        assert_eq!(bundle.observations[2].kind, "concept");
    }
}
