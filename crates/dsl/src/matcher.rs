use crate::ast::{Condition, Rule};

#[derive(Debug, Clone)]
pub struct InputFile<'a> {
    pub extension: &'a str,
    pub media_type: &'a str,
    pub fields: &'a [&'a str],
    pub class: Option<&'a str>,
    pub shape: Option<&'a str>,
}

pub struct RuleMatcher;

impl RuleMatcher {
    pub fn matches(rule: &Rule, file: &InputFile<'_>) -> bool {
        rule.when_clause
            .conditions
            .iter()
            .all(|condition| eval(condition, file))
    }
}

fn eval(condition: &Condition, file: &InputFile<'_>) -> bool {
    match condition {
        Condition::Extension(ext) => ext == file.extension,
        Condition::MediaType(mt) => mt == file.media_type,
        Condition::ContainsField(field) => file.fields.iter().any(|candidate| candidate == field),
        Condition::Class(class) => file.class == Some(class.as_str()),
        Condition::Shape(shape) => file.shape == Some(shape.as_str()),
        Condition::And(lhs, rhs) => eval(lhs, file) && eval(rhs, file),
    }
}
