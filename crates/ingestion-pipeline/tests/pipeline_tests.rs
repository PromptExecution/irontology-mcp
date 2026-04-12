use std::collections::BTreeMap;

use domain::{Artifact, ArtifactKind, EvidenceBundle, Observation, SourceSystemKind};
use ingestion_pipeline::{
    assurance::compute_agreement_score,
    docling::DoclingPipeline,
    langextract::LangextractPipeline,
    ExternalIngestionPipeline, PipelineRegistry,
};

fn make_artifact(
    locator: &str,
    media_type: Option<&str>,
    kind: ArtifactKind,
    source_kind: SourceSystemKind,
) -> Artifact {
    Artifact {
        id: format!("artifact:test:{}", locator),
        source_id: "test-source".to_string(),
        source_kind,
        kind,
        title: None,
        locator: locator.to_string(),
        media_type: media_type.map(String::from),
        tags: BTreeMap::new(),
        valid_at: None,
        observed_at: None,
    }
}

fn make_bundle(artifact: &Artifact, observations: Vec<(&str, &str)>) -> EvidenceBundle {
    EvidenceBundle {
        artifact: artifact.clone(),
        namespaces: vec![],
        anchors: vec![],
        observations: observations
            .into_iter()
            .enumerate()
            .map(|(i, (id, content))| Observation {
                id: id.to_string(),
                artifact_id: artifact.id.clone(),
                anchor_id: None,
                kind: "text_block".to_string(),
                content: content.to_string(),
                attributes: BTreeMap::new(),
                confidence: 0.9,
                namespace: None,
            })
            .collect(),
        claims: vec![],
        concepts: vec![],
        entities: vec![],
        relations: vec![],
    }
}

/// 1. Pipeline registry selects docling for PDF artifacts.
#[test]
fn pipeline_registry_selects_docling_for_pdf() {
    let pipeline = DoclingPipeline::new();
    let artifact = make_artifact(
        "report.pdf",
        Some("application/pdf"),
        ArtifactKind::ArchitectureDocument,
        SourceSystemKind::DocumentSilo,
    );
    let confidence = pipeline.can_handle(&artifact).0;
    assert!(
        confidence > 0.9,
        "DoclingPipeline should have confidence > 0.9 for PDF, got {confidence}"
    );
}

/// 2. Pipeline registry selects langextract for plain text.
#[test]
fn pipeline_registry_selects_langextract_for_text() {
    let pipeline = LangextractPipeline::new();
    let artifact = make_artifact(
        "notes.txt",
        Some("text/plain"),
        ArtifactKind::MeetingNotes,
        SourceSystemKind::GitRepository,
    );
    let confidence = pipeline.can_handle(&artifact).0;
    assert!(
        confidence >= 0.75,
        "LangextractPipeline should have confidence >= 0.75 for .txt, got {confidence}"
    );
}

/// 3. merge_bundles deduplicates observations by content.
#[test]
fn merge_bundles_deduplicates_observations() {
    let artifact = make_artifact(
        "doc.txt",
        None,
        ArtifactKind::MeetingNotes,
        SourceSystemKind::GitRepository,
    );

    let shared_content = "shared observation content here";
    let bundle_a = make_bundle(
        &artifact,
        vec![
            ("obs:a:1", shared_content),
            ("obs:a:2", "unique to pipeline A"),
        ],
    );
    let bundle_b = make_bundle(
        &artifact,
        vec![
            ("obs:b:1", shared_content), // duplicate by content
            ("obs:b:2", "unique to pipeline B"),
        ],
    );

    let merged = PipelineRegistry::merge_bundles(vec![
        ("pipeline_a".to_string(), bundle_a),
        ("pipeline_b".to_string(), bundle_b),
    ]);

    // 4 total observations, but shared_content is duplicated → should be 3
    assert_eq!(
        merged.observations.len(),
        3,
        "Merged bundle should have 3 deduplicated observations, got {}",
        merged.observations.len()
    );
    let contents: Vec<&str> = merged
        .observations
        .iter()
        .map(|o| o.content.as_str())
        .collect();
    assert!(contents.contains(&shared_content));
    assert!(contents.contains(&"unique to pipeline A"));
    assert!(contents.contains(&"unique to pipeline B"));
}

/// 4a. Two identical bundles → agreement score ~1.0.
#[test]
fn multi_pipeline_agreement_score_identical() {
    let artifact = make_artifact(
        "doc.txt",
        None,
        ArtifactKind::MeetingNotes,
        SourceSystemKind::GitRepository,
    );
    let obs_content = "the quick brown fox jumps over the lazy dog";
    let bundle_a = make_bundle(&artifact, vec![("obs:1", obs_content)]);
    let mut bundle_b = bundle_a.clone();
    bundle_b.observations[0].id = "obs:2".to_string(); // different ID, same content

    let bundles = vec![
        ("pipeline_a".to_string(), bundle_a),
        ("pipeline_b".to_string(), bundle_b),
    ];
    let score = compute_agreement_score(&bundles);
    assert!(
        score >= 0.99,
        "Identical bundles should score ~1.0, got {score}"
    );
}

/// 4b. Completely different bundles → agreement score ~0.0.
#[test]
fn multi_pipeline_agreement_score_different() {
    let artifact = make_artifact(
        "doc.txt",
        None,
        ArtifactKind::MeetingNotes,
        SourceSystemKind::GitRepository,
    );
    let bundle_a = make_bundle(&artifact, vec![("obs:1", "alpha beta gamma delta epsilon")]);
    let bundle_b = make_bundle(
        &artifact,
        vec![("obs:2", "completely unrelated content here xyz")],
    );

    let bundles = vec![
        ("pipeline_a".to_string(), bundle_a),
        ("pipeline_b".to_string(), bundle_b),
    ];
    let score = compute_agreement_score(&bundles);
    assert!(
        score < 0.01,
        "Completely different bundles should score ~0.0, got {score}"
    );
}

/// 5. When is_available() returns false (subprocess pipelines not installed),
///    select_best returns None gracefully.
#[tokio::test]
async fn pipeline_registry_graceful_fallback_when_unavailable() {
    // DoclingPipeline with a non-existent binary → is_available() = false
    let mut registry = PipelineRegistry::new();
    // Use a clearly non-existent binary name
    let docling = ingestion_pipeline::docling::DoclingPipeline::with_binary(
        "__nonexistent_docling_binary_xyz__",
    );
    registry.register(Box::new(docling));

    let artifact = make_artifact(
        "report.pdf",
        Some("application/pdf"),
        ArtifactKind::ArchitectureDocument,
        SourceSystemKind::DocumentSilo,
    );
    let best = registry.select_best(&artifact).await;
    assert!(
        best.is_none(),
        "Should return None when no pipelines are available"
    );
}
