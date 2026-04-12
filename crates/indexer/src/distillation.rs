use anyhow::Result;
use provider_api::{ChatMessage, ChatRequest, ModelProvider};

/// Distil each chunk into a 1-2 sentence summary using the provider's chat
/// completion endpoint.
///
/// For each chunk the first 200 characters (or 3 sentences, whichever is
/// shorter) are sent as context to keep prompt tokens low.  The prompt asks
/// the model to be specific about entities, obligations, and defined terms —
/// important for regulatory / legal corpora such as the Australian NEL or
/// AEMO schemas.
///
/// If the provider's `chat()` call returns an error for a chunk the function
/// stores an empty string for that chunk rather than propagating the error, so
/// a single LLM hiccup does not abort the whole ingestion pipeline.
///
/// Returns a `Vec<String>` that is **parallel** to `chunks`: one summary per
/// input chunk.
pub async fn distill_chunks(
    chunks: &[String],
    provider: &dyn ModelProvider,
) -> Result<Vec<String>> {
    let mut summaries = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        // Extract a short excerpt (first 200 chars or 3 sentences).
        let excerpt = excerpt(chunk, 200);
        let prompt = format!(
            "Summarise in 1-2 sentences what this passage is about. \
Be specific about entities, obligations, and defined terms.\n\nPassage:\n{excerpt}"
        );
        let req = ChatRequest {
            model: provider.model_id().to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            max_tokens: Some(128),
            stream: false,
            params: Default::default(),
        };
        let summary = match provider.chat(req).await {
            Ok(resp) => resp.content.trim().to_string(),
            Err(_) => String::new(),
        };
        summaries.push(summary);
    }
    Ok(summaries)
}

/// Return the first `max_chars` characters of `text`, clipped at sentence
/// boundaries where possible (up to 3 sentences).
fn excerpt(text: &str, max_chars: usize) -> String {
    // Collect up to 3 sentence-ending positions.
    let mut sentence_ends: Vec<usize> = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?') {
            // Check that the next char is whitespace or end-of-string.
            if chars
                .peek()
                .map(|(_, nc)| nc.is_whitespace())
                .unwrap_or(true)
            {
                sentence_ends.push(i + c.len_utf8());
                if sentence_ends.len() == 3 {
                    break;
                }
            }
        }
    }
    // Use the latest sentence boundary that still fits within max_chars,
    // otherwise fall back to max_chars character truncation.
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    // Find the last sentence end whose prefix fits within max_chars.
    let best_end = sentence_ends
        .iter()
        .rev()
        .find(|&&end| text[..end].chars().count() <= max_chars)
        .copied();
    if let Some(end) = best_end {
        return text[..end].to_string();
    }
    // Hard truncation at max_chars.
    text.char_indices()
        .nth(max_chars)
        .map(|(i, _)| text[..i].to_string())
        .unwrap_or_else(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_returns_full_text_when_short() {
        let s = "Hello world.";
        assert_eq!(excerpt(s, 200), s);
    }

    #[test]
    fn excerpt_clips_at_max_chars_when_no_sentence_boundary() {
        let s = "abcdefghij"; // 10 chars, no sentence end
        assert_eq!(excerpt(s, 5), "abcde");
    }

    #[test]
    fn excerpt_prefers_sentence_boundary() {
        let s = "First. Second. Third sentence goes on much longer.";
        // max_chars=20 — "First. Second." is 14 chars, fits
        let result = excerpt(s, 20);
        assert!(result.ends_with('.'), "expected sentence boundary: {result:?}");
    }
}
