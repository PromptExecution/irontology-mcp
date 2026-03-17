use dsl::{compile_prompt, Action, Condition};
use provider_test::FixtureProvider;

#[tokio::test]
async fn prompt_compiler_uses_provider_output_as_dsl() {
    let provider = FixtureProvider::new("fixture-model").with_chat_content(
        r#"
        rule receipt_prompt
        when
          class == "doc:Receipt"
          and contains_field("vendor")
        then
          bucket = "finance-docs-au"
          prefix = "financial/receipt/{vendor_slug}/{yyyy}/{mm}/"
          filename = "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf"
        "#,
    );

    let rule = compile_prompt(
        &provider,
        "Receipt PDFs should go to the finance bucket with vendor/date naming.",
        None,
    )
    .await
    .expect("compile prompt");

    assert_eq!(rule.name, "receipt_prompt");
    assert_eq!(
        rule.when_clause.conditions,
        vec![Condition::And(
            Box::new(Condition::Class("doc:Receipt".to_string())),
            Box::new(Condition::ContainsField("vendor".to_string())),
        )]
    );
    assert_eq!(
        rule.then_clause.actions,
        vec![
            Action::Bucket("finance-docs-au".to_string()),
            Action::Prefix("financial/receipt/{vendor_slug}/{yyyy}/{mm}/".to_string()),
            Action::Filename(
                "{date}_{vendor_slug}_{total_minor}_{currency}_receipt.pdf".to_string()
            ),
        ]
    );
}
