use std::io::{BufRead, BufReader, Read};

use anyhow::Result;

pub fn load_tomllm<R: Read>(reader: R) -> Result<toml::Value> {
    let mut exec = String::new();
    for line in BufReader::new(reader).lines() {
        let line = line?;
        if !line.trim_start().starts_with('#') {
            exec.push_str(&line);
            exec.push('\n');
        }
    }
    Ok(toml::from_str(&exec)?)
}

#[cfg(test)]
pub fn load_tomllm_hints<R: Read>(reader: R) -> Result<Vec<String>> {
    let mut hints = Vec::new();
    for line in BufReader::new(reader).lines() {
        let line = line?;
        if line.trim_start().starts_with('#') {
            hints.push(line);
        }
    }
    Ok(hints)
}

#[cfg(test)]
mod tests {
    use super::{load_tomllm, load_tomllm_hints};

    #[test]
    fn strips_hints_and_parses_execution_layer() {
        let src = r#"# 🤓: secret answer key
title = "agent"
[index]
embed = true
"#;

        let parsed = load_tomllm(src.as_bytes()).expect("parse tomllm");
        assert_eq!(parsed["title"].as_str(), Some("agent"));
        assert_eq!(parsed["index"]["embed"].as_bool(), Some(true));
    }

    #[test]
    fn exposes_hints_only_to_test_harness() {
        let src = r#"# top secret
title = "agent"
"#;

        let hints = load_tomllm_hints(src.as_bytes()).expect("hints");
        assert_eq!(hints, vec!["# top secret".to_string()]);
    }

    #[test]
    fn round_trips_execution_layer() {
        let src = r#"# comment
[agent]
name = "repo_reasoner"
"#;

        let parsed = load_tomllm(src.as_bytes()).expect("parse");
        let serialized = toml::to_string(&parsed).expect("serialize");
        let reparsed = load_tomllm(serialized.as_bytes()).expect("reparse");
        assert_eq!(parsed, reparsed);
    }
}
