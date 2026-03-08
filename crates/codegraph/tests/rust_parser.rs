use codegraph::{
    extractors::{extract, Language},
    EdgeKind,
};

#[test]
fn extracts_functions_imports_and_calls() {
    let src = domain::sample_source();
    let graph = extract(Language::Rust, "abc123", src).expect("graph build");

    let names: Vec<_> = graph.nodes().map(|n| n.name.clone()).collect();
    assert!(names.iter().any(|n| n == "alpha"));
    assert!(names.iter().any(|n| n == "beta"));
    assert!(graph
        .edges()
        .any(|e| matches!(e, EdgeKind::Imports | EdgeKind::Calls)));
}
