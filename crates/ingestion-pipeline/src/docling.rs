use std::{
    collections::BTreeMap,
    process::Command,
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use domain::{Anchor, Artifact, EvidenceBundle, Observation};
use serde_json::Value;

use crate::{ExternalIngestionPipeline, PipelineConfidence};

pub struct DoclingPipeline {
    /// Path to the `docling` CLI binary, or "docling" if on PATH.
    binary: String,
    /// Optional HTTP endpoint if running as a service.
    endpoint: Option<String>,
}

impl Default for DoclingPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl DoclingPipeline {
    pub fn new() -> Self {
        Self {
            binary: "docling".to_string(),
            endpoint: None,
        }
    }

    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            binary: "docling".to_string(),
            endpoint: Some(endpoint.into()),
        }
    }

    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            endpoint: None,
        }
    }

    fn extension_confidence(locator: &str) -> f32 {
        let lower = locator.to_lowercase();
        if lower.ends_with(".pdf") {
            0.95
        } else if lower.ends_with(".docx")
            || lower.ends_with(".pptx")
            || lower.ends_with(".xlsx")
        {
            0.90
        } else if lower.ends_with(".html") || lower.ends_with(".md") {
            0.60
        } else {
            0.0
        }
    }

    fn media_type_confidence(media_type: &str) -> f32 {
        match media_type {
            "application/pdf" => 0.95,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/msword"
            | "application/vnd.ms-powerpoint"
            | "application/vnd.ms-excel" => 0.90,
            "text/html" | "text/markdown" => 0.60,
            _ => 0.0,
        }
    }

    fn parse_docling_json(artifact: &Artifact, json: &Value) -> EvidenceBundle {
        let artifact_id = artifact.id.clone();
        let mut anchors = Vec::new();
        let mut observations = Vec::new();

        // Parse sections → Anchors
        if let Some(sections) = json.get("sections").and_then(|v| v.as_array()) {
            for (i, section) in sections.iter().enumerate() {
                let section_number = section
                    .get("number")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| i.to_string());
                let section_title = section
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let anchor_id = format!("anchor:docling:{}:section:{}", artifact_id, section_number);
                anchors.push(Anchor {
                    id: anchor_id,
                    artifact_id: artifact_id.clone(),
                    kind: "section".to_string(),
                    locator: section_number,
                    label: section_title,
                });
            }
        }

        // Parse text_blocks → Observations
        if let Some(text_blocks) = json.get("text_blocks").and_then(|v| v.as_array()) {
            for (i, block) in text_blocks.iter().enumerate() {
                let content = match block.get("text").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => t.to_string(),
                    _ => continue,
                };
                let section_ref = block
                    .get("section_ref")
                    .and_then(|v| v.as_str())
                    .map(|s| format!("anchor:docling:{}:section:{}", artifact_id, s));
                let obs_id = format!("obs:docling:{}:text_block:{}", artifact_id, i);
                observations.push(Observation {
                    id: obs_id,
                    artifact_id: artifact_id.clone(),
                    anchor_id: section_ref,
                    kind: "text_block".to_string(),
                    content,
                    attributes: BTreeMap::new(),
                    confidence: 0.9,
                    namespace: None,
                });
            }
        }

        // Also handle pages → text_blocks if present
        if let Some(pages) = json.get("pages").and_then(|v| v.as_array()) {
            for page in pages {
                if let Some(blocks) = page.get("text_blocks").and_then(|v| v.as_array()) {
                    let page_num = page
                        .get("page_number")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    for (i, block) in blocks.iter().enumerate() {
                        let content = match block.get("text").and_then(|v| v.as_str()) {
                            Some(t) if !t.is_empty() => t.to_string(),
                            _ => continue,
                        };
                        let section_ref = block
                            .get("section_ref")
                            .and_then(|v| v.as_str())
                            .map(|s| format!("anchor:docling:{}:section:{}", artifact_id, s));
                        let obs_id = format!(
                            "obs:docling:{}:page:{}:text_block:{}",
                            artifact_id, page_num, i
                        );
                        // Avoid duplicates if text_blocks also present at top level
                        if !observations.iter().any(|o: &Observation| o.content == content) {
                            observations.push(Observation {
                                id: obs_id,
                                artifact_id: artifact_id.clone(),
                                anchor_id: section_ref,
                                kind: "text_block".to_string(),
                                content,
                                attributes: BTreeMap::new(),
                                confidence: 0.9,
                                namespace: None,
                            });
                        }
                    }
                }
            }
        }

        EvidenceBundle {
            artifact: artifact.clone(),
            namespaces: Vec::new(),
            anchors,
            observations,
            claims: Vec::new(),
            concepts: Vec::new(),
            entities: Vec::new(),
            relations: Vec::new(),
        }
    }
}

#[async_trait]
impl ExternalIngestionPipeline for DoclingPipeline {
    fn name(&self) -> &str {
        "docling"
    }

    fn can_handle(&self, artifact: &Artifact) -> PipelineConfidence {
        // Check media_type first, then fall back to file extension in locator
        let media_confidence = artifact
            .media_type
            .as_deref()
            .map(Self::media_type_confidence)
            .unwrap_or(0.0);
        let ext_confidence = Self::extension_confidence(&artifact.locator);
        PipelineConfidence(media_confidence.max(ext_confidence))
    }

    async fn extract(&self, artifact: &Artifact) -> anyhow::Result<EvidenceBundle> {
        // If HTTP endpoint configured, delegate there (stub: not fully implemented)
        if let Some(ref _endpoint) = self.endpoint {
            return Err(anyhow!("docling HTTP endpoint mode not yet implemented"));
        }

        // Write artifact locator path content to a temp file and run docling
        let tmp = tempfile_for_artifact(artifact)?;
        let tmp_path = tmp.path().to_string_lossy().to_string();

        let output = Command::new(&self.binary)
            .args(["convert", "--output-format", "json", &tmp_path])
            .output()
            .with_context(|| format!("failed to run docling binary '{}'", self.binary))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "docling exited with status {}: {}",
                output.status,
                stderr
            ));
        }

        let stdout = String::from_utf8(output.stdout)
            .context("docling output was not valid UTF-8")?;
        let json: Value = serde_json::from_str(&stdout)
            .context("failed to parse docling JSON output")?;

        Ok(Self::parse_docling_json(artifact, &json))
    }

    async fn is_available(&self) -> bool {
        Command::new("which")
            .arg(&self.binary)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Materialize the artifact into a temp file for `docling`.
/// Currently this pipeline supports artifacts whose locator is a readable file path.
/// We copy the file into a temp file so the downstream CLI always receives real bytes.
fn tempfile_for_artifact(artifact: &Artifact) -> anyhow::Result<tempfile::NamedTempFile> {
    use std::io::Write;

    let source_path = std::path::Path::new(&artifact.locator);
    if !source_path.is_file() {
        return Err(anyhow!(
            "artifact locator '{}' is not a readable file path; docling extraction requires a real document file",
            artifact.locator
        ));
    }

    let suffix = extension_from_locator(&artifact.locator);
    let mut tmp = tempfile::Builder::new()
        .suffix(&suffix)
        .tempfile()
        .context("failed to create temp file")?;

    let mut source = std::fs::File::open(source_path).with_context(|| {
        format!(
            "failed to open artifact source file '{}'",
            source_path.display()
        )
    })?;

    std::io::copy(&mut source, &mut tmp).with_context(|| {
        format!(
            "failed to copy artifact source file '{}' into temp file",
            source_path.display()
        )
    })?;
    tmp.flush().context("failed to flush temp file")?;
    Ok(tmp)
}

fn extension_from_locator(locator: &str) -> String {
    std::path::Path::new(locator)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::{Artifact, ArtifactKind, SourceSystemKind};

    use super::DoclingPipeline;
    use crate::ExternalIngestionPipeline;

    fn make_artifact(locator: &str, media_type: Option<&str>) -> Artifact {
        Artifact {
            id: format!("artifact:test:{}", locator),
            source_id: "test-source".to_string(),
            source_kind: SourceSystemKind::DocumentSilo,
            kind: ArtifactKind::ArchitectureDocument,
            title: None,
            locator: locator.to_string(),
            media_type: media_type.map(String::from),
            tags: BTreeMap::new(),
            valid_at: None,
            observed_at: None,
        }
    }

    #[test]
    fn pdf_confidence_by_extension() {
        let pipeline = DoclingPipeline::new();
        let artifact = make_artifact("report.pdf", None);
        assert!(pipeline.can_handle(&artifact).0 >= 0.9);
    }

    #[test]
    fn pdf_confidence_by_media_type() {
        let pipeline = DoclingPipeline::new();
        let artifact = make_artifact("file", Some("application/pdf"));
        assert!(pipeline.can_handle(&artifact).0 >= 0.9);
    }

    #[test]
    fn docx_confidence() {
        let pipeline = DoclingPipeline::new();
        let artifact = make_artifact("plan.docx", None);
        assert!(pipeline.can_handle(&artifact).0 >= 0.85);
    }

    #[test]
    fn html_confidence() {
        let pipeline = DoclingPipeline::new();
        let artifact = make_artifact("page.html", None);
        let c = pipeline.can_handle(&artifact).0;
        assert!(c >= 0.5 && c < 0.9);
    }

    #[test]
    fn unknown_extension_zero() {
        let pipeline = DoclingPipeline::new();
        let artifact = make_artifact("data.csv", None);
        assert_eq!(pipeline.can_handle(&artifact).0, 0.0);
    }

    #[test]
    fn parse_docling_json_extracts_text_blocks() {
        let artifact = make_artifact("doc.pdf", Some("application/pdf"));
        let json = serde_json::json!({
            "sections": [
                { "number": "1", "title": "Introduction" }
            ],
            "text_blocks": [
                { "text": "Hello world", "section_ref": "1" },
                { "text": "Second block" }
            ]
        });
        let bundle = DoclingPipeline::parse_docling_json(&artifact, &json);
        assert_eq!(bundle.anchors.len(), 1);
        assert_eq!(bundle.observations.len(), 2);
        assert_eq!(bundle.observations[0].content, "Hello world");
        assert_eq!(bundle.observations[0].kind, "text_block");
        assert!(bundle.observations[0].anchor_id.is_some());
    }
}
