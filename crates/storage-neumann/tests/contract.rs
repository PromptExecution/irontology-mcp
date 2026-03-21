use std::sync::Arc;

use serde_json::json;
use storage_neumann::{
    config::NeumannConfig, EmbeddingModality, EmbeddingRecord, FactRecord, FileRecord,
    KnowledgeStore, NeumannStore, SemanticQuery,
};
use tempfile::tempdir;

#[tokio::test]
async fn neumann_store_contract_basics() {
    let dir = tempdir().expect("tempdir");
    let store = NeumannStore::try_new(test_config(dir.path().join("basics"))).expect("open store");

    store
        .upsert_file(FileRecord {
            id: "file:git:blob:blob-1".to_string(),
            blob: "blob-1".to_string(),
            path: "src/lib.rs".to_string(),
            media_type: "text/plain".to_string(),
            size: 42,
            commit: "commit-1".to_string(),
        })
        .await
        .expect("upsert file");
    store
        .upsert_facts(vec![FactRecord {
            subject: "file:git:blob:blob-1".to_string(),
            predicate: "media_type".to_string(),
            object: json!("text/plain"),
        }])
        .await
        .expect("upsert facts");
    store
        .upsert_embeddings(vec![
            EmbeddingRecord {
                id: "sym:a".to_string(),
                source_blob: "blob-1".to_string(),
                vector: Arc::from([1.0_f32, 0.0_f32]),
                modality: EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
            },
            EmbeddingRecord {
                id: "sym:b".to_string(),
                source_blob: "blob-2".to_string(),
                vector: Arc::from([0.0_f32, 1.0_f32]),
                modality: EmbeddingModality::DocChunk,
                semantic_weight: 1.0,
            },
        ])
        .await
        .expect("upsert");

    assert!(store.has_blob("blob-1").await.expect("has_blob"));

    let files = store
        .query(SemanticQuery::Files {
            path: Some("src/lib.rs".to_string()),
            blob: None,
        })
        .await
        .expect("file query");
    assert_eq!(files.files.len(), 1);

    let facts = store
        .query(SemanticQuery::Facts {
            subject: Some("file:git:blob:blob-1".to_string()),
            predicate: Some("media_type".to_string()),
        })
        .await
        .expect("fact query");
    assert_eq!(facts.facts.len(), 1);

    let result = store
        .query(SemanticQuery::Vector {
            embedding: Arc::from([0.9_f32, 0.1_f32]),
            top_k: 1,
            modality: Some(EmbeddingModality::CodeSymbol),
        })
        .await
        .expect("query");

    assert_eq!(result.ids, vec!["sym:a".to_string()]);
}

#[tokio::test]
async fn neumann_ingests_ontology_turtle_resources() {
    let dir = tempdir().expect("tempdir");
    let store = NeumannStore::try_new(test_config(dir.path().join("ontology"))).expect("open store");
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
    assert_eq!(
        evidence,
        vec!["https://example.org/pe/doc/incident-42".to_string()]
    );

    let labels = store
        .related_objects(
            "https://example.org/pe/topic/payment-retries",
            "http://www.w3.org/2004/02/skos/core#prefLabel",
        )
        .await
        .expect("topic labels");
    assert_eq!(labels, vec!["Payment retries".to_string()]);
}

fn test_config(path: std::path::PathBuf) -> NeumannConfig {
    NeumannConfig {
        endpoint: "http://localhost:7777".to_string(),
        namespace: "test".to_string(),
        data_path: Some(path),
    }
}
