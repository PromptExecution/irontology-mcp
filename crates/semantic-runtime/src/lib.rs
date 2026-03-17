use anyhow::{anyhow, Context, Result};
use domain::{Claim, EvidenceBundle, Relation};
use rhai::{Array, Engine, Scope, FLOAT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrelationRule {
    pub name: String,
    pub script: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorrelationOutcome {
    pub claims: Vec<Claim>,
    pub relations: Vec<Relation>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct HookContext {
    bundle: EvidenceBundle,
    claims: Vec<Claim>,
    relations: Vec<Relation>,
    notes: Vec<String>,
}

impl HookContext {
    fn new(bundle: EvidenceBundle) -> Self {
        Self {
            bundle,
            claims: Vec::new(),
            relations: Vec::new(),
            notes: Vec::new(),
        }
    }
}

pub struct SemanticRuntime {
    engine: Engine,
}

impl Default for SemanticRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticRuntime {
    pub fn new() -> Self {
        let mut engine = Engine::new();
        engine.set_max_operations(10_000);
        engine.register_type_with_name::<HookContext>("HookContext");
        engine.register_fn("concept_count", concept_count);
        engine.register_fn("concept_ids_named", concept_ids_named);
        engine.register_fn("observation_texts", observation_texts);
        engine.register_fn("emit_relation", emit_relation);
        engine.register_fn("emit_claim", emit_claim);
        engine.register_fn("emit_note", emit_note);

        Self { engine }
    }

    pub fn correlate(
        &self,
        bundle: &EvidenceBundle,
        rules: &[CorrelationRule],
    ) -> Result<CorrelationOutcome> {
        let mut outcome = CorrelationOutcome::default();

        for rule in rules {
            let mut scope = Scope::new();
            scope.push("ctx", HookContext::new(bundle.clone()));
            self.engine
                .run_with_scope(&mut scope, &rule.script)
                .map_err(|error| anyhow!("run correlation rule {}: {error}", rule.name))?;

            let state = scope
                .get_value::<HookContext>("ctx")
                .with_context(|| format!("recover hook context for rule {}", rule.name))?;
            outcome.claims.extend(state.claims);
            outcome.relations.extend(state.relations);
            outcome.notes.extend(state.notes);
        }

        Ok(outcome)
    }

    pub fn validate(&self, rules: &[CorrelationRule]) -> Result<()> {
        for rule in rules {
            self.engine
                .compile(&rule.script)
                .map_err(|error| anyhow!("compile correlation rule {}: {error}", rule.name))?;
        }
        Ok(())
    }
}

fn concept_count(ctx: &mut HookContext, label: &str) -> i64 {
    ctx.bundle.concepts_named(label).len() as i64
}

fn concept_ids_named(ctx: &mut HookContext, label: &str) -> Array {
    ctx.bundle
        .concepts_named(label)
        .into_iter()
        .map(|concept| concept.id.clone().into())
        .collect()
}

fn observation_texts(ctx: &mut HookContext, kind: &str) -> Array {
    ctx.bundle
        .observations
        .iter()
        .filter(|observation| observation.kind == kind)
        .map(|observation| observation.content.clone().into())
        .collect()
}

fn emit_relation(
    ctx: &mut HookContext,
    subject_id: &str,
    predicate: &str,
    object_id: &str,
    namespace: &str,
    confidence: FLOAT,
) {
    ctx.relations.push(Relation {
        id: format!("relation:{}", ctx.relations.len() + 1),
        subject_id: subject_id.to_string(),
        predicate: predicate.to_string(),
        object_id: object_id.to_string(),
        evidence: ctx
            .bundle
            .observations
            .iter()
            .map(|observation| observation.id.clone())
            .collect(),
        confidence: confidence as f32,
        namespace: Some(namespace.to_string()),
    });
}

fn emit_claim(
    ctx: &mut HookContext,
    subject: &str,
    predicate: &str,
    object: &str,
    namespace: &str,
    confidence: FLOAT,
) {
    ctx.claims.push(Claim {
        id: format!("claim:{}", ctx.claims.len() + 1),
        subject: subject.to_string(),
        predicate: predicate.to_string(),
        object: object.to_string(),
        evidence: ctx
            .bundle
            .observations
            .iter()
            .map(|observation| observation.id.clone())
            .collect(),
        confidence: confidence as f32,
        namespace: Some(namespace.to_string()),
    });
}

fn emit_note(ctx: &mut HookContext, note: &str) {
    ctx.notes.push(note.to_string());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::{
        Artifact, ArtifactKind, Concept, ContextNamespace, EvidenceBundle, Observation,
        SourceSystemKind,
    };
    use serde_json::json;

    use crate::{CorrelationRule, SemanticRuntime};

    fn sample_bundle() -> EvidenceBundle {
        EvidenceBundle {
            artifact: Artifact {
                id: "artifact:sharepoint:notes".to_string(),
                source_id: "sharepoint://ops".to_string(),
                source_kind: SourceSystemKind::SharePoint,
                kind: ArtifactKind::MeetingNotes,
                title: Some("Weekly dependency meeting".to_string()),
                locator: "/ops/weekly.docx".to_string(),
                media_type: Some(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        .to_string(),
                ),
                tags: BTreeMap::new(),
                valid_at: None,
                observed_at: None,
            },
            namespaces: vec![
                ContextNamespace {
                    id: "ctx:market-ops".to_string(),
                    label: "Market operations".to_string(),
                    description: None,
                    parent: None,
                },
                ContextNamespace {
                    id: "ctx:enterprise-arch".to_string(),
                    label: "Enterprise architecture".to_string(),
                    description: None,
                    parent: None,
                },
            ],
            anchors: vec![],
            observations: vec![Observation {
                id: "obs:1".to_string(),
                artifact_id: "artifact:sharepoint:notes".to_string(),
                anchor_id: None,
                kind: "definition".to_string(),
                content: "Standing data means different things in operations and architecture."
                    .to_string(),
                attributes: BTreeMap::from([(
                    "speaker".to_string(),
                    json!("enterprise architect"),
                )]),
                confidence: 0.94,
                namespace: Some("ctx:enterprise-arch".to_string()),
            }],
            claims: vec![],
            concepts: vec![
                Concept {
                    id: "concept:standing-data:ops".to_string(),
                    preferred_label: "standing data".to_string(),
                    aliases: vec![],
                    definition: Some("Operational reference data".to_string()),
                    evidence: vec!["obs:1".to_string()],
                    confidence: 0.8,
                    namespace: Some("ctx:market-ops".to_string()),
                },
                Concept {
                    id: "concept:standing-data:arch".to_string(),
                    preferred_label: "standing data".to_string(),
                    aliases: vec![],
                    definition: Some("Architectural baseline data".to_string()),
                    evidence: vec!["obs:1".to_string()],
                    confidence: 0.78,
                    namespace: Some("ctx:enterprise-arch".to_string()),
                },
            ],
            entities: vec![],
            relations: vec![],
        }
    }

    #[test]
    fn correlates_concepts_across_namespaces_from_evidence_bundle() {
        let runtime = SemanticRuntime::new();
        let outcome = runtime
            .correlate(
                &sample_bundle(),
                &[CorrelationRule {
                    name: "standing_data_disambiguation".to_string(),
                    script: r#"
                        if concept_count(ctx, "standing data") >= 2 {
                            let ids = concept_ids_named(ctx, "standing data");
                            emit_relation(
                                ctx,
                                ids[0],
                                "semantic:requires_disambiguation",
                                ids[1],
                                "ctx:enterprise-arch",
                                0.82
                            );
                            emit_note(ctx, "duplicate concept label across namespaces");
                        }
                    "#
                    .to_string(),
                }],
            )
            .expect("correlate");

        assert_eq!(outcome.relations.len(), 1);
        assert_eq!(
            outcome.relations[0].predicate,
            "semantic:requires_disambiguation"
        );
        assert_eq!(
            outcome.notes,
            vec!["duplicate concept label across namespaces"]
        );
    }

    #[test]
    fn emits_claims_from_semantic_correlation_rules() {
        let runtime = SemanticRuntime::new();
        let outcome = runtime
            .correlate(
                &sample_bundle(),
                &[CorrelationRule {
                    name: "impact_claim".to_string(),
                    script: r#"
                        let texts = observation_texts(ctx, "definition");
                        if texts.len > 0 {
                            emit_claim(
                                ctx,
                                "concept:standing-data:ops",
                                "impact:may_affect",
                                "view:decision-support",
                                "ctx:market-ops",
                                0.67
                            );
                        }
                    "#
                    .to_string(),
                }],
            )
            .expect("correlate");

        assert_eq!(outcome.claims.len(), 1);
        assert_eq!(outcome.claims[0].predicate, "impact:may_affect");
        assert_eq!(outcome.claims[0].evidence, vec!["obs:1"]);
    }

    #[test]
    fn validates_rhai_rules_before_runtime_execution() {
        let runtime = SemanticRuntime::new();
        runtime
            .validate(&[CorrelationRule {
                name: "valid_rule".to_string(),
                script: r#"if concept_count(ctx, "standing data") > 0 { emit_note(ctx, "ok"); }"#
                    .to_string(),
            }])
            .expect("validate");
    }
}
