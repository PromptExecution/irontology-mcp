use codegraph::extractors::{extract, Language};

#[test]
fn extraction_is_idempotent_for_same_source() {
    let src = domain::sample_source();

    let g1 = extract(Language::Rust, "blob-x", src).expect("first");
    let g2 = extract(Language::Rust, "blob-x", src).expect("second");

    assert_eq!(g1.node_count(), g2.node_count());
    assert_eq!(g1.edge_count(), g2.edge_count());
}
