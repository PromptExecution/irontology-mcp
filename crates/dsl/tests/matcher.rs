use dsl::{compile_rule, InputFile, RuleMatcher};

#[test]
fn contains_field_matches_when_field_is_present() {
    let rule = compile_rule(
        r#"
rule receipt_csv
  when
    extension == ".csv" and media_type == "text/csv"
    contains_field("nmi")
  then
    handler = csv_receipt
"#,
    )
    .expect("parse");

    assert!(RuleMatcher::matches(
        &rule,
        &InputFile {
            extension: ".csv",
            media_type: "text/csv",
            fields: &["nmi", "amount"],
            class: None,
            shape: None,
        }
    ));
}

#[test]
fn contains_field_rejects_when_field_is_missing() {
    let rule = compile_rule(
        r#"
rule receipt_csv
  when
    extension == ".csv"
    contains_field("nmi")
  then
    handler = csv_receipt
"#,
    )
    .expect("parse");

    assert!(!RuleMatcher::matches(
        &rule,
        &InputFile {
            extension: ".csv",
            media_type: "text/csv",
            fields: &["amount", "vendor"],
            class: None,
            shape: None,
        }
    ));
}

#[test]
fn class_and_shape_match_when_present() {
    let rule = compile_rule(
        r#"
rule receipt_naming
  when
    class == "doc:Receipt"
    and shape == "shape:ReceiptShape"
  then
    bucket = finance-docs-au
"#,
    )
    .expect("parse");

    assert!(RuleMatcher::matches(
        &rule,
        &InputFile {
            extension: ".pdf",
            media_type: "application/pdf",
            fields: &[],
            class: Some("doc:Receipt"),
            shape: Some("shape:ReceiptShape"),
        }
    ));
}

#[test]
fn class_and_shape_reject_when_mismatched() {
    let rule = compile_rule(
        r#"
rule receipt_naming
  when
    class == "doc:Receipt"
    and shape == "shape:ReceiptShape"
  then
    bucket = finance-docs-au
"#,
    )
    .expect("parse");

    assert!(!RuleMatcher::matches(
        &rule,
        &InputFile {
            extension: ".pdf",
            media_type: "application/pdf",
            fields: &[],
            class: Some("doc:Invoice"),
            shape: Some("shape:ReceiptShape"),
        }
    ));
}
