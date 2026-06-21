//! repo.index MCP tool — ingest text content into NeumannStore
//!
//! Used by b00t grok digest/learn to push content into irontology.
//! Chunks content, embeds via ModelProvider, upserts embeddings + turtle triple.
//!
//! Input:
//!   content: string  — text to index
//!   source:  string  — source identifier (URL, file path, or label)
//!   topic:   string? — optional semantic category label

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::{json, Value};
use std::sync::Arc;

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
                "topic": { "type": "string" },
                "content": {
                    "type": "string",
                    "description": "Text to index. Maximum 524288 bytes (512 KiB)."
                },
                "source": {
                    "type": "string",
                    "description": "URL or file path"
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

        // Validate content size before doing any work
        if content.len() > MAX_CONTENT_BYTES {
            return Err(anyhow!(
                "content exceeds maximum allowed size of {} bytes ({} bytes provided)",
                MAX_CONTENT_BYTES,
                content.len()
            ));
        }

        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // Sanitize source and topic into safe IRIs before embedding in Turtle.
        // Raw user strings may contain spaces, angle brackets, or other characters
        // that are illegal in Turtle <...> IRIs and could allow Turtle injection.
        let source_iri = to_safe_iri(source);
        let topic_iri = format!("http://b00t.io/topic/{}", percent_encode(topic));

        // 1. Chunk the content
        let chunks = chunk_text(content, CHUNK_SIZE, CHUNK_OVERLAP);
        let chunk_count = chunks.len();

        // Validate chunk count
        if chunk_count > MAX_CHUNKS {
            return Err(anyhow!(
                "content exceeds the maximum of {} chunks ({} chunks would be produced)",
                MAX_CHUNKS,
                chunk_count
            ));
        }

        // 2. Embed each chunk + upsert into store
        let mut embeddings = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let id = format!("{}::chunk::{}", source, i);
            let req = EmbedRequest {
                model: self.provider.model_id().to_string(),
                inputs: vec![chunk.clone()],
                batch_size: 1,
            };
            match self.provider.embed(req).await {
                Ok(resp) if !resp.vectors.is_empty() => {
                    embeddings.push(EmbeddingRecord {
                        id,
                        source_blob: source.to_string(),
                        vector: resp.vectors.into_iter().next().expect("vectors is non-empty; checked above"),
                        modality: EmbeddingModality::DocChunk,
                        semantic_weight: 1.0,
                        anchor_id: None,
                        artifact_locator: None,
                    });
                }
                Ok(_) => {
                    eprintln!("⚠️ repo.index: embed chunk {i} returned empty vectors (skipping)");
                }
                Err(e) => {
                    eprintln!("⚠️ repo.index: embed chunk {i} failed: {e} (skipping)");
                }
            }
        }
        let embedded = embeddings.len();
        self.store.upsert_embeddings(embeddings).await?;

        // 3. Store a semantic triple: source --hasTopic--> topic
        let turtle = format!(
            "<{}> <http://b00t.io/ontology#hasTopic> <{}> .",
            source_iri, topic_iri,
        );
        let turtle_ok = match self.store.ingest_turtle(source, &turtle).await {
            Ok(()) => true,
            Err(e) => {
                eprintln!("⚠️ repo.index: ingest_turtle failed: {e}");
                false
            }
        };

        Ok(json!({
            "indexed": embedded > 0 && turtle_ok,
            "source": source,
            "topic": topic,
            "chunks": chunk_count,
            "chunks_created": embedded,
            "embedded": embedded,
        }))
    }
}

/// Convert an arbitrary string to a safe IRI for use in Turtle `<…>`.
///
/// If `s` is already a valid absolute IRI (has a recognised scheme and
/// contains no characters that are illegal inside angle-bracket IRIs),
/// it is returned unchanged.  Otherwise, the value is percent-encoded and
/// placed in a `urn:b00t:resource:` URN to prevent Turtle injection.
fn to_safe_iri(s: &str) -> String {
    let has_safe_scheme = s
        .find("://")
        .map(|end| {
            s[..end]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
        .unwrap_or(false);

    // Characters that are illegal inside a Turtle <...> IRI reference:
    let has_illegal = s
        .chars()
        .any(|c| matches!(c, ' ' | '<' | '>' | '"' | '{' | '}' | '|' | '\\' | '^' | '`'));

    if has_safe_scheme && !has_illegal {
        s.to_string()
    } else {
        format!("urn:b00t:resource:{}", percent_encode(s))
    }
}

/// Percent-encode all bytes that are not unreserved URI characters
/// (RFC 3986 §2.3: ALPHA / DIGIT / "-" / "." / "_" / "~").
fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
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
    use super::{chunk_text, percent_encode, to_safe_iri};

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
    fn safe_iri_passes_valid_url() {
        let url = "https://example.com/doc/foo";
        assert_eq!(to_safe_iri(url), url);
    }

    #[test]
    fn safe_iri_escapes_file_path() {
        let path = "/home/alice/notes/my doc.txt";
        let iri = to_safe_iri(path);
        assert!(iri.starts_with("urn:b00t:resource:"));
        assert!(!iri.contains(' '));
    }

    #[test]
    fn safe_iri_escapes_injection_attempt() {
        // Attempting Turtle injection via angle bracket in source
        let evil = "http://ok.example/> . <http://evil.example/x> <http://evil.example/y> <http://evil.example/z";
        let iri = to_safe_iri(evil);
        assert!(iri.starts_with("urn:b00t:resource:"));
        assert!(!iri.contains('>'));
    }

    #[test]
    fn percent_encode_spaces_and_specials() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a<b>c"), "a%3Cb%3Ec");
    }
}

