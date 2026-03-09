use serde_json::json;

use crate::Resource;

pub fn list_classes() -> serde_json::Value {
    json!({
        "classes": [
            "CodeSymbol",
            "Module",
            "Function",
            "OntologyNode",
            "Document",
            "Topic",
            "SemanticAnchor"
        ]
    })
}

pub fn phase2_resources() -> Vec<Resource> {
    vec![
        Resource {
            uri: "ontology://classes".to_string(),
            mime_type: "application/json".to_string(),
            body: list_classes().to_string(),
        },
        Resource {
            uri: "ontology://predicates".to_string(),
            mime_type: "text/turtle".to_string(),
            body: predicates_turtle().to_string(),
        },
        Resource {
            uri: "ontology://shapes".to_string(),
            mime_type: "text/turtle".to_string(),
            body: shapes_turtle().to_string(),
        },
        Resource {
            uri: "ontology://naming_conventions".to_string(),
            mime_type: "text/turtle".to_string(),
            body: naming_conventions_turtle().to_string(),
        },
        Resource {
            uri: "ontology://query_languages".to_string(),
            mime_type: "application/json".to_string(),
            body: json!({
                "query_languages": ["sparql"],
                "serializations": ["turtle", "json-ld", "n-triples"]
            })
            .to_string(),
        },
    ]
}

fn predicates_turtle() -> &'static str {
    r#"@prefix ex: <https://example.org/pe/> .
@prefix oa: <http://www.w3.org/ns/oa#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .

ex:hasTopic a rdf:Property ;
    rdfs:domain ex:Document ;
    rdfs:range ex:Topic ;
    rdfs:label "has topic" .

ex:about a rdf:Property ;
    rdfs:domain ex:SemanticAnchor ;
    rdfs:range ex:Topic ;
    rdfs:label "about" .

ex:evidenceIn a rdf:Property ;
    rdfs:domain ex:Topic ;
    rdfs:range ex:Document ;
    rdfs:label "evidence in" .

oa:hasBody a rdf:Property .
oa:hasTarget a rdf:Property .
skos:prefLabel a rdf:Property .
"#
}

fn shapes_turtle() -> &'static str {
    r#"@prefix ex: <https://example.org/pe/> .
@prefix oa: <http://www.w3.org/ns/oa#> .
@prefix sh: <http://www.w3.org/ns/shacl#> .

ex:SemanticAnchorShape a sh:NodeShape ;
    sh:targetClass ex:SemanticAnchor ;
    sh:property [
        sh:path oa:hasBody ;
        sh:class ex:Topic ;
    ] ;
    sh:property [
        sh:path oa:hasTarget ;
        sh:minCount 1 ;
    ] .
"#
}

fn naming_conventions_turtle() -> &'static str {
    r#"@prefix ex: <https://example.org/pe/> .
@prefix oa: <http://www.w3.org/ns/oa#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .

ex:Document a rdfs:Class .
ex:Topic a rdfs:Class ;
    rdfs:subClassOf skos:Concept .
ex:SemanticAnchor a rdfs:Class ;
    rdfs:subClassOf oa:Annotation .

<https://example.org/pe/topic/payment-retries> a ex:Topic ;
    skos:prefLabel "Payment retries" .

<https://example.org/pe/doc/incident-42> a ex:Document ;
    ex:hasTopic <https://example.org/pe/topic/payment-retries> .

<https://example.org/pe/anchor/incident-42-item-7> a ex:SemanticAnchor, oa:Annotation ;
    oa:hasTarget <https://example.org/pe/doc/incident-42#item-7> ;
    oa:hasBody <https://example.org/pe/topic/payment-retries> ;
    ex:about <https://example.org/pe/topic/payment-retries> .

<https://example.org/pe/topic/payment-retries> ex:evidenceIn <https://example.org/pe/doc/incident-42> .
"#
}
