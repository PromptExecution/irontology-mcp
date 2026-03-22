use tempfile::tempdir;

use storage_neumann::{config::NeumannConfig, KnowledgeStore, NeumannStore};

#[tokio::test]
async fn turtle_ingest_persists_across_restart() {
    let dir = tempdir().expect("tempdir");
    let config = NeumannConfig {
        endpoint: "http://localhost:7777".to_string(),
        namespace: "persist-test".to_string(),
        data_path: Some(dir.path().join("neumann")),
    };
    let turtle = r#"@prefix ex: <https://example.org/pe/> .
<https://example.org/pe/doc/incident-42> ex:hasTopic <https://example.org/pe/topic/payment-retries> .
"#;

    {
        let store = NeumannStore::try_new(config.clone()).expect("open store");
        store
            .ingest_turtle("ontology://persist", turtle)
            .await
            .expect("ingest turtle");
    }

    let store = NeumannStore::try_new(config).expect("open store");
    let related = store
        .related_objects(
            "https://example.org/pe/doc/incident-42",
            "https://example.org/pe/hasTopic",
        )
        .await
        .expect("load related objects");

    assert_eq!(
        related,
        vec!["https://example.org/pe/topic/payment-retries".to_string()]
    );
}

#[tokio::test]
async fn in_memory_store_works_without_data_path() {
    let config = NeumannConfig::default(); // data_path = None
    let store = NeumannStore::try_new(config).expect("open store");
    store
        .ingest_turtle(
            "test-source",
            r#"<http://ex.org/x> <http://ex.org/q> <http://ex.org/y> ."#,
        )
        .await
        .expect("ingest turtle");
    let objects = store
        .related_objects("http://ex.org/x", "http://ex.org/q")
        .await
        .expect("related objects");
    assert_eq!(objects, vec!["http://ex.org/y"]);
}
