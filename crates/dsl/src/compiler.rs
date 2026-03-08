use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::ast::{Action, Condition, Rule, ThenClause, WhenClause};

pub fn compile_rule(input: &str) -> Result<Rule> {
    let lines: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();

    let mut name = String::new();
    let mut in_when = false;
    let mut in_then = false;
    let mut conditions = Vec::new();
    let mut kv = HashMap::new();

    for line in lines {
        if let Some(rest) = line.strip_prefix("rule ") {
            name = rest.trim().to_string();
            continue;
        }
        if line == "when" {
            in_when = true;
            in_then = false;
            continue;
        }
        if line == "then" {
            in_when = false;
            in_then = true;
            continue;
        }

        if in_when {
            if let Some(rest) = line.strip_prefix("extension == ") {
                conditions.push(Condition::Extension(rest.trim_matches('"').to_string()));
            } else if let Some(rest) = line.strip_prefix("media_type == ") {
                conditions.push(Condition::MediaType(rest.trim_matches('"').to_string()));
            }
            continue;
        }

        if in_then {
            if let Some((k, v)) = line.split_once('=') {
                kv.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    if name.is_empty() {
        bail!("rule name missing");
    }

    let mut actions = Vec::new();
    if let Some(v) = kv.get("handler") {
        actions.push(Action::Handler(clean(v)));
    }
    if let Some(v) = kv.get("extract") {
        actions.push(Action::Extract(parse_list(v)));
    }
    if let Some(v) = kv.get("embed") {
        actions.push(Action::Embed(parse_list(v)));
    }
    if let Some(v) = kv.get("ontology") {
        actions.push(Action::Ontology(clean(v)));
    }
    if let Some(v) = kv.get("bucket") {
        actions.push(Action::Bucket(clean(v)));
    }
    if let Some(v) = kv.get("prefix") {
        actions.push(Action::Prefix(clean(v)));
    }
    if let Some(v) = kv.get("filename") {
        actions.push(Action::Filename(clean(v)));
    }

    Ok(Rule {
        name,
        when_clause: WhenClause { conditions },
        then_clause: ThenClause { actions },
    })
}

fn clean(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn parse_list(s: &str) -> Vec<String> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(clean)
        .collect()
}
