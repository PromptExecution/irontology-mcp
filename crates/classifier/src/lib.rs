use anyhow::Result;
use async_trait::async_trait;
use handlers::Extraction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassMatch {
    pub class: String,
    pub shape: String,
    pub confidence: f32,
    pub matched_by: Vec<String>,
}

#[async_trait]
pub trait Classifier: Send + Sync {
    async fn classify(&self, ext: &Extraction) -> Result<Vec<ClassMatch>>;
}

#[derive(Debug, Clone)]
pub struct ShapeRule {
    pub class: String,
    pub shape: String,
    pub required_fields: Vec<String>,
    pub detected_kind: Option<String>,
}

pub struct ShapeRuleClassifier {
    rules: Vec<ShapeRule>,
}

impl ShapeRuleClassifier {
    pub fn new(rules: Vec<ShapeRule>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl Classifier for ShapeRuleClassifier {
    async fn classify(&self, ext: &Extraction) -> Result<Vec<ClassMatch>> {
        let mut matches = Vec::new();
        for rule in &self.rules {
            if let Some(kind) = rule.detected_kind.as_deref() {
                if kind != ext.detected_kind {
                    continue;
                }
            }

            let matched_by: Vec<String> = rule
                .required_fields
                .iter()
                .filter(|field| ext.fields.contains_key(field.as_str()))
                .cloned()
                .collect();

            if matched_by.len() == rule.required_fields.len() {
                matches.push(ClassMatch {
                    class: rule.class.clone(),
                    shape: rule.shape.clone(),
                    confidence: 1.0,
                    matched_by,
                });
            }
        }

        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use handlers::{Extraction, MoneyValue, TemporalValue};
    use serde_json::json;

    use crate::{Classifier, ShapeRule, ShapeRuleClassifier};

    #[tokio::test]
    async fn classifies_receipts_from_required_fields() {
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

        let extraction = Extraction {
            detected_kind: "receipt".to_string(),
            text: Some("Officeworks total".to_string()),
            fields: std::collections::BTreeMap::from([
                ("vendor".to_string(), json!("Officeworks")),
                ("date".to_string(), json!("2026-03-07")),
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
        };

        let matches = classifier.classify(&extraction).await.expect("classify");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].class, "doc:Receipt");
        assert_eq!(matches[0].shape, "shape:ReceiptShape");
    }
}
