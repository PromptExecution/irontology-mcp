use anyhow::Result;
use retrieval::{fusion_search, FusionWeights, RankedResult, SearchBackend};

struct FixedBackend;
impl SearchBackend for FixedBackend {
    fn search_vector(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![RankedResult {
            id: "A".into(),
            score: 0.9,

            anchor_locator: None,

            artifact_uri: None,
        }])
    }
    fn search_graph(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![
            RankedResult {
                id: "A".into(),
                score: 0.8,

                anchor_locator: None,

                artifact_uri: None,
            },
            RankedResult {
                id: "B".into(),
                score: 0.7,

                anchor_locator: None,

                artifact_uri: None,
            },
        ])
    }
    fn search_lexical(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![RankedResult {
            id: "B".into(),
            score: 0.6,

            anchor_locator: None,

            artifact_uri: None,
        }])
    }
    fn search_ontology(&self, _query: &str, _top_k: usize) -> Result<Vec<RankedResult>> {
        Ok(vec![RankedResult {
            id: "A".into(),
            score: 0.4,

            anchor_locator: None,

            artifact_uri: None,
        }])
    }
}

#[test]
fn deterministic_fusion_scoring() {
    let weights = FusionWeights::default();
    let first = fusion_search("alpha", 2, weights, &FixedBackend).expect("first");
    let second = fusion_search("alpha", 2, weights, &FixedBackend).expect("second");
    assert_eq!(first, second);
    assert_eq!(first[0].id, "A");
}
