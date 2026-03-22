//! TDD: persistence survives NeumannStore restart
//! Red: no sled → test compiles but data lost on drop
//! Green: with sled, data restored from disk

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
