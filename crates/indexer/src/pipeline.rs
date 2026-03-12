use std::{collections::BTreeMap, path::Path};

use anyhow::Result;
use async_trait::async_trait;
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::json;
use storage_neumann::{EmbeddingRecord, FactRecord, FileRecord, KnowledgeStore};

use crate::{chunking::chunk_text, embedding::Modality};

#[derive(Debug, Clone)]
pub struct IntakeFile {
    pub path: String,
    pub extension: String,
    pub media_type: String,
    pub fields: Vec<String>,
    pub class: Option<String>,
    pub shape: Option<String>,
    pub source_id: Option<String>,
    pub source_kind: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub ontology_refs: Vec<String>,
}

impl IntakeFile {
    pub fn from_path(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}"))
            .unwrap_or_default();

        Self {
            path: path.display().to_string(),
            media_type: infer_media_type(&extension).to_string(),
            extension,
            fields: vec![],
            class: None,
            shape: None,
            source_id: None,
            source_kind: None,
            tags: BTreeMap::new(),
            ontology_refs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Extraction {
    pub text: String,
    pub has_symbols: bool,
}

#[async_trait]
pub trait GitLedger: Send + Sync {
    async fn blob_id(&self, path: &Path) -> Result<String>;
}

pub trait RuleMatcher: Send + Sync {
    fn match_file(&self, file: &IntakeFile) -> bool;
}

#[async_trait]
pub trait Handler: Send + Sync {
    async fn extract(&self, file: &IntakeFile) -> Result<Extraction>;
}

pub async fn index_file(
    path: &Path,
    git_ledger: &dyn GitLedger,
    rules: &dyn RuleMatcher,
    handler: &dyn Handler,
    store: &dyn KnowledgeStore,
    provider: &dyn ModelProvider,
) -> Result<bool> {
    index_intake_file(
        path,
        IntakeFile::from_path(path),
        git_ledger,
        rules,
        handler,
        store,
        provider,
    )
    .await
}

pub async fn index_intake_file(
    path: &Path,
    intake: IntakeFile,
    git_ledger: &dyn GitLedger,
    rules: &dyn RuleMatcher,
    handler: &dyn Handler,
    store: &dyn KnowledgeStore,
    provider: &dyn ModelProvider,
) -> Result<bool> {
    let blob_id = git_ledger.blob_id(path).await?;
    if store.has_blob(&blob_id).await? {
        return Ok(false);
    }

    if !rules.match_file(&intake) {
        return Ok(false);
    }

    let extraction = handler.extract(&intake).await?;
    let chunks = chunk_text(&extraction.text, 512);
    if chunks.is_empty() {
        return Ok(false);
    }

    let file_id = format!("file:git:blob:{blob_id}");
    store
        .upsert_file(FileRecord {
            id: file_id.clone(),
            blob: blob_id.clone(),
            path: intake.path.clone(),
            media_type: intake.media_type.clone(),
            size: std::fs::metadata(path)
                .map(|meta| meta.len())
                .unwrap_or_default(),
            commit: blob_id.clone(),
        })
        .await?;
    let mut facts = vec![
        FactRecord {
            subject: file_id.clone(),
            predicate: "path".to_string(),
            object: json!(intake.path),
        },
        FactRecord {
            subject: file_id.clone(),
            predicate: "media_type".to_string(),
            object: json!(intake.media_type),
        },
        FactRecord {
            subject: file_id.clone(),
            predicate: "extension".to_string(),
            object: json!(intake.extension),
        },
    ];
    if let Some(class) = &intake.class {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "class".to_string(),
            object: json!(class),
        });
    }
    if let Some(shape) = &intake.shape {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "shape".to_string(),
            object: json!(shape),
        });
    }
    if let Some(source_id) = &intake.source_id {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "source_id".to_string(),
            object: json!(source_id),
        });
    }
    if let Some(source_kind) = &intake.source_kind {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "source_kind".to_string(),
            object: json!(source_kind),
        });
    }
    for (key, value) in &intake.tags {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: format!("tag:{key}"),
            object: json!(value),
        });
    }
    for ontology_ref in &intake.ontology_refs {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "ontology_ref".to_string(),
            object: json!(ontology_ref),
        });
    }
    store.upsert_facts(facts).await?;

    let embeddings = provider
        .embed(EmbedRequest {
            model: provider.model_id().to_string(),
            inputs: chunks,
            batch_size: 32,
        })
        .await?;
    let mut records = Vec::new();
    for (index, vector) in embeddings.vectors.into_iter().enumerate() {
        records.push(EmbeddingRecord {
            id: format!("{file_id}#chunk-{index}"),
            source_blob: blob_id.clone(),
            vector,
            modality: if extraction.has_symbols {
                Modality::CodeSymbol
            } else {
                Modality::DocChunk
            },
            semantic_weight: 1.0,
        });
    }
    store.upsert_embeddings(records).await?;
    Ok(true)
}

fn infer_media_type(extension: &str) -> &'static str {
    match extension {
        ".csv" => "text/csv",
        ".json" => "application/json",
        ".pdf" => "application/pdf",
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".rs" | ".py" | ".toml" | ".md" | ".txt" | ".yaml" | ".yml" => "text/plain",
        _ => "",
    }
}
