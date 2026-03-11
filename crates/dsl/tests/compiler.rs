use std::collections::BTreeMap;

use dsl::{compile_rule, Action, Condition, InputFile, RuleMatcher};

#[test]
fn compilation_is_deterministic_and_matchable() {
    let src = include_str!("../rules/rust.tomllm");
    let a = compile_rule(src).expect("first parse");
    let b = compile_rule(src).expect("second parse");

    assert_eq!(a, b);
    assert!(RuleMatcher::matches(
        &a,
        &InputFile {
            extension: ".rs",
            media_type: "text/plain",
            fields: &[],
            class: None,
            shape: None,
        }
    ));
}

#[test]
fn parses_boolean_conditions_and_all_actions() {
    let src = r#"
# comment
rule receipt_csv
  when
    extension == ".csv" and media_type == "text/csv"
    contains_field("nmi")
  then
    handler = csv_receipt
    extract = [rows, totals]
    embed = [rows]
    ontology = receipt_document
    bucket = "{repo_slug}/receipts"
    prefix = "{region}/"
    filename = "{blob_hash}.csv"
    classify = [shape:Receipt]
"#;

    let rule = compile_rule(src).expect("parse");

    assert_eq!(rule.name, "receipt_csv");
    assert_eq!(
        rule.when_clause.conditions,
        vec![
            Condition::And(
                Box::new(Condition::Extension(".csv".to_string())),
                Box::new(Condition::MediaType("text/csv".to_string())),
            ),
            Condition::ContainsField("nmi".to_string()),
        ]
    );
    assert_eq!(
        rule.then_clause.actions,
        vec![
            Action::Handler("csv_receipt".to_string()),
            Action::Extract(vec!["rows".to_string(), "totals".to_string()]),
            Action::Embed(vec!["rows".to_string()]),
            Action::Ontology("receipt_document".to_string()),
            Action::Bucket("{repo_slug}/receipts".to_string()),
            Action::Prefix("{region}/".to_string()),
            Action::Filename("{blob_hash}.csv".to_string()),
            Action::Classify(vec!["shape:Receipt".to_string()]),
        ]
    );
}

#[test]
fn parses_naming_rule_conditions_and_tags_action() {
    let src = r#"
rule receipt_naming
  when
    class == "doc:Receipt"
    and shape == "shape:ReceiptShape"
  then
    bucket = "finance-docs-au"
    prefix = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
    filename = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
    tags = {
      "ontology_class": "doc:Receipt",
      "shacl_shape": "shape:ReceiptShape",
      "vendor": "{vendor}"
    }
"#;

    let rule = compile_rule(src).expect("parse");

    assert_eq!(
        rule.when_clause.conditions,
        vec![Condition::And(
            Box::new(Condition::Class("doc:Receipt".to_string())),
            Box::new(Condition::Shape("shape:ReceiptShape".to_string())),
        )]
    );

    let mut tags = BTreeMap::new();
    tags.insert("ontology_class".to_string(), "doc:Receipt".to_string());
    tags.insert("shacl_shape".to_string(), "shape:ReceiptShape".to_string());
    tags.insert("vendor".to_string(), "{vendor}".to_string());

    assert_eq!(
        rule.then_clause.actions,
        vec![
            Action::Bucket("finance-docs-au".to_string()),
            Action::Prefix("financial/receipt/{vendor_slug}/{yyyy}/{mm}/".to_string()),
            Action::Filename(
                "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf".to_string()
            ),
            Action::Tags(tags),
        ]
    );
}
