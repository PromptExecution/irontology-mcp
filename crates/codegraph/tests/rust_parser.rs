use codegraph::{
    extractors::{extract, Language},
    EdgeKind, SymbolKind,
};

#[test]
fn extracts_functions_imports_and_calls() {
    let src = domain::sample_source();
    let graph = extract(Language::Rust, "abc123", src).expect("graph build");

    let names: Vec<_> = graph.nodes().map(|n| n.name.clone()).collect();
    assert!(names.iter().any(|n| n == "__module__"));
    assert!(names.iter().any(|n| n == "alpha"));
    assert!(names.iter().any(|n| n == "beta"));
    assert!(names.iter().any(|n| n == "std::fmt::Debug"));
    assert!(graph
        .edge_refs()
        .any(|(from, to, kind)| from.name == "__module__"
            && to.name == "std::fmt::Debug"
            && matches!(kind, EdgeKind::Imports)));
    assert!(graph
        .edges()
        .any(|e| matches!(e, EdgeKind::Imports | EdgeKind::Calls)));
}

#[test]
fn extracts_type_symbols_and_test_relationships() {
    let src = r#"
pub struct Widget;

enum Mode {
    Fast,
}

fn alpha() {}

#[test]
fn smoke_test() {
    alpha();
}
"#;

    let graph = extract(Language::Rust, "blob-types", src).expect("graph build");

    assert_eq!(
        graph.node_named("Widget").expect("widget").kind,
        SymbolKind::Type
    );
    assert_eq!(
        graph.node_named("Mode").expect("mode").kind,
        SymbolKind::Type
    );
    assert_eq!(
        graph.node_named("smoke_test").expect("test fn").kind,
        SymbolKind::Test
    );
    assert!(graph
        .edge_refs()
        .any(|(from, to, kind)| from.name == "smoke_test"
            && to.name == "alpha"
            && matches!(kind, EdgeKind::Tests)));
}

#[test]
fn resolves_qualified_call_sites() {
    let src = r#"
mod helpers {
    pub fn beta() {}
}

fn alpha() {
    crate::helpers::beta();
}
"#;

    let graph = extract(Language::Rust, "blob-qualified", src).expect("graph build");

    assert!(graph
        .edge_refs()
        .any(|(from, to, kind)| from.name == "alpha"
            && to.name == "beta"
            && matches!(kind, EdgeKind::Calls)));
}
