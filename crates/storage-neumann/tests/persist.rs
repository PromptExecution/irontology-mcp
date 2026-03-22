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
}
