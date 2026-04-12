use regex::Regex;

/// A chunk produced by the structure-aware chunker.
#[derive(Debug, Clone)]
pub struct StructuredChunk {
    pub text: String,
    pub section_id: Option<String>,
    pub heading: Option<String>,
    pub locator: String,
    pub chunk_index: usize,
}

/// Slugify a heading string for use in a locator.
fn slugify(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Split text into char-bounded pieces no larger than `max_chars`, preferring
/// paragraph breaks (double newline) and then hard-cutting only when necessary.
fn split_at_paragraphs(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for para in paragraphs {
        let para_chars = para.chars().count();
        let current_chars = current.chars().count();
        if current_chars == 0 {
            if para_chars <= max_chars {
                current.push_str(para);
            } else {
                // Para itself is too long — hard-split it
                for piece in chunk_text(para, max_chars) {
                    out.push(piece);
                }
            }
            continue;
        }
        let sep_chars = 2; // "\n\n"
        if current_chars + sep_chars + para_chars <= max_chars {
            current.push_str("\n\n");
            current.push_str(para);
        } else {
            out.push(current.clone());
            current.clear();
            if para_chars <= max_chars {
                current.push_str(para);
            } else {
                for piece in chunk_text(para, max_chars) {
                    out.push(piece);
                }
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Structure-aware chunker for regulatory / legal documents.
///
/// Detection priority:
/// 1. Numbered section headers  `^\s*(\d+(\.\d+)*)\s+([A-Z][^\n]{3,60})\s*$`
/// 2. Markdown headings          `^#{1,4}\s+(.+)$`
/// 3. ALL-CAPS standalone lines  (≥ 4 chars)
/// 4. Fallback: paragraph / char splits
pub fn chunk_structured(text: &str, max_chars: usize) -> Vec<StructuredChunk> {
    let re_numbered =
        Regex::new(r"(?m)^\s*(\d+(\.\d+)*)\s+([A-Z][^\n]{3,60})\s*$").unwrap();
    let re_markdown = Regex::new(r"(?m)^#{1,4}\s+(.+)$").unwrap();
    let re_allcaps = Regex::new(r"(?m)^([A-Z][A-Z\s]{3,}[A-Z])\s*$").unwrap();

    struct Section {
        section_id: Option<String>,
        heading: Option<String>,
        locator: String,
        body: String,
    }

    // Walk lines and identify section boundaries.
    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;

    for line in &lines {
        if let Some(cap) = re_numbered.captures(line) {
            // Flush current section
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            let id = cap[1].to_string();
            let heading = cap[3].trim().to_string();
            let locator = format!("§{id}");
            current = Some(Section {
                section_id: Some(id),
                heading: Some(heading),
                locator,
                body: String::new(),
            });
        } else if let Some(cap) = re_markdown.captures(line) {
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            let heading = cap[1].trim().to_string();
            let locator = format!("§{}", slugify(&heading));
            current = Some(Section {
                section_id: None,
                heading: Some(heading),
                locator,
                body: String::new(),
            });
        } else if let Some(cap) = re_allcaps.captures(line) {
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            let heading = cap[1].trim().to_string();
            let locator = format!("§{}", slugify(&heading));
            current = Some(Section {
                section_id: None,
                heading: Some(heading.clone()),
                locator,
                body: String::new(),
            });
        } else {
            if let Some(ref mut sec) = current {
                if !sec.body.is_empty() {
                    sec.body.push('\n');
                }
                sec.body.push_str(line);
            } else {
                // Text before any heading — create an implicit preamble section.
                current = Some(Section {
                    section_id: None,
                    heading: None,
                    locator: format!("p0"),
                    body: line.to_string(),
                });
            }
        }
    }
    if let Some(sec) = current.take() {
        sections.push(sec);
    }

    // If we found no section structure, fall back to pure paragraph/char split.
    if sections.iter().all(|s| s.section_id.is_none() && s.heading.is_none()) {
        let pieces = split_at_paragraphs(text, max_chars);
        return pieces
            .into_iter()
            .enumerate()
            .map(|(i, text)| StructuredChunk {
                text,
                section_id: None,
                heading: None,
                locator: format!("chunk-{i}"),
                chunk_index: i,
            })
            .collect();
    }

    // Emit chunks, splitting large sections by paragraph boundary.
    let mut out: Vec<StructuredChunk> = Vec::new();
    let mut chunk_index = 0usize;
    for sec in sections {
        let body = sec.body.trim().to_string();
        if body.is_empty() {
            continue;
        }
        let pieces = split_at_paragraphs(&body, max_chars);
        let num_pieces = pieces.len();
        for (sub, piece) in pieces.into_iter().enumerate() {
            let locator = if num_pieces > 1 {
                format!("{}/{}", sec.locator, sub)
            } else {
                sec.locator.clone()
            };
            out.push(StructuredChunk {
                text: piece,
                section_id: sec.section_id.clone(),
                heading: sec.heading.clone(),
                locator,
                chunk_index,
            });
            chunk_index += 1;
        }
    }
    out
}

/// Original character-count splitter — unchanged.
///
/// Splits `text` into chunks of at most `max_chars` Unicode scalar values.
/// Chunk boundaries always fall on codepoint boundaries (never mid-codepoint).
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut char_iter = text.char_indices().peekable();
    loop {
        let start = match char_iter.peek() {
            Some(&(i, _)) => i,
            None => break,
        };
        let mut end = start;
        let mut count = 0;
        while count < max_chars {
            match char_iter.next() {
                Some((i, c)) => {
                    end = i + c.len_utf8();
                    count += 1;
                }
                None => break,
            }
        }
        out.push(text[start..end].to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── existing tests (chunk_text) ──────────────────────────────────────────

    #[test]
    fn empty_input_returns_empty() {
        assert!(chunk_text("", 512).is_empty());
    }

    #[test]
    fn ascii_chunks_evenly() {
        let text = "abcdefgh";
        let chunks = chunk_text(text, 3);
        assert_eq!(chunks, vec!["abc", "def", "gh"]);
    }

    #[test]
    fn multibyte_utf8_does_not_panic() {
        // Each '中' is 3 bytes; chunk at 2 chars must not split a codepoint
        let text = "中文内容测试";
        let chunks = chunk_text(text, 2);
        assert_eq!(chunks, vec!["中文", "内容", "测试"]);
    }

    #[test]
    fn multibyte_utf8_boundary_safe() {
        // 512-byte window that would fall mid-codepoint with the old byte-offset logic
        let segment = "é"; // 2 bytes each
        let text: String = segment.repeat(300); // 600 bytes, 300 chars
        let chunks = chunk_text(&text, 512);
        // All chunks must be valid UTF-8 (this will panic on invalid slices)
        for chunk in &chunks {
            assert!(std::str::from_utf8(chunk.as_bytes()).is_ok());
            // Every character in the chunk must be the expected 'é' — no corruption
            assert!(chunk.chars().all(|c| c == 'é'));
        }
        // 300 chars fit in one chunk (300 < 512)
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn emoji_chunks_by_char_not_byte() {
        // Each emoji is 4 bytes; old code would split mid-codepoint at byte 512
        let text: String = "🦀".repeat(200); // 800 bytes, 200 chars
        let chunks = chunk_text(&text, 100);
        assert_eq!(chunks.len(), 2);
        for chunk in &chunks {
            assert_eq!(chunk.chars().count(), 100);
        }
    }

    // ── new tests (chunk_structured) ────────────────────────────────────────

    #[test]
    fn structured_chunker_detects_numbered_sections() {
        let text = "5.3.1 Connection Agreements\n\nAll participants must enter a connection agreement.";
        let chunks = chunk_structured(text, 512);
        assert!(!chunks.is_empty(), "expected at least one chunk");
        let c = &chunks[0];
        assert_eq!(c.section_id.as_deref(), Some("5.3.1"));
        assert_eq!(c.locator, "§5.3.1");
        // heading captures the title part
        assert!(c.heading.as_deref().unwrap_or("").contains("Connection"));
    }

    #[test]
    fn structured_chunker_detects_markdown_headings() {
        let text = "## Network Access\n\nProvisions for network access are governed by this section.";
        let chunks = chunk_structured(text, 512);
        assert!(!chunks.is_empty());
        let c = &chunks[0];
        assert_eq!(c.heading.as_deref(), Some("Network Access"));
        assert!(c.locator.starts_with('§'));
    }

    #[test]
    fn structured_chunker_falls_back_to_char_split_for_long_sections() {
        // Build a section whose body is longer than max_chars
        let long_body = "word ".repeat(300); // 1500 chars
        let text = format!("5.1 Long Section\n\n{long_body}");
        let max = 512;
        let chunks = chunk_structured(&text, max);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(
                c.text.chars().count() <= (max as f64 * 1.5) as usize,
                "chunk exceeds 1.5x max_chars: {} chars",
                c.text.chars().count()
            );
        }
    }

    #[test]
    fn chunk_text_unchanged() {
        // Verify existing chunk_text behaviour is unaffected.
        assert!(chunk_text("", 512).is_empty());
        let chunks = chunk_text("abcdefgh", 3);
        assert_eq!(chunks, vec!["abc", "def", "gh"]);
    }

    #[test]
    fn structured_chunker_inherits_section_to_subchunks() {
        // Each sub-chunk within a section should carry the section_id.
        let para = "word ".repeat(60); // ~300 chars per paragraph
        let text = format!(
            "3.2 Market Obligations\n\n{para}\n\n{para}\n\n{para}",
            para = para
        );
        let max = 350;
        let chunks = chunk_structured(&text, max);
        for c in &chunks {
            assert_eq!(c.section_id.as_deref(), Some("3.2"));
        }
    }
}
