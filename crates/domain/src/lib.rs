use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn sample_source() -> &'static str {
    r#"
use std::fmt::Debug;

pub fn alpha() { beta(); }

fn beta() {}
"#
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceSystemKind {
    GitRepository,
    SharePoint,
    DatabaseSchema,
    DocumentSilo,
    ProcessCatalog,
    ArXiv,
    HuggingFacePapers,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    SourceCode,
    ArchitectureDocument,
    ProjectPlan,
    MeetingNotes,
    Presentation,
    Diagram,
    DatabaseSchema,
    DatabaseRecordSet,
    WikiPage,
    Spreadsheet,
    AcademicPaper,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextNamespace {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub source_id: String,
    pub source_kind: SourceSystemKind,
    pub kind: ArtifactKind,
    pub title: Option<String>,
    pub locator: String,
    pub media_type: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub valid_at: Option<String>,
    pub observed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub id: String,
    pub artifact_id: String,
    pub kind: String,
    pub locator: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub artifact_id: String,
    pub anchor_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub attributes: BTreeMap<String, Value>,
    pub confidence: f32,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub evidence: Vec<String>,
    pub confidence: f32,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    pub id: String,
    pub preferred_label: String,
    pub aliases: Vec<String>,
    pub definition: Option<String>,
    pub evidence: Vec<String>,
    pub confidence: f32,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub canonical_name: String,
    pub external_refs: BTreeMap<String, String>,
    pub evidence: Vec<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_id: String,
    pub evidence: Vec<String>,
    pub confidence: f32,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub artifact: Artifact,
    pub namespaces: Vec<ContextNamespace>,
    pub anchors: Vec<Anchor>,
    pub observations: Vec<Observation>,
    pub claims: Vec<Claim>,
    pub concepts: Vec<Concept>,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
}

impl EvidenceBundle {
    pub fn namespace(&self, id: &str) -> Option<&ContextNamespace> {
        self.namespaces.iter().find(|namespace| namespace.id == id)
    }

    pub fn concepts_named(&self, label: &str) -> Vec<&Concept> {
        self.concepts
            .iter()
            .filter(|concept| {
                concept.preferred_label.eq_ignore_ascii_case(label)
                    || concept
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(label))
            })
            .collect()
    }

    pub fn evidence_refs(&self) -> Vec<&str> {
        let mut refs = Vec::new();
        refs.extend(
            self.observations
                .iter()
                .map(|observation| observation.id.as_str()),
        );
        for claim in &self.claims {
            refs.extend(claim.evidence.iter().map(String::as_str));
        }
        for relation in &self.relations {
            refs.extend(relation.evidence.iter().map(String::as_str));
        }
        refs
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        Anchor, Artifact, ArtifactKind, Claim, Concept, ContextNamespace, EvidenceBundle,
        Observation, Relation, SourceSystemKind,
    };

    fn sample_bundle() -> EvidenceBundle {
        EvidenceBundle {
            artifact: Artifact {
                id: "artifact:sharepoint:plan-42".to_string(),
                source_id: "sharepoint://program-delivery".to_string(),
                source_kind: SourceSystemKind::SharePoint,
                kind: ArtifactKind::ProjectPlan,
                title: Some("Program roadmap".to_string()),
                locator: "/plans/program-roadmap.docx".to_string(),
                media_type: Some(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        .to_string(),
                ),
                tags: std::collections::BTreeMap::from([(
                    "portfolio".to_string(),
                    "delivery".to_string(),
                )]),
                valid_at: Some("2026-03-12".to_string()),
                observed_at: Some("2026-03-12T09:00:00Z".to_string()),
            },
            namespaces: vec![
                ContextNamespace {
                    id: "ctx:market-ops".to_string(),
                    label: "Market operations".to_string(),
                    description: Some("Operational data and dispatch terms".to_string()),
                    parent: None,
                },
                ContextNamespace {
                    id: "ctx:enterprise-arch".to_string(),
                    label: "Enterprise architecture".to_string(),
                    description: Some("Architecture planning and governance".to_string()),
                    parent: None,
                },
            ],
            anchors: vec![Anchor {
                id: "anchor:standing-data".to_string(),
                artifact_id: "artifact:sharepoint:plan-42".to_string(),
                kind: "paragraph".to_string(),
                locator: "p17".to_string(),
                label: Some("Standing data workstream".to_string()),
            }],
            observations: vec![Observation {
                id: "obs:standing-data-definition".to_string(),
                artifact_id: "artifact:sharepoint:plan-42".to_string(),
                anchor_id: Some("anchor:standing-data".to_string()),
                kind: "definition".to_string(),
                content: "Standing data requires reconciliation before dispatch.".to_string(),
                attributes: std::collections::BTreeMap::from([(
                    "speaker".to_string(),
                    json!("Program board"),
                )]),
                confidence: 0.92,
                namespace: Some("ctx:market-ops".to_string()),
            }],
            claims: vec![Claim {
                id: "claim:dispatch-standing-data".to_string(),
                subject: "concept:standing-data:market-ops".to_string(),
                predicate: "depends_on".to_string(),
                object: "entity:dispatch-engine".to_string(),
                evidence: vec!["obs:standing-data-definition".to_string()],
                confidence: 0.88,
                namespace: Some("ctx:market-ops".to_string()),
            }],
            concepts: vec![
                Concept {
                    id: "concept:standing-data:market-ops".to_string(),
                    preferred_label: "standing data".to_string(),
                    aliases: vec!["dispatch standing data".to_string()],
                    definition: Some("Reference data used in dispatch operations.".to_string()),
                    evidence: vec!["obs:standing-data-definition".to_string()],
                    confidence: 0.81,
                    namespace: Some("ctx:market-ops".to_string()),
                },
                Concept {
                    id: "concept:standing-data:enterprise-arch".to_string(),
                    preferred_label: "standing data".to_string(),
                    aliases: vec!["architecture standing data".to_string()],
                    definition: Some("Baseline architectural master data.".to_string()),
                    evidence: vec!["obs:standing-data-definition".to_string()],
                    confidence: 0.65,
                    namespace: Some("ctx:enterprise-arch".to_string()),
                },
            ],
            entities: vec![],
            relations: vec![Relation {
                id: "rel:semantic-overlap".to_string(),
                subject_id: "concept:standing-data:market-ops".to_string(),
                predicate: "semantic:overlaps_with".to_string(),
                object_id: "concept:standing-data:enterprise-arch".to_string(),
                evidence: vec!["obs:standing-data-definition".to_string()],
                confidence: 0.51,
                namespace: Some("ctx:enterprise-arch".to_string()),
            }],
        }
    }

    #[test]
    fn preserves_same_term_across_distinct_context_namespaces() {
        let bundle = sample_bundle();

        let concepts = bundle.concepts_named("standing data");
        assert_eq!(concepts.len(), 2);
        assert_ne!(concepts[0].namespace, concepts[1].namespace);
        assert_eq!(
            bundle
                .namespace("ctx:market-ops")
                .expect("market namespace")
                .label,
            "Market operations"
        );
    }

    #[test]
    fn retains_provenance_for_claims_and_relations() {
        let bundle = sample_bundle();
        let refs = bundle.evidence_refs();

        assert!(refs.contains(&"obs:standing-data-definition"));
        assert_eq!(
            bundle.claims[0].evidence,
            vec!["obs:standing-data-definition"]
        );
        assert_eq!(
            bundle.relations[0].evidence,
            vec!["obs:standing-data-definition"]
        );
    }
}
