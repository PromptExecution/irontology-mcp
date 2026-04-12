pub mod assurance;
pub mod docling;
pub mod langextract;

use std::collections::HashSet;

use async_trait::async_trait;
use domain::{Artifact, EvidenceBundle};

/// Confidence [0.0, 1.0] that this pipeline can handle this artifact.
/// 0.0 = cannot handle. 1.0 = purpose-built for this type.
#[derive(Debug, Clone, Copy)]
pub struct PipelineConfidence(pub f32);

#[async_trait]
pub trait ExternalIngestionPipeline: Send + Sync {
    fn name(&self) -> &str;

    /// Estimate how well this pipeline handles the given artifact.
    /// Uses MIME type, file extension, content sniff, and source metadata.
    fn can_handle(&self, artifact: &Artifact) -> PipelineConfidence;

    /// Extract an EvidenceBundle from the artifact.
    async fn extract(&self, artifact: &Artifact) -> anyhow::Result<EvidenceBundle>;

    /// Check if the underlying tool/service is available (process exists, HTTP endpoint responds).
    async fn is_available(&self) -> bool;
}

pub struct PipelineRegistry {
    pipelines: Vec<Box<dyn ExternalIngestionPipeline>>,
}

impl Default for PipelineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineRegistry {
    pub fn new() -> Self {
        Self {
            pipelines: Vec::new(),
        }
    }

    pub fn register(&mut self, p: Box<dyn ExternalIngestionPipeline>) {
        self.pipelines.push(p);
    }

    /// Select the highest-confidence available pipeline.
    pub async fn select_best(
        &self,
        artifact: &Artifact,
    ) -> Option<&dyn ExternalIngestionPipeline> {
        let mut best: Option<(f32, &dyn ExternalIngestionPipeline)> = None;
        for pipeline in &self.pipelines {
            let confidence = pipeline.can_handle(artifact).0;
            if confidence <= 0.0 {
                continue;
            }
            let is_better = best.as_ref().map_or(true, |(c, _)| confidence > *c);
            if is_better && pipeline.is_available().await {
                best = Some((confidence, pipeline.as_ref()));
            }
        }
        best.map(|(_, p)| p)
    }

    /// Run ALL pipelines with confidence > threshold in parallel.
    /// Used for multi-pipeline assurance mode.
    pub async fn extract_all(
        &self,
        artifact: &Artifact,
        min_confidence: f32,
    ) -> Vec<anyhow::Result<(String, EvidenceBundle)>> {
        let mut candidates: Vec<&dyn ExternalIngestionPipeline> = Vec::new();
        for pipeline in &self.pipelines {
            let confidence = pipeline.can_handle(artifact).0;
            if confidence > min_confidence && pipeline.is_available().await {
                candidates.push(pipeline.as_ref());
            }
        }

        let mut results = Vec::new();
        for pipeline in candidates {
            let name = pipeline.name().to_string();
            let result = pipeline.extract(artifact).await;
            results.push(result.map(|bundle| (name, bundle)));
        }
        results
    }

    /// Merge multiple EvidenceBundles into one with merged evidence refs.
    /// Observations from multiple pipelines are deduplicated by content hash.
    pub fn merge_bundles(bundles: Vec<(String, EvidenceBundle)>) -> EvidenceBundle {
        if bundles.is_empty() {
            panic!("merge_bundles called with empty bundle list");
        }

        let (_, first_bundle) = bundles.into_iter().fold(
            (0usize, None::<EvidenceBundle>),
            |(idx, acc), (_pipeline_name, bundle)| {
                let merged = match acc {
                    None => bundle,
                    Some(mut existing) => {
                        let mut seen_obs: HashSet<String> = existing
                            .observations
                            .iter()
                            .map(|o| o.content.clone())
                            .collect();
                        for obs in bundle.observations {
                            if seen_obs.insert(obs.content.clone()) {
                                existing.observations.push(obs);
                            }
                        }

                        let mut seen_anchors: HashSet<String> = existing
                            .anchors
                            .iter()
                            .map(|a| a.id.clone())
                            .collect();
                        for anchor in bundle.anchors {
                            if seen_anchors.insert(anchor.id.clone()) {
                                existing.anchors.push(anchor);
                            }
                        }

                        let mut seen_claims: HashSet<String> = existing
                            .claims
                            .iter()
                            .map(|c| c.id.clone())
                            .collect();
                        for claim in bundle.claims {
                            if seen_claims.insert(claim.id.clone()) {
                                existing.claims.push(claim);
                            }
                        }

                        let mut seen_concepts: HashSet<String> = existing
                            .concepts
                            .iter()
                            .map(|c| c.id.clone())
                            .collect();
                        for concept in bundle.concepts {
                            if seen_concepts.insert(concept.id.clone()) {
                                existing.concepts.push(concept);
                            }
                        }

                        let mut seen_entities: HashSet<String> = existing
                            .entities
                            .iter()
                            .map(|e| e.id.clone())
                            .collect();
                        for entity in bundle.entities {
                            if seen_entities.insert(entity.id.clone()) {
                                existing.entities.push(entity);
                            }
                        }

                        let mut seen_relations: HashSet<String> = existing
                            .relations
                            .iter()
                            .map(|r| r.id.clone())
                            .collect();
                        for relation in bundle.relations {
                            if seen_relations.insert(relation.id.clone()) {
                                existing.relations.push(relation);
                            }
                        }

                        // Merge namespaces
                        let mut seen_ns: HashSet<String> = existing
                            .namespaces
                            .iter()
                            .map(|n| n.id.clone())
                            .collect();
                        for ns in bundle.namespaces {
                            if seen_ns.insert(ns.id.clone()) {
                                existing.namespaces.push(ns);
                            }
                        }

                        existing
                    }
                };
                (idx + 1, Some(merged))
            },
        );

        first_bundle.expect("merge_bundles: at least one bundle required")
    }
}
