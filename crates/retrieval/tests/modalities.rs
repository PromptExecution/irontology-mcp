use retrieval::{fusion_search, DeterministicBackend, FusionWeights};

#[test]
fn deterministic_backend_produces_ranked_results() {
    let backend = DeterministicBackend;
    let results =
        fusion_search("alpha beta", 3, FusionWeights::default(), &backend).expect("search");

    assert!(!results.is_empty());
    assert!(results.len() <= 3);
}
