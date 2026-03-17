use anyhow::{anyhow, Result};
use lalrpop_util::lalrpop_mod;
use provider_api::{ChatMessage, ChatRequest, ModelProvider};

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

pub async fn compile_prompt(
    provider: &dyn ModelProvider,
    prompt: &str,
    model: Option<&str>,
) -> Result<Rule> {
    let response = provider
        .chat(ChatRequest {
            model: model.unwrap_or(provider.model_id()).to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "Compile the user request into a single valid DSL rule with no prose."
                        .to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
            ],
            max_tokens: Some(512),
            stream: false,
            params: Default::default(),
        })
        .await?;

    compile_rule(&response.content)
}
