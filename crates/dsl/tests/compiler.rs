use dsl::{compile_rule, InputFile, RuleMatcher};

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
        }
    ));
}
