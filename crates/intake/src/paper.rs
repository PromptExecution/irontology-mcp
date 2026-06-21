//! Academic paper ingestion — PDF extraction + arXiv/HuggingFace connectors.
//!
//! `PdfExtractor` calls `pdftotext` (poppler-utils) as a subprocess — zero
//! Rust PDF-parsing dependencies; degrades gracefully when pdftotext is absent.
//!
//! `ArXivConnector` fetches arXiv abstract pages and the HTML paper format
//! (ar5iv/html) and returns `Artifact` records for downstream extraction.

use std::process::Command;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use domain::{Anchor, Artifact, ArtifactKind, EvidenceBundle, Observation, SourceSystemKind};

use crate::{ArtifactConnector, EvidenceExtractor, SourceSystem};

// ── PdfExtractor ─────────────────────────────────────────────────────────────

/// Extracts plain text from PDF artifacts using the system `pdftotext` binary.
///
/// Falls back gracefully when `pdftotext` is not installed — returns an error
/// rather than panicking so the pipeline can skip and log.
pub struct PdfExtractor;

impl PdfExtractor {
    pub fn new() -> Self {
        Self
    }

    fn pdftotext_available() -> bool {
        Command::new("pdftotext")
            .arg("-v")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty())
            .unwrap_or(false)
    }
}

#[async_trait]
impl EvidenceExtractor for PdfExtractor {
    fn name(&self) -> &str {
        "pdf_extractor"
    }

    fn supports(&self, artifact: &Artifact) -> bool {
        artifact.media_type.as_deref() == Some("application/pdf")
            || artifact
                .locator
                .to_lowercase()
                .ends_with(".pdf")
    }

    async fn extract(&self, artifact: &Artifact) -> Result<EvidenceBundle> {
        if !Self::pdftotext_available() {
            return Err(anyhow!(
                "pdftotext not found — install poppler-utils: apt install poppler-utils"
            ));
        }

        let output = Command::new("pdftotext")
            .args(["-layout", "-enc", "UTF-8", &artifact.locator, "-"])
            .output()
            .context("pdftotext subprocess failed")?;

        if !output.status.success() {
            return Err(anyhow!(
                "pdftotext exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let text = text.trim().to_owned();

        // Chunk by paragraphs (double newline), max 2000 chars per chunk.
        let observations: Vec<Observation> = chunk_text(&text, 2000)
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| Observation {
                id: format!("{}-pdf-chunk-{i}", artifact.id),
                artifact_id: artifact.id.clone(),
                anchor_id: None,
                kind: "pdf_text_chunk".into(),
                content: chunk,
                attributes: Default::default(),
                confidence: 0.9,
                namespace: Some("paper".into()),
            })
            .collect();

        Ok(EvidenceBundle {
            artifact: artifact.clone(),
            namespaces: vec![],
            anchors: vec![],
            observations,
            claims: vec![],
            concepts: vec![],
            entities: vec![],
            relations: vec![],
        })
    }
}

// ── ArXivConnector ────────────────────────────────────────────────────────────

/// Fetches arXiv paper abstract + HTML body and returns Artifact records.
///
/// Supports two locator formats:
///   - `arxiv:2604.02029`  (preferred — resolved to abs + html URLs)
///   - `https://arxiv.org/abs/2604.02029`  (full URL)
pub struct ArXivConnector;

impl ArXivConnector {
    pub fn new() -> Self {
        Self
    }

    fn arxiv_id_from_locator(locator: &str) -> Option<String> {
        if let Some(id) = locator.strip_prefix("arxiv:") {
            return Some(id.trim().to_owned());
        }
        // https://arxiv.org/abs/2604.02029 or /pdf/2604.02029
        let segments: Vec<&str> = locator.rsplitn(2, '/').collect();
        if locator.contains("arxiv.org") && segments.len() >= 1 {
            let id = segments[0].trim_end_matches(".pdf");
            if !id.is_empty() {
                return Some(id.to_owned());
            }
        }
        None
    }
}

#[async_trait]
impl ArtifactConnector for ArXivConnector {
    fn name(&self) -> &str {
        "arxiv_connector"
    }

    fn supports(&self, source: &SourceSystem) -> bool {
        matches!(source.kind, SourceSystemKind::ArXiv)
    }

    async fn list_artifacts(&self, source: &SourceSystem) -> Result<Vec<Artifact>> {
        // source.locator = "arxiv:<id>" or arxiv URL
        let id = Self::arxiv_id_from_locator(&source.locator)
            .ok_or_else(|| anyhow!("ArXivConnector: cannot parse arxiv id from '{}'", source.locator))?;

        // Artifact 1: abstract page (text/html, always available)
        let abs_artifact = Artifact {
            id: format!("arxiv-abs-{id}"),
            source_id: source.id.clone(),
            source_kind: SourceSystemKind::ArXiv,
            kind: ArtifactKind::AcademicPaper,
            title: None,
            locator: format!("https://arxiv.org/abs/{id}"),
            media_type: Some("text/html".into()),
            tags: [("arxiv_id".into(), id.clone())].into_iter().collect(),
            valid_at: None,
            observed_at: None,
        };

        // Artifact 2: HTML full paper via ar5iv (LaTeX→HTML, best-effort)
        let html_artifact = Artifact {
            id: format!("arxiv-html-{id}"),
            source_id: source.id.clone(),
            source_kind: SourceSystemKind::ArXiv,
            kind: ArtifactKind::AcademicPaper,
            title: None,
            locator: format!("https://ar5iv.org/html/{id}"),
            media_type: Some("text/html".into()),
            tags: [
                ("arxiv_id".into(), id.clone()),
                ("format".into(), "html_paper".into()),
            ]
            .into_iter()
            .collect(),
            valid_at: None,
            observed_at: None,
        };

        Ok(vec![abs_artifact, html_artifact])
    }
}

// ── HuggingFacePapersConnector ────────────────────────────────────────────────

/// Fetches paper markdown from HuggingFace Papers (papers/<id>.md endpoint).
pub struct HuggingFacePapersConnector;

impl HuggingFacePapersConnector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ArtifactConnector for HuggingFacePapersConnector {
    fn name(&self) -> &str {
        "huggingface_papers_connector"
    }

    fn supports(&self, source: &SourceSystem) -> bool {
        matches!(source.kind, SourceSystemKind::HuggingFacePapers)
    }

    async fn list_artifacts(&self, source: &SourceSystem) -> Result<Vec<Artifact>> {
        // source.locator = arxiv id, e.g. "2604.02029"
        let id = source.locator.trim();
        let artifact = Artifact {
            id: format!("hf-paper-{id}"),
            source_id: source.id.clone(),
            source_kind: SourceSystemKind::HuggingFacePapers,
            kind: ArtifactKind::AcademicPaper,
            title: None,
            locator: format!("https://huggingface.co/papers/{id}.md"),
            media_type: Some("text/markdown".into()),
            tags: [("arxiv_id".into(), id.to_owned())].into_iter().collect(),
            valid_at: None,
            observed_at: None,
        };
        Ok(vec![artifact])
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut buf = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if buf.len() + para.len() + 2 > max_chars && !buf.is_empty() {
            chunks.push(std::mem::take(&mut buf));
        }
        if !buf.is_empty() {
            buf.push_str("\n\n");
        }
        buf.push_str(para);
    }
    if !buf.is_empty() {
        chunks.push(buf);
    }
    chunks
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arxiv_id_parsing() {
        assert_eq!(
            ArXivConnector::arxiv_id_from_locator("arxiv:2604.02029"),
            Some("2604.02029".into())
        );
        assert_eq!(
            ArXivConnector::arxiv_id_from_locator("https://arxiv.org/abs/2604.02029"),
            Some("2604.02029".into())
        );
        assert_eq!(
            ArXivConnector::arxiv_id_from_locator("https://arxiv.org/pdf/2604.02029.pdf"),
            Some("2604.02029".into())
        );
        assert_eq!(
            ArXivConnector::arxiv_id_from_locator("not-an-arxiv-url"),
            None
        );
    }

    #[test]
    fn chunk_text_basic() {
        let text = "para one\n\npara two\n\npara three";
        let chunks = chunk_text(text, 20);
        assert!(chunks.len() >= 2, "should split when para exceeds max_chars");
        let all: String = chunks.join("\n\n");
        assert!(all.contains("para one"));
        assert!(all.contains("para three"));
    }

    #[test]
    fn pdf_extractor_supports() {
        let ex = PdfExtractor::new();
        let mut art = Artifact {
            id: "a1".into(),
            source_id: "s1".into(),
            source_kind: SourceSystemKind::DocumentSilo,
            kind: ArtifactKind::AcademicPaper,
            title: None,
            locator: "/tmp/paper.pdf".into(),
            media_type: None,
            tags: Default::default(),
            valid_at: None,
            observed_at: None,
        };
        // locator ends with .pdf → supported
        assert!(ex.supports(&art));
        art.locator = "/tmp/notes.txt".into();
        art.media_type = Some("application/pdf".into());
        // media_type matches → supported
        assert!(ex.supports(&art));
        art.media_type = None;
        assert!(!ex.supports(&art));
    }

    #[tokio::test]
    async fn arxiv_connector_lists_two_artifacts() {
        let conn = ArXivConnector::new();
        let source = SourceSystem {
            id: "test-src".into(),
            kind: SourceSystemKind::ArXiv,
            locator: "arxiv:2604.02029".into(),
        };
        let arts = conn.list_artifacts(&source).await.unwrap();
        assert_eq!(arts.len(), 2);
        assert!(arts[0].locator.contains("arxiv.org/abs"));
        assert!(arts[1].locator.contains("ar5iv.org/html"));
    }

    #[tokio::test]
    async fn hf_papers_connector_lists_one_artifact() {
        let conn = HuggingFacePapersConnector::new();
        let source = SourceSystem {
            id: "test-hf".into(),
            kind: SourceSystemKind::HuggingFacePapers,
            locator: "2604.02029".into(),
        };
        let arts = conn.list_artifacts(&source).await.unwrap();
        assert_eq!(arts.len(), 1);
        assert!(arts[0].locator.contains("huggingface.co/papers"));
    }
}
