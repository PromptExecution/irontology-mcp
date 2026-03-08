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
