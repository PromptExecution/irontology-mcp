use anyhow::{anyhow, Result};
use lalrpop_util::lalrpop_mod;

use crate::ast::Rule;

lalrpop_mod!(pub grammar);

pub fn compile_rule(input: &str) -> Result<Rule> {
    let normalized = input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    grammar::RuleParser::new()
        .parse(&normalized)
        .map_err(|err| anyhow!("failed to parse DSL rule:\n{normalized}\n{err}"))
}
