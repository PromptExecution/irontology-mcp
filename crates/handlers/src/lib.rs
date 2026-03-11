use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeFile {
    pub sha256: [u8; 32],
    pub bytes: Bytes,
    pub path_hint: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandlerScore(pub f32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalValue {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyValue {
    pub label: String,
    pub amount_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extraction {
    pub detected_kind: String,
    pub text: Option<String>,
    pub fields: BTreeMap<String, Value>,
    pub dates: Vec<TemporalValue>,
    pub amounts: Vec<MoneyValue>,
    pub entities: Vec<Entity>,
}

#[async_trait]
pub trait FileHandler: Send + Sync {
    fn name(&self) -> &str;
    fn score(&self, file: &IntakeFile) -> HandlerScore;
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction>;
}

pub struct HandlerRegistry {
    handlers: Vec<Arc<dyn FileHandler>>,
}

impl HandlerRegistry {
    pub fn new(handlers: Vec<Arc<dyn FileHandler>>) -> Self {
        Self { handlers }
    }

    pub fn select(&self, file: &IntakeFile) -> Option<Arc<dyn FileHandler>> {
        self.handlers
            .iter()
            .max_by(|lhs, rhs| {
                lhs.score(file)
                    .0
                    .partial_cmp(&rhs.score(file).0)
                    .unwrap_or(Ordering::Equal)
            })
            .cloned()
    }
}

#[async_trait]
pub trait McpExtractorClient: Send + Sync {
    async fn extract(&self, target: &str, file: &IntakeFile) -> Result<Extraction>;
}

pub struct McpHandler {
    name: String,
    target: String,
    min_score: f32,
    client: Arc<dyn McpExtractorClient>,
}

impl McpHandler {
    pub fn new(
        name: impl Into<String>,
        target: impl Into<String>,
        min_score: f32,
        client: Arc<dyn McpExtractorClient>,
    ) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            min_score,
            client,
        }
    }
}

#[async_trait]
impl FileHandler for McpHandler {
    fn name(&self) -> &str {
        &self.name
    }

    fn score(&self, file: &IntakeFile) -> HandlerScore {
        if file.media_type.as_deref() == Some("application/pdf") {
            HandlerScore(self.min_score)
        } else {
            HandlerScore(0.0)
        }
    }

    async fn extract(&self, file: &IntakeFile) -> Result<Extraction> {
        self.client.extract(&self.target, file).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use async_trait::async_trait;
    use bytes::Bytes;
    use serde_json::json;

    use crate::{
        Entity, Extraction, FileHandler, HandlerRegistry, HandlerScore, IntakeFile,
        McpExtractorClient, McpHandler, MoneyValue, TemporalValue,
    };

    struct StaticHandler {
        name: &'static str,
        score: f32,
    }

    #[async_trait]
    impl FileHandler for StaticHandler {
        fn name(&self) -> &str {
            self.name
        }

        fn score(&self, _file: &IntakeFile) -> HandlerScore {
            HandlerScore(self.score)
        }

        async fn extract(&self, _file: &IntakeFile) -> Result<Extraction> {
            Ok(sample_extraction())
        }
    }

    struct ClientProbe {
        seen: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl McpExtractorClient for ClientProbe {
        async fn extract(&self, target: &str, _file: &IntakeFile) -> Result<Extraction> {
            self.seen.lock().expect("seen").push(target.to_string());
            Ok(sample_extraction())
        }
    }

    #[tokio::test]
    async fn registry_selects_highest_scoring_handler() {
        let registry = HandlerRegistry::new(vec![
            Arc::new(StaticHandler {
                name: "generic",
                score: 0.1,
            }),
            Arc::new(StaticHandler {
                name: "pdf_text",
                score: 0.9,
            }),
        ]);

        let selected = registry.select(&sample_file()).expect("select");
        assert_eq!(selected.name(), "pdf_text");
    }

    #[tokio::test]
    async fn mcp_handler_delegates_extraction() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler = McpHandler::new(
            "pdf_mcp",
            "stdio://child:python-worker",
            0.8,
            Arc::new(ClientProbe { seen: seen.clone() }),
        );

        let extraction = handler.extract(&sample_file()).await.expect("extract");
        assert_eq!(extraction.detected_kind, "receipt");
        assert_eq!(
            seen.lock().expect("seen").as_slice(),
            &["stdio://child:python-worker".to_string()]
        );
    }

    fn sample_file() -> IntakeFile {
        IntakeFile {
            sha256: [7; 32],
            bytes: Bytes::from_static(b"fake pdf"),
            path_hint: Some("receipt.pdf".to_string()),
            media_type: Some("application/pdf".to_string()),
        }
    }

    fn sample_extraction() -> Extraction {
        Extraction {
            detected_kind: "receipt".to_string(),
            text: Some("Officeworks total 148.95".to_string()),
            fields: std::collections::BTreeMap::from([
                ("vendor".to_string(), json!("Officeworks")),
                ("currency".to_string(), json!("AUD")),
            ]),
            dates: vec![TemporalValue {
                label: "issue_date".to_string(),
                value: "2026-03-07".to_string(),
            }],
            amounts: vec![MoneyValue {
                label: "total".to_string(),
                amount_minor: 14895,
                currency: "AUD".to_string(),
            }],
            entities: vec![Entity {
                kind: "vendor".to_string(),
                value: "Officeworks".to_string(),
            }],
        }
    }
}
