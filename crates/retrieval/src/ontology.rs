use anyhow::Result;

use crate::fusion::RankedResult;

pub fn search(query: &str, top_k: usize) -> Result<Vec<RankedResult>> {
    let mut out: Vec<RankedResult> = query
        .split_whitespace()
        .enumerate()
        .map(|(i, t)| RankedResult {
            id: format!("ont:{t}"),
            score: 0.4 / ((i + 1) as f32),
        })
        .collect();
    out.truncate(top_k);
    Ok(out)
}
