use crate::ast::{Condition, Rule};

#[derive(Debug, Clone)]
pub struct InputFile<'a> {
    pub extension: &'a str,
    pub media_type: &'a str,
}

pub struct RuleMatcher;

impl RuleMatcher {
    pub fn matches(rule: &Rule, file: &InputFile<'_>) -> bool {
        rule.when_clause
            .conditions
            .iter()
            .all(|condition| match condition {
                Condition::Extension(ext) => ext == file.extension,
                Condition::MediaType(mt) => mt == file.media_type,
                Condition::ContainsField(_) => false,
                Condition::And(lhs, rhs) => eval(lhs, file) && eval(rhs, file),
            })
    }
}

fn eval(condition: &Condition, file: &InputFile<'_>) -> bool {
    match condition {
        Condition::Extension(ext) => ext == file.extension,
        Condition::MediaType(mt) => mt == file.media_type,
        Condition::ContainsField(_) => false,
        Condition::And(lhs, rhs) => eval(lhs, file) && eval(rhs, file),
    }
}
