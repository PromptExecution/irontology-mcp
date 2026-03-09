use std::sync::Arc;

use storage_neumann::{
    config::NeumannConfig, EmbeddingRecord, KnowledgeStore, NeumannStore, SemanticQuery,
};

#[tokio::test]
async fn neumann_store_contract_basics() {
    let store = NeumannStore::new(NeumannConfig::default());

    store
        .upsert_embeddings(vec![
            EmbeddingRecord {
                id: "sym:a".to_string(),
                source_blob: "blob-1".to_string(),
                vector: Arc::from([1.0_f32, 0.0_f32]),
            },
            EmbeddingRecord {
                id: "sym:b".to_string(),
                source_blob: "blob-2".to_string(),
                vector: Arc::from([0.0_f32, 1.0_f32]),
            },
        ])
        .await
        .expect("upsert");

    assert!(store.has_blob("blob-1").await.expect("has_blob"));

    let result = store
        .query(SemanticQuery::Vector {
            embedding: Arc::from([0.9_f32, 0.1_f32]),
            top_k: 1,
        })
        .await
        .expect("query");

    assert_eq!(result.ids, vec!["sym:a".to_string()]);
}

#[tokio::test]
async fn neumann_ingests_ontology_turtle_resources() {
    let store = NeumannStore::new(NeumannConfig::default());
    let naming = r#"@prefix ex: <https://example.org/pe/> .
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
"#;

    store
        .ingest_turtle("ontology://naming_conventions", naming)
        .await
        .expect("ingest turtle");

    let has_topic = store
        .related_objects(
            "https://example.org/pe/doc/incident-42",
            "https://example.org/pe/hasTopic",
        )
        .await
        .expect("doc topics");
    assert_eq!(
        has_topic,
        vec!["https://example.org/pe/topic/payment-retries".to_string()]
    );

    let evidence = store
        .related_objects(
            "https://example.org/pe/topic/payment-retries",
            "https://example.org/pe/evidenceIn",
        )
        .await
        .expect("topic evidence");
    assert_eq!(evidence, vec!["https://example.org/pe/doc/incident-42".to_string()]);

    let labels = store
        .related_objects(
            "https://example.org/pe/topic/payment-retries",
            "http://www.w3.org/2004/02/skos/core#prefLabel",
        )
        .await
        .expect("topic labels");
    assert_eq!(labels, vec!["Payment retries".to_string()]);
}
