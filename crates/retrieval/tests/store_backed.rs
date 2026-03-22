use std::sync::Arc;

use anyhow::Result;
use retrieval::{fusion_search, FusionWeights, SearchBackend, StoreBackedBackend};
use storage_neumann::{
    config::NeumannConfig, EdgeKind, EdgeRecord, EmbeddingModality, EmbeddingRecord, FactRecord,
    FileRecord, KnowledgeStore, NeumannStore, SymbolRecord,
};

#[tokio::test]
async fn store_backed_backend_ranks_real_store_content() -> Result<()> {
    let store = NeumannStore::new(NeumannConfig::default());

    store
        .upsert_file(FileRecord {
            id: "file:git:blob:blob-alpha".to_string(),
            blob: "blob-alpha".to_string(),
            path: "src/alpha.rs".to_string(),
            media_type: "text/plain".to_string(),
            size: 12,
            commit: "commit-1".to_string(),
        })
        .await?;
    store
        .upsert_facts(vec![
            FactRecord {
                subject: "sym:alpha".to_string(),
                predicate: "class".to_string(),
                object: serde_json::json!("Function"),
            },
            FactRecord {
                subject: "sym:beta".to_string(),
                predicate: "class".to_string(),
                object: serde_json::json!("Type"),
            },
        ])
        .await?;
    store
        .upsert_symbols(vec![
            SymbolRecord {
                id: "sym:alpha".to_string(),
                blob: "blob-alpha".to_string(),
                path: "src/alpha.rs".to_string(),
                name: "alpha".to_string(),
                kind: "Function".to_string(),
                start_line: 1,
                end_line: 4,
                signature: Some("fn alpha()".to_string()),
                content: "alpha beta".to_string(),
            },
            SymbolRecord {
                id: "sym:beta".to_string(),
                blob: "blob-alpha".to_string(),
                path: "src/beta.rs".to_string(),
                name: "beta".to_string(),
                kind: "Type".to_string(),
                start_line: 1,
                end_line: 4,
                signature: Some("struct Beta".to_string()),
                content: "beta".to_string(),
            },
        ])
        .await?;
    store
        .upsert_edges(vec![EdgeRecord {
            from: "sym:alpha".to_string(),
            to: "sym:beta".to_string(),
            kind: EdgeKind::Calls,
            weight: 100,
        }])
        .await?;
    store
        .upsert_embeddings(vec![
            EmbeddingRecord {
                id: "sym:alpha".to_string(),
                source_blob: "blob-alpha".to_string(),
                vector: Arc::from([1.0_f32, 1.0_f32]),
                modality: EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
            },
            EmbeddingRecord {
                id: "sym:beta".to_string(),
                source_blob: "blob-alpha".to_string(),
                vector: Arc::from([0.0_f32, 0.0_f32]),
                modality: EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
            },
        ])
        .await?;
    store
        .ingest_turtle(
            "ontology://sample",
            r#"@prefix ex: <https://example.org/pe/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

ex:Topic a rdf:Class .
ex:Topic ex:relatedTo ex:Concept .
"#,
        )
        .await?;

    let backend = StoreBackedBackend::from_snapshot(store.snapshot());

    let vector_results = backend.search_vector("alpha", 2)?;
    assert_eq!(vector_results[0].id, "sym:alpha");

    let graph_results = backend.search_graph("alpha", 2)?;
    assert_eq!(graph_results[0].id, "sym:alpha");

    let lexical_results = backend.search_lexical("alpha", 2)?;
    assert!(lexical_results.iter().any(|result| result.id == "sym:alpha"));

    let ontology_results = backend.search_ontology("topic", 2)?;
    assert!(
        ontology_results
            .iter()
            .any(|result| result.id == "https://example.org/pe/Topic")
    );

    let fused = fusion_search("alpha topic", 2, FusionWeights::default(), &backend)?;
    let fused_again = fusion_search("alpha topic", 2, FusionWeights::default(), &backend)?;
    assert_eq!(fused, fused_again);
    assert!(fused.iter().any(|result| result.id == "sym:alpha"));

    Ok(())
}
