use mcp_server::ResourceRegistry;

#[test]
fn registry_contains_phase2_ontology_resources() {
    let registry = ResourceRegistry::with_phase2_resources();

    assert!(registry.has("ontology://classes"));
    assert!(registry.has("ontology://predicates"));
    assert!(registry.has("ontology://shapes"));
    assert!(registry.has("ontology://naming_conventions"));
    assert!(registry.has("ontology://query_languages"));
    assert!(registry.get("ontology://missing").is_none());
}

#[test]
fn naming_conventions_resource_is_rdf_native_turtle() {
    let registry = ResourceRegistry::with_phase2_resources();
    let resource = registry
        .get("ontology://naming_conventions")
        .expect("naming conventions resource");

    assert_eq!(resource.mime_type, "text/turtle");
    assert!(resource.body.contains("oa:Annotation"));
    assert!(resource.body.contains("skos:Concept"));
    assert!(resource.body.contains("ex:Topic"));
    assert!(resource.body.contains("ex:hasTopic"));
    assert!(resource.body.contains("ex:evidenceIn"));
    assert!(resource
        .body
        .contains("<https://example.org/pe/topic/payment-retries>"));
}

#[test]
fn query_languages_resource_mentions_sparql() {
    let registry = ResourceRegistry::with_phase2_resources();
    let resource = registry
        .get("ontology://query_languages")
        .expect("query languages resource");

    assert_eq!(resource.mime_type, "application/json");
    assert!(resource.body.contains("sparql"));
}
