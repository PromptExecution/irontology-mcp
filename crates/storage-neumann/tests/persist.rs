//! TDD: persistence survives NeumannStore restart
//! Red: no sled → test compiles but data lost on drop
//! Green: with sled, data restored from disk

use storage_neumann::{KnowledgeStore, NeumannStore};
use storage_neumann::config::NeumannConfig;

#[tokio::test]
async fn triple_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    let config = NeumannConfig {
        endpoint: "http://localhost:7777".into(),
        namespace: "persist-test".into(),
        data_dir: Some(path.clone()),
    };

    // Write
    {
        let store = NeumannStore::new(config.clone());
        store
            .ingest_turtle(
                "test-source",
                r#"<http://ex.org/a> <http://ex.org/p> <http://ex.org/b> ."#,
            )
            .await
            .unwrap();
    } // store dropped — must flush to sled

    // Restore
    {
        let store = NeumannStore::new(config.clone());
        let objects = store
            .related_objects("http://ex.org/a", "http://ex.org/p")
            .await
            .unwrap();
        assert_eq!(objects, vec!["http://ex.org/b"]);
    }
}

#[tokio::test]
async fn in_memory_still_works_without_data_dir() {
    let config = NeumannConfig::default(); // data_dir = None
    let store = NeumannStore::new(config);
    store
        .ingest_turtle(
            "test-source",
            r#"<http://ex.org/x> <http://ex.org/q> <http://ex.org/y> ."#,
        )
        .await
        .unwrap();
    let objects = store
        .related_objects("http://ex.org/x", "http://ex.org/q")
        .await
        .unwrap();
    assert_eq!(objects, vec!["http://ex.org/y"]);
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
