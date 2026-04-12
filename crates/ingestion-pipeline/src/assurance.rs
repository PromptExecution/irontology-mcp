use std::collections::HashSet;

use domain::EvidenceBundle;

use crate::PipelineRegistry;

pub enum AssuranceLevel {
    /// Single best-fit pipeline.
    Standard,
    /// Run 2 pipelines, report if results diverge.
    Corroborated,
    /// Run all capable pipelines, merge, flag disagreements.
    HighAssurance,
}

pub struct MultiPipelineResult {
    pub merged: EvidenceBundle,
    pub pipeline_count: usize,
    /// 0.0-1.0, how consistent pipelines were.
    pub agreement_score: f32,
    /// Human-readable divergence notes.
    pub divergences: Vec<String>,
}

/// Compute Jaccard similarity between two strings on word sets.
pub fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let words_a: HashSet<&str> = a.split_whitespace().collect();
    let words_b: HashSet<&str> = b.split_whitespace().collect();
    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }
    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

/// Compute agreement score from multiple bundles.
///
/// The score is the ratio of observations that appear in ≥ 2 pipelines
/// (by content similarity > 0.8 using simple Jaccard on word sets).
pub fn compute_agreement_score(bundles: &[(String, EvidenceBundle)]) -> f32 {
    if bundles.len() < 2 {
        return 1.0;
    }

    let all_observations: Vec<(usize, &str)> = bundles
        .iter()
        .enumerate()
        .flat_map(|(pipeline_idx, (_, bundle))| {
            bundle
                .observations
                .iter()
                .map(move |obs| (pipeline_idx, obs.content.as_str()))
        })
        .collect();

    if all_observations.is_empty() {
        return 1.0;
    }

    // For each observation, check if it has a sufficiently similar counterpart
    // in at least one other pipeline.
    let mut corroborated_count = 0usize;
    let total = all_observations.len();

    for (i, (pipeline_i, content_i)) in all_observations.iter().enumerate() {
        let mut has_corroboration = false;
        for (j, (pipeline_j, content_j)) in all_observations.iter().enumerate() {
            if i == j || pipeline_i == pipeline_j {
                continue;
            }
            if jaccard_similarity(content_i, content_j) > 0.8 {
                has_corroboration = true;
                break;
            }
        }
        if has_corroboration {
            corroborated_count += 1;
        }
    }

    corroborated_count as f32 / total as f32
}

/// Detect divergences between bundles: observations present in one pipeline but
/// not in any other (Jaccard < 0.8).
pub fn detect_divergences(bundles: &[(String, EvidenceBundle)]) -> Vec<String> {
    if bundles.len() < 2 {
        return Vec::new();
    }

    let mut divergences = Vec::new();

    for (pipeline_name, bundle) in bundles {
        for obs in &bundle.observations {
            let corroborated = bundles.iter().any(|(other_name, other_bundle)| {
                if other_name == pipeline_name {
                    return false;
                }
                other_bundle
                    .observations
                    .iter()
                    .any(|other_obs| jaccard_similarity(&obs.content, &other_obs.content) > 0.8)
            });

            if !corroborated {
                divergences.push(format!(
                    "Pipeline '{}' unique observation: '{}'",
                    pipeline_name,
                    truncate(&obs.content, 80)
                ));
            }
        }
    }

    divergences
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}...", &s[..max_chars])
    }
}

pub fn run_multi_pipeline(
    bundles: Vec<(String, EvidenceBundle)>,
) -> MultiPipelineResult {
    let pipeline_count = bundles.len();
    let agreement_score = compute_agreement_score(&bundles);
    let divergences = detect_divergences(&bundles);
    let merged = PipelineRegistry::merge_bundles(bundles);
    MultiPipelineResult {
        merged,
        pipeline_count,
        agreement_score,
        divergences,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical_strings() {
        assert!((jaccard_similarity("hello world", "hello world") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_disjoint_strings() {
        assert!((jaccard_similarity("alpha beta", "gamma delta") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let score = jaccard_similarity("a b c", "b c d");
        // intersection={b,c}=2, union={a,b,c,d}=4 => 0.5
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn agreement_score_identical_bundles() {
        use std::collections::BTreeMap;
        use domain::{Artifact, ArtifactKind, EvidenceBundle, Observation, SourceSystemKind};

        let artifact = Artifact {
            id: "art:1".to_string(),
            source_id: "src".to_string(),
            source_kind: SourceSystemKind::GitRepository,
            kind: ArtifactKind::WikiPage,
            title: None,
            locator: "doc.txt".to_string(),
            media_type: None,
            tags: BTreeMap::new(),
            valid_at: None,
            observed_at: None,
        };
        let obs = Observation {
            id: "obs:1".to_string(),
            artifact_id: "art:1".to_string(),
            anchor_id: None,
            kind: "text_block".to_string(),
            content: "hello world foo bar baz".to_string(),
            attributes: BTreeMap::new(),
            confidence: 0.9,
            namespace: None,
        };
        let bundle_a = EvidenceBundle {
            artifact: artifact.clone(),
            namespaces: vec![],
            anchors: vec![],
            observations: vec![obs.clone()],
            claims: vec![],
            concepts: vec![],
            entities: vec![],
            relations: vec![],
        };
        let mut obs_b = obs.clone();
        obs_b.id = "obs:2".to_string();
        let bundle_b = EvidenceBundle {
            observations: vec![obs_b],
            ..bundle_a.clone()
        };

        let bundles = vec![
            ("pipeline_a".to_string(), bundle_a),
            ("pipeline_b".to_string(), bundle_b),
        ];
        let score = compute_agreement_score(&bundles);
        assert!(score >= 0.99, "identical bundles should score ~1.0, got {score}");
    }

    #[test]
    fn agreement_score_different_bundles() {
        use std::collections::BTreeMap;
        use domain::{Artifact, ArtifactKind, EvidenceBundle, Observation, SourceSystemKind};

        let artifact = Artifact {
            id: "art:1".to_string(),
            source_id: "src".to_string(),
            source_kind: SourceSystemKind::GitRepository,
            kind: ArtifactKind::WikiPage,
            title: None,
            locator: "doc.txt".to_string(),
            media_type: None,
            tags: BTreeMap::new(),
            valid_at: None,
            observed_at: None,
        };
        let make_obs = |id: &str, content: &str| Observation {
            id: id.to_string(),
            artifact_id: "art:1".to_string(),
            anchor_id: None,
            kind: "text_block".to_string(),
            content: content.to_string(),
            attributes: BTreeMap::new(),
            confidence: 0.9,
            namespace: None,
        };
        let bundle_a = EvidenceBundle {
            artifact: artifact.clone(),
            namespaces: vec![],
            anchors: vec![],
            observations: vec![make_obs("obs:1", "alpha beta gamma delta")],
            claims: vec![],
            concepts: vec![],
            entities: vec![],
            relations: vec![],
        };
        let bundle_b = EvidenceBundle {
            observations: vec![make_obs("obs:2", "completely different words here")],
            ..bundle_a.clone()
        };

        let bundles = vec![
            ("pipeline_a".to_string(), bundle_a),
            ("pipeline_b".to_string(), bundle_b),
        ];
        let score = compute_agreement_score(&bundles);
        assert!(score < 0.01, "completely different bundles should score ~0.0, got {score}");
    }
}
