use anyhow::{anyhow, Result};
use classifier::{ClassMatch, Classifier};
use handlers::{Extraction, HandlerRegistry, IntakeFile};
use naming::{NamingPolicy, StoragePlan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeOutcome {
    Classified,
    ClassifiedLowConfidence,
    Unclassified,
    FailedExtraction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntakeDecision {
    pub handler_name: String,
    pub extraction: Extraction,
    pub class_match: Option<ClassMatch>,
    pub storage_plan: Option<StoragePlan>,
    pub outcome: IntakeOutcome,
}

pub async fn plan_file(
    file: &IntakeFile,
    registry: &HandlerRegistry,
    classifier: &dyn Classifier,
    naming: &dyn NamingPolicy,
) -> Result<IntakeDecision> {
    let handler = registry
        .select(file)
        .ok_or_else(|| anyhow!("no handler available"))?;
    let extraction = handler.extract(file).await?;
    let matches = classifier.classify(&extraction).await?;
    let class_match = matches.into_iter().next();

    match class_match {
        Some(class_match) => {
            let plan = naming.derive(&extraction, &class_match)?;
            let outcome = if class_match.confidence >= 0.9 {
                IntakeOutcome::Classified
            } else {
                IntakeOutcome::ClassifiedLowConfidence
            };
            Ok(IntakeDecision {
                handler_name: handler.name().to_string(),
                extraction,
                class_match: Some(class_match),
                storage_plan: Some(plan),
                outcome,
            })
        }
        None => Ok(IntakeDecision {
            handler_name: handler.name().to_string(),
            extraction,
            class_match: None,
            storage_plan: None,
            outcome: IntakeOutcome::Unclassified,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use bytes::Bytes;
    use classifier::{ShapeRule, ShapeRuleClassifier};
    use handlers::{
        Extraction, FileHandler, HandlerRegistry, HandlerScore, IntakeFile, MoneyValue,
        TemporalValue,
    };
    use naming::DslNamingPolicy;
    use serde_json::json;

    use crate::{plan_file, IntakeOutcome};

    struct ReceiptHandler;

    #[async_trait]
    impl FileHandler for ReceiptHandler {
        fn name(&self) -> &str {
            "pdf_receipt"
        }

        fn score(&self, file: &IntakeFile) -> HandlerScore {
            if file.media_type.as_deref() == Some("application/pdf") {
                HandlerScore(0.9)
            } else {
                HandlerScore(0.0)
            }
        }

        async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
            Ok(Extraction {
                detected_kind: "receipt".to_string(),
                text: Some("Officeworks".to_string()),
                fields: std::collections::BTreeMap::from([
                    ("vendor".to_string(), json!("Officeworks")),
                    ("date".to_string(), json!("2026-03-07")),
                    ("currency".to_string(), json!("AUD")),
                    ("total".to_string(), json!(148.95)),
                ]),
                dates: vec![TemporalValue {
                    label: "issue_date".to_string(),
                    value: "2026-03-07".to_string(),
                }],
                amounts: vec![MoneyValue {
                    label: "total".to_string(),
                    amount_minor: 14895,
                    currency: "AUD".to_string(),
                }],
                entities: vec![],
            })
        }
    }

    #[tokio::test]
    async fn plans_receipt_intake_end_to_end() {
        let registry = HandlerRegistry::new(vec![Arc::new(ReceiptHandler)]);
        let classifier = ShapeRuleClassifier::new(vec![ShapeRule {
            class: "doc:Receipt".to_string(),
            shape: "shape:ReceiptShape".to_string(),
            required_fields: vec![
                "vendor".to_string(),
                "date".to_string(),
                "total".to_string(),
            ],
            detected_kind: Some("receipt".to_string()),
        }]);
        let policy = DslNamingPolicy::new(vec![dsl::compile_rule(
            r#"
            rule receipt_naming
            when
              class == "doc:Receipt" and shape == "shape:ReceiptShape"
            then
              bucket = "finance-docs-au"
              prefix = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
              filename = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
              tags = { "vendor" : "{vendor_slug}" }
            "#,
        )
        .expect("rule")]);

        let decision = plan_file(
            &IntakeFile {
                sha256: [3; 32],
                bytes: Bytes::from_static(b"fake receipt"),
                path_hint: Some("receipt.pdf".to_string()),
                media_type: Some("application/pdf".to_string()),
            },
            &registry,
            &classifier,
            &policy,
        )
        .await
        .expect("plan");

        assert_eq!(decision.handler_name, "pdf_receipt");
        assert_eq!(decision.outcome, IntakeOutcome::Classified);
        assert_eq!(
            decision.storage_plan.as_ref().expect("plan").filename,
            "2026-03-07_officeworks_14895_aud_receipt.pdf"
        );
    }
}
