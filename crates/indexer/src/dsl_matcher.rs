use dsl::{InputFile as DslInputFile, Rule, RuleMatcher as DslRuleMatcher};

use crate::{IntakeFile, RuleMatcher};

pub struct DslRuleMatcherAdapter {
    rules: Vec<Rule>,
}

impl DslRuleMatcherAdapter {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }
}

impl RuleMatcher for DslRuleMatcherAdapter {
    fn match_file(&self, file: &IntakeFile) -> bool {
        let fields = file.fields.iter().map(String::as_str).collect::<Vec<_>>();

        let input = DslInputFile {
            extension: &file.extension,
            media_type: &file.media_type,
            fields: fields.as_slice(),
            class: file.class.as_deref(),
            shape: file.shape.as_deref(),
        };

        self.rules
            .iter()
            .any(|rule| DslRuleMatcher::matches(rule, &input))
    }
}
