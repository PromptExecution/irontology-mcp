use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use classifier::ClassMatch;
use dsl::{Action, InputFile, Rule, RuleMatcher};
use handlers::Extraction;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoragePlan {
    pub bucket: String,
    pub prefix: String,
    pub filename: String,
    pub tags: BTreeMap<String, String>,
    pub ontology_class: String,
    pub shape: String,
}

pub trait NamingPolicy: Send + Sync {
    fn derive(&self, ext: &Extraction, class: &ClassMatch) -> Result<StoragePlan>;
}

pub struct DslNamingPolicy {
    rules: Vec<Rule>,
}

impl DslNamingPolicy {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }
}

impl NamingPolicy for DslNamingPolicy {
    fn derive(&self, ext: &Extraction, class: &ClassMatch) -> Result<StoragePlan> {
        let fields: Vec<&str> = ext.fields.keys().map(String::as_str).collect();
        let file = InputFile {
            extension: "",
            media_type: "",
            fields: &fields,
            class: Some(class.class.as_str()),
            shape: Some(class.shape.as_str()),
        };

        let rule = self
            .rules
            .iter()
            .find(|rule| RuleMatcher::matches(rule, &file))
            .ok_or_else(|| anyhow!("no naming rule matched {}", class.class))?;

        let bucket = action_value(rule, |action| match action {
            Action::Bucket(value) => Some(value),
            _ => None,
        })?;
        let prefix = action_value(rule, |action| match action {
            Action::Prefix(value) => Some(value),
            _ => None,
        })?;
        let filename = action_value(rule, |action| match action {
            Action::Filename(value) => Some(value),
            _ => None,
        })?;

        let mut tags = BTreeMap::new();
        if let Some(Action::Tags(entries)) = rule
            .then_clause
            .actions
            .iter()
            .find(|action| matches!(action, Action::Tags(_)))
        {
            for (key, value) in entries {
                tags.insert(key.clone(), render_template(value, ext));
            }
        }
        tags.insert("ontology_class".to_string(), class.class.clone());
        tags.insert("shape".to_string(), class.shape.clone());

        Ok(StoragePlan {
            bucket: render_template(bucket, ext),
            prefix: render_template(prefix, ext),
            filename: render_template(filename, ext),
            tags,
            ontology_class: class.class.clone(),
            shape: class.shape.clone(),
        })
    }
}

fn action_value<'a, F>(rule: &'a Rule, project: F) -> Result<&'a str>
where
    F: Fn(&'a Action) -> Option<&'a String>,
{
    rule.then_clause
        .actions
        .iter()
        .find_map(project)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("naming rule missing storage action"))
}

fn render_template(template: &str, ext: &Extraction) -> String {
    let vendor = string_field(ext, "vendor");
    let date = string_field(ext, "date");
    let currency = string_field(ext, "currency").to_lowercase();
    let total_minor = total_minor(ext);
    let vendor_slug = slugify(&vendor);
    let (yyyy, mm) = split_date(&date);

    template
        .replace("{vendor_slug}", &vendor_slug)
        .replace("{date}", &date)
        .replace("{yyyy}", &yyyy)
        .replace("{mm}", &mm)
        .replace("{currency}", &currency)
        .replace("{total_minor}", &total_minor.to_string())
}

fn string_field(ext: &Extraction, key: &str) -> String {
    ext.fields
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn total_minor(ext: &Extraction) -> i64 {
    if let Some(value) = ext.fields.get("total_minor").and_then(Value::as_i64) {
        return value;
    }
    if let Some(value) = ext.fields.get("total").and_then(Value::as_f64) {
        return (value * 100.0).round() as i64;
    }
    ext.amounts
        .iter()
        .find(|amount| amount.label == "total")
        .map(|amount| amount.amount_minor)
        .unwrap_or_default()
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            dash = false;
            out.push(ch);
        } else if !dash && !out.is_empty() {
            dash = true;
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn split_date(date: &str) -> (String, String) {
    let mut parts = date.split('-');
    let year = parts.next().unwrap_or_default().to_string();
    let month = parts.next().unwrap_or_default().to_string();
    (year, month)
}

#[cfg(test)]
mod tests {
    use classifier::ClassMatch;
    use handlers::{Extraction, MoneyValue, TemporalValue};
    use serde_json::json;

    use crate::{DslNamingPolicy, NamingPolicy};

    #[test]
    fn derives_storage_plan_from_dsl_rule() {
        let rule = dsl::compile_rule(
            r#"
            rule receipt_naming
            when
              class == "doc:Receipt" and shape == "shape:ReceiptShape"
            then
              bucket = "finance-docs-au"
              prefix = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
              filename = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
              tags = { "vendor" : "{vendor_slug}", "year" : "{yyyy}" }
            "#,
        )
        .expect("parse");

        let policy = DslNamingPolicy::new(vec![rule]);
        let extraction = Extraction {
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
        };
        let class_match = ClassMatch {
            class: "doc:Receipt".to_string(),
            shape: "shape:ReceiptShape".to_string(),
            confidence: 1.0,
            matched_by: vec![
                "vendor".to_string(),
                "date".to_string(),
                "total".to_string(),
            ],
        };

        let plan = policy.derive(&extraction, &class_match).expect("plan");
        assert_eq!(plan.bucket, "finance-docs-au");
        assert_eq!(plan.prefix, "financial/receipt/officeworks/2026/03/");
        assert_eq!(
            plan.filename,
            "2026-03-07_officeworks_14895_aud_receipt.pdf"
        );
        assert_eq!(plan.tags["ontology_class"], "doc:Receipt");
        assert_eq!(plan.tags["shape"], "shape:ReceiptShape");
        assert_eq!(plan.tags["vendor"], "officeworks");
    }
}
