//! repo.index MCP tool — ingest text content into NeumannStore
//!
//! Used by b00t grok digest/learn to push content into irontology.
//! Chunks content, embeds via ModelProvider, upserts embeddings + turtle triple.
//!
//! Input:
//!   content: string  — text to index
//!   source:  string? — optional source identifier (URL, file path, or label)
//!   topic:   string? — optional semantic category label

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use provider_api::{EmbedRequest, ModelProvider};
use storage_neumann::{
    EmbeddingModality, EmbeddingRecord, KnowledgeStore,
};

use crate::Tool;

const CHUNK_SIZE: usize = 512;
const CHUNK_OVERLAP: usize = 64;

/// Maximum allowed content size in bytes (512 KiB).
pub const MAX_CONTENT_BYTES: usize = 512 * 1024;
/// Maximum number of chunks produced from a single ingestion call.
pub const MAX_CHUNKS: usize = 256;

pub struct RepoIndexTool {
    store: Arc<dyn KnowledgeStore>,
    provider: Arc<dyn ModelProvider>,
}

impl RepoIndexTool {
    pub fn new(store: Arc<dyn KnowledgeStore>, provider: Arc<dyn ModelProvider>) -> Self {
        Self { store, provider }
    }
}

#[async_trait]
impl Tool for RepoIndexTool {
    fn name(&self) -> &str {
        "repo.index"
    }

    fn description(&self) -> &str {
        "Index content into the knowledge store (chunk, embed, upsert). \
         Content must not exceed 512 KiB and must produce no more than 256 chunks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Semantic category label"
                },
                "content": {
                    "type": "string",
                    "description": "Text to index. Maximum 524288 bytes (512 KiB)."
                },
                "source": {
                    "type": "string",
                    "description": "URL or file path (optional). Used as the RDF subject IRI."
                }
            },
            "required": ["topic", "content"]
        })
    }

    async fn call(&self, params: Value) -> Result<Value> {
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("content missing"))?;

        // Validate content size before doing any work.
        if content.len() > MAX_CONTENT_BYTES {
            return Err(anyhow!(
                "content exceeds maximum allowed size ({} bytes, limit is {})",
                content.len(),
                MAX_CONTENT_BYTES
            ));
        }

        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // 1. Chunk the content
        let chunks = chunk_text(content, CHUNK_SIZE, CHUNK_OVERLAP);
        let chunk_count = chunks.len();

        if chunk_count > MAX_CHUNKS {
            return Err(anyhow!(
                "content produces {} chunks which exceeds the maximum allowed ({})",
                chunk_count,
                MAX_CHUNKS
            ));
        }

        // 2. Embed each chunk + upsert into NeumannStore
        let embed_req = EmbedRequest {
            model: self.provider.model_id().to_string(),
            inputs: chunks.clone(),
            batch_size: chunks.len().max(1),
        };
        let mut embeddings = Vec::new();
        match self.provider.embed(embed_req).await {
            Ok(resp) => {
                for (i, vector) in resp.vectors.into_iter().enumerate() {
                    let id = format!("{}::chunk::{}", source, i);
                    embeddings.push(EmbeddingRecord {
                        id,
                        source_blob: source.to_string(),
                        vector,
                        modality: EmbeddingModality::DocChunk,
                        semantic_weight: 1.0,
                    });
                }
            }
            Err(e) => {
                eprintln!("⚠️ repo.index: embed failed: {e} (skipping embeddings)");
            }
        }
        let embedded = embeddings.len();
        self.store.upsert_embeddings(embeddings).await?;

        // 3. Store a semantic triple: source --hasTopic--> topic
        //    source and topic are normalized to safe IRIs before interpolation.
        let source_iri = normalize_to_iri(source);
        let topic_iri = format!(
            "http://b00t.io/topic/{}",
            iri_encode_path_segment(topic)
        );
        let turtle = format!(
            "<{}> <http://b00t.io/ontology#hasTopic> <{}> .",
            source_iri, topic_iri
        );
        self.store.ingest_turtle(&source_iri, &turtle).await?;

        Ok(json!({
            "indexed": true,
            "source": source,
            "topic": topic,
            "chunks_created": chunk_count,
            "embedded": embedded,
        }))
    }
}

/// Normalize an arbitrary string into a safe absolute IRI suitable for use
/// inside `<...>` in Turtle syntax.
///
/// - If `s` is already an absolute IRI (starts with a known scheme like
///   `http://`, `https://`, `urn:`, `file:`, etc.) **and** contains no
///   characters that are illegal inside `<...>` delimiters, it is returned
///   as-is after percent-encoding any remaining unsafe characters.
/// - Otherwise the string is encoded as `urn:source:{percent-encoded}`.
fn normalize_to_iri(s: &str) -> String {
    if s.is_empty() {
        return "urn:source:unknown".to_string();
    }
    let looks_absolute = s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("ftp://")
        || s.starts_with("urn:")
        || s.starts_with("file:");

    if looks_absolute {
        // Percent-encode only the characters that are explicitly forbidden
        // inside IRI angle brackets by the Turtle / N-Triples grammar and
        // RFC 3987: space, <, >, ", {, }, |, \, ^, `
        iri_encode(s)
    } else {
        format!("urn:source:{}", percent_encode_unrestricted(s))
    }
}

/// Percent-encode characters that are forbidden inside `<...>` IRI references
/// in Turtle syntax (space, <, >, ", {, }, |, \, ^, `).
fn iri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '"' => out.push_str("%22"),
            '{' => out.push_str("%7B"),
            '}' => out.push_str("%7D"),
            '|' => out.push_str("%7C"),
            '\\' => out.push_str("%5C"),
            '^' => out.push_str("%5E"),
            '`' => out.push_str("%60"),
            other => out.push(other),
        }
    }
    out
}

/// Percent-encode a string so it can be used as part of a URN or IRI path
/// segment: encodes everything that is not an unreserved URI character
/// (letters, digits, `-`, `.`, `_`, `~`) or `/` for path separators.
fn percent_encode_unrestricted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            other => {
                out.push('%');
                out.push(hex_digit(other >> 4));
                out.push(hex_digit(other & 0xF));
            }
        }
    }
    out
}

/// Encode a topic label for safe use as an IRI path segment.
/// Spaces become `_`; characters illegal in IRI path segments are percent-encoded.
fn iri_encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == ' ' {
            out.push('_');
        } else {
            // Encode characters that would break Turtle IRI syntax
            match c {
                '<' => out.push_str("%3C"),
                '>' => out.push_str("%3E"),
                '"' => out.push_str("%22"),
                '{' => out.push_str("%7B"),
                '}' => out.push_str("%7D"),
                '|' => out.push_str("%7C"),
                '\\' => out.push_str("%5C"),
                '^' => out.push_str("%5E"),
                '`' => out.push_str("%60"),
                '#' => out.push_str("%23"),
                other => out.push(other),
            }
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => '0',
    }
}

/// Simple fixed-size text chunker with overlap
fn chunk_text(text: &str, size: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || size == 0 {
        return vec![];
    }
    let step = if size > overlap { size - overlap } else { 1 };
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + size).min(chars.len());
        chunks.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start += step;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{chunk_text, iri_encode_path_segment, normalize_to_iri};

    #[test]
    fn chunk_basic() {
        let text = "hello world foo bar baz";
        let chunks = chunk_text(text, 10, 2);
        assert!(!chunks.is_empty());
        assert!(chunks[0].len() <= 10);
    }

    #[test]
    fn chunk_empty() {
        assert!(chunk_text("", 10, 2).is_empty());
    }

    #[test]
    fn normalize_http_url_is_unchanged() {
        let url = "https://example.com/auth";
        assert_eq!(normalize_to_iri(url), url);
    }

    #[test]
    fn normalize_url_with_spaces_encodes_them() {
        let url = "https://example.com/some path";
        let result = normalize_to_iri(url);
        assert!(!result.contains(' '), "spaces must be encoded: {result}");
        assert!(result.contains("%20"), "spaces must become %20: {result}");
    }

    #[test]
    fn normalize_file_path_becomes_urn() {
        let path = "/home/user/file.txt";
        let result = normalize_to_iri(path);
        assert!(
            result.starts_with("urn:source:"),
            "file path should become urn:source:..., got: {result}"
        );
        assert!(!result.contains('<'), "must not contain <");
        assert!(!result.contains('>'), "must not contain >");
    }

    #[test]
    fn normalize_label_becomes_urn() {
        let label = "my topic label";
        let result = normalize_to_iri(label);
        assert!(result.starts_with("urn:source:"), "got: {result}");
        assert!(!result.contains(' '), "spaces must be encoded: {result}");
    }

    #[test]
    fn normalize_injection_attempt() {
        // A crafted source that would break Turtle syntax if not encoded
        let evil = "http://evil.com/> . <http://evil.com/b> <http://evil.com/c> <http://evil.com/d>";
        let result = normalize_to_iri(evil);
        assert!(!result.contains('>'), "must encode >: {result}");
    }

    #[test]
    fn iri_encode_path_segment_spaces_become_underscores() {
        assert_eq!(iri_encode_path_segment("auth risks"), "auth_risks");
    }

    #[test]
    fn iri_encode_path_segment_no_injection() {
        let topic = "a<b>c";
        let result = iri_encode_path_segment(topic);
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
    }
}
