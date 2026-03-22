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
}
