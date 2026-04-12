use std::sync::Arc;

use anyhow::Result;
use retrieval::{fusion_search, FusionWeights, SearchBackend, StoreBackedBackend};
use storage_neumann::{
    config::NeumannConfig, AnchorRecord, ArtifactRecord, EdgeKind, EdgeRecord, EmbeddingModality,
    EmbeddingRecord, FactRecord, FileRecord, KnowledgeStore, NeumannStore, SymbolRecord,
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
                anchor_id: None,
                artifact_locator: None,
            },
            EmbeddingRecord {
                id: "sym:beta".to_string(),
                source_blob: "blob-alpha".to_string(),
                vector: Arc::from([0.0_f32, 0.0_f32]),
                modality: EmbeddingModality::CodeSymbol,
                semantic_weight: 1.0,
                anchor_id: None,
                artifact_locator: None,
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

#[tokio::test]
async fn anchor_locator_propagates_through_vector_search() -> Result<()> {
    let store = NeumannStore::new(NeumannConfig::default());

    // Register an artifact with a source URI (e.g. legislation URL)
    store
        .upsert_artifact(ArtifactRecord {
            id: "file:git:blob:blob-legislation".to_string(),
            source_uri: "https://legislation.gov.au/act/42".to_string(),
            source_kind: "LegislationPortal".to_string(),
            artifact_kind: "LegislationDocument".to_string(),
            title: Some("Connection Act 2024".to_string()),
            locator: "docs/legislation/connection-act.md".to_string(),
            media_type: Some("text/plain".to_string()),
            content_sha256: "blob-legislation".to_string(),
            valid_at: Some("2024-01-01".to_string()),
            observed_at: None,
        })
        .await?;

    // Register a §-section anchor
    store
        .upsert_anchors(vec![AnchorRecord {
            id: "anchor:§5.3.1a".to_string(),
            artifact_id: "file:git:blob:blob-legislation".to_string(),
            kind: "section".to_string(),
            locator: "§5.3.1(a)".to_string(),
            label: Some("Connection agreements".to_string()),
            byte_offset: None,
            char_offset: None,
        }])
        .await?;

    // Store an embedding for this anchor chunk, linking anchor_id and artifact_locator
    store
        .upsert_embeddings(vec![EmbeddingRecord {
            id: "file:git:blob:blob-legislation#chunk-0".to_string(),
            source_blob: "blob-legislation".to_string(),
            vector: Arc::from([1.0_f32, 0.0_f32, 0.5_f32]),
            modality: EmbeddingModality::DocChunk,
            semantic_weight: 1.0,
            anchor_id: Some("anchor:§5.3.1a".to_string()),
            artifact_locator: Some("docs/legislation/connection-act.md".to_string()),
        }])
        .await?;

    let backend = retrieval::StoreBackedBackend::from_snapshot(store.snapshot());
    let results = backend.search_vector("connection agreements", 3)?;

    assert!(!results.is_empty(), "should have at least one result");
    let hit = &results[0];
    assert_eq!(hit.id, "file:git:blob:blob-legislation#chunk-0");
    assert_eq!(
        hit.anchor_locator.as_deref(),
        Some("§5.3.1(a)"),
        "anchor locator should be §5.3.1(a)"
    );
    assert_eq!(
        hit.artifact_uri.as_deref(),
        Some("https://legislation.gov.au/act/42"),
        "artifact URI should be the legislation URL"
    );

    // Also verify get_anchors_for works
    let anchors = store.get_anchors_for("file:git:blob:blob-legislation").await?;
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].locator, "§5.3.1(a)");

    Ok(())
}
