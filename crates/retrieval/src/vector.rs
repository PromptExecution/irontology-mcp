use anyhow::Result;

use crate::fusion::RankedResult;

pub fn search(query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
    let mut out = seed(query, "vec");
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(top_k);
    Ok(out)
}

fn seed(query: &str, ns: &str) -> Vec<RankedResult> {
    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return vec![];
    }

    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| RankedResult {
            id: format!("{ns}:{t}"),
            score: 1.0 / ((i + 1) as f32),
        })
        .collect()
}
