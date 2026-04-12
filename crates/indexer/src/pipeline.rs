use std::{collections::BTreeMap, path::Path};

use anyhow::Result;
use async_trait::async_trait;
use codegraph::{
    extractors::{extract as extract_symbol_graph, Language},
    EdgeKind as CodeEdgeKind, SymbolGraph, SymbolKind, SymbolNode,
};
use domain::{Claim, Relation};
use provider_api::{EmbedRequest, ModelProvider};
use serde_json::json;
use storage_neumann::{
    EdgeKind, EdgeRecord, EmbeddingRecord, FactRecord, FileRecord, KnowledgeStore, SymbolRecord,
};

use crate::{
    chunking::chunk_structured,
    distillation::distill_chunks,
    embedding::Modality,
};

const SYNTHETIC_MODULE_NAME: &str = "__module__";

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
    pub fields: BTreeMap<String, serde_json::Value>,
    pub class: Option<String>,
    pub shape: Option<String>,
    pub claims: Vec<Claim>,
    pub relations: Vec<Relation>,
    pub notes: Vec<String>,
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

    let file_id = format!("file:git:blob:{blob_id}");
    let effective_class = intake.class.clone().or_else(|| extraction.class.clone());
    let effective_shape = intake.shape.clone().or_else(|| extraction.shape.clone());
    store
        .upsert_file(FileRecord {
            id: file_id.clone(),
            blob: blob_id.clone(),
            path: intake.path.clone(),
            media_type: intake.media_type.clone(),
            size: tokio::fs::metadata(path)
                .await
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
    if let Some(class) = &effective_class {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "class".to_string(),
            object: json!(class),
        });
    }
    if let Some(shape) = &effective_shape {
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
    for (key, value) in &extraction.fields {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: format!("field:{key}"),
            object: value.clone(),
        });
    }
    for note in &extraction.notes {
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: "semantic_note".to_string(),
            object: json!(note),
        });
    }
    for claim in &extraction.claims {
        facts.push(FactRecord {
            subject: claim.subject.clone(),
            predicate: claim.predicate.clone(),
            object: json!(claim.object),
        });
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: format!("claim:{}", claim.predicate),
            object: json!({
                "id": claim.id,
                "subject": claim.subject,
                "object": claim.object,
                "namespace": claim.namespace,
                "confidence": claim.confidence,
                "evidence": claim.evidence,
            }),
        });
    }
    let mut edges = Vec::new();
    for relation in &extraction.relations {
        edges.push(EdgeRecord {
            from: relation.subject_id.clone(),
            to: relation.object_id.clone(),
            kind: EdgeKind::Related,
            weight: ((relation.confidence.max(0.0) * 100.0).round() as u32).max(1),
        });
        facts.push(FactRecord {
            subject: file_id.clone(),
            predicate: format!("relation:{}", relation.predicate),
            object: json!({
                "id": relation.id,
                "from": relation.subject_id,
                "to": relation.object_id,
                "namespace": relation.namespace,
                "confidence": relation.confidence,
                "evidence": relation.evidence,
            }),
        });
    }

    let symbol_graph =
        extract_symbol_graph_for_intake(&intake.extension, &blob_id, &extraction.text);
    let symbol_nodes = if let Some(graph) = symbol_graph.as_ref() {
        persist_symbol_graph(&file_id, &intake.path, &blob_id, &extraction.text, graph, store)
            .await?
    } else {
        Vec::new()
    };

    let (embedding_inputs, embedding_modality) = if !symbol_nodes.is_empty() {
        (
            symbol_nodes
                .iter()
                .map(|node| symbol_text_for_embedding(&extraction.text, node))
                .collect::<Vec<_>>(),
            Modality::CodeSymbol,
        )
    } else {
        let modality = fallback_modality(&intake.extension, extraction.has_symbols);
        let structured = chunk_structured(&extraction.text, 512);
        if structured.is_empty() {
            return Ok(false);
        }

        // Store section locator and heading as facts for each chunk.
        for (i, chunk) in structured.iter().enumerate() {
            facts.push(FactRecord {
                subject: format!("{file_id}#chunk-{i}"),
                predicate: "section_locator".to_string(),
                object: json!(chunk.locator),
            });
            if let Some(heading) = &chunk.heading {
                facts.push(FactRecord {
                    subject: format!("{file_id}#chunk-{i}"),
                    predicate: "section_heading".to_string(),
                    object: json!(heading),
                });
            }
        }

        // Gap 2: Distillation — only for non-code document chunks.
        if modality == Modality::DocChunk {
            let chunk_texts: Vec<String> = structured.iter().map(|c| c.text.clone()).collect();
            let summaries = distill_chunks(&chunk_texts, provider).await?;
            for (i, summary) in summaries.into_iter().enumerate() {
                if !summary.is_empty() {
                    facts.push(FactRecord {
                        subject: format!("{file_id}#chunk-{i}"),
                        predicate: "semantic_note".to_string(),
                        object: json!(summary),
                    });
                }
            }
        }

        let chunks: Vec<String> = structured.into_iter().map(|c| c.text).collect();
        (chunks, modality)
    };

    store.upsert_facts(facts).await?;
    if !edges.is_empty() {
        store.upsert_edges(edges).await?;
    }

    let embeddings = provider
        .embed(EmbedRequest {
            model: provider.model_id().to_string(),
            inputs: embedding_inputs,
            batch_size: 32,
        })
        .await?;
    let mut records = Vec::new();
    if !symbol_nodes.is_empty() {
        for (symbol, vector) in symbol_nodes.into_iter().zip(embeddings.vectors.into_iter()) {
            records.push(EmbeddingRecord {
                id: symbol.id.to_string(),
                source_blob: blob_id.clone(),
                vector,
                modality: Modality::CodeSymbol,
                semantic_weight: 1.0,
            });
        }
        store.upsert_embeddings(records).await?;
        return Ok(true);
    }
    for (index, vector) in embeddings.vectors.into_iter().enumerate() {
        records.push(EmbeddingRecord {
            id: format!("{file_id}#chunk-{index}"),
            source_blob: blob_id.clone(),
            vector,
            modality: embedding_modality,
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

fn fallback_modality(extension: &str, has_symbols: bool) -> Modality {
    if code_language_for_extension(extension).is_some() || has_symbols {
        Modality::CodeSymbol
    } else {
        Modality::DocChunk
    }
}

fn code_language_for_extension(extension: &str) -> Option<Language> {
    match extension {
        ".rs" => Some(Language::Rust),
        ".py" => Some(Language::Python),
        _ => None,
    }
}

fn extract_symbol_graph_for_intake(
    extension: &str,
    blob_id: &str,
    source: &str,
) -> Option<SymbolGraph> {
    let language = code_language_for_extension(extension)?;
    extract_symbol_graph(language, blob_id, source).ok()
}

async fn persist_symbol_graph(
    file_id: &str,
    path: &str,
    blob_id: &str,
    source: &str,
    graph: &SymbolGraph,
    store: &dyn KnowledgeStore,
) -> Result<Vec<SymbolNode>> {
    let mut symbol_nodes = Vec::new();
    let mut symbols = Vec::new();
    let mut facts = Vec::new();
    let mut edges = Vec::new();

    for node in graph.nodes() {
        if !should_embed_symbol(node) {
            continue;
        }

        let content = symbol_text_for_embedding(source, node);
        symbol_nodes.push(node.clone());
        symbols.push(SymbolRecord {
            id: node.id.to_string(),
            blob: blob_id.to_string(),
            path: path.to_string(),
            name: node.name.clone(),
            kind: symbol_kind_label(&node.kind).to_string(),
            start_line: node.span.start_line,
            end_line: node.span.end_line,
            signature: node.signature.clone(),
            content: content.clone(),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "symbol_name".to_string(),
            object: json!(node.name),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "symbol_kind".to_string(),
            object: json!(symbol_kind_label(&node.kind)),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "symbol_span_start_line".to_string(),
            object: json!(node.span.start_line),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "symbol_span_end_line".to_string(),
            object: json!(node.span.end_line),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "path".to_string(),
            object: json!(path),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "location".to_string(),
            object: json!(format!("{path}:{}-{}", node.span.start_line, node.span.end_line)),
        });
        facts.push(FactRecord {
            subject: node.id.to_string(),
            predicate: "content".to_string(),
            object: json!(content),
        });
        if let Some(signature) = &node.signature {
            facts.push(FactRecord {
                subject: node.id.to_string(),
                predicate: "symbol_signature".to_string(),
                object: json!(signature),
            });
        }
        edges.push(EdgeRecord {
            from: file_id.to_string(),
            to: node.id.to_string(),
            kind: EdgeKind::Defines,
            weight: 100,
        });
    }

    for (from, to, kind) in graph.edge_refs() {
        match kind {
            CodeEdgeKind::Calls => edges.push(EdgeRecord {
                from: from.id.to_string(),
                to: to.id.to_string(),
                kind: EdgeKind::Calls,
                weight: 100,
            }),
            CodeEdgeKind::Tests => edges.push(EdgeRecord {
                from: from.id.to_string(),
                to: to.id.to_string(),
                kind: EdgeKind::Tests,
                weight: 100,
            }),
            CodeEdgeKind::Imports if from.name == SYNTHETIC_MODULE_NAME => edges.push(EdgeRecord {
                from: file_id.to_string(),
                to: to.id.to_string(),
                kind: EdgeKind::DependsOn,
                weight: 100,
            }),
            _ => {}
        }
    }

    if !facts.is_empty() {
        store.upsert_facts(facts).await?;
    }
    if !symbols.is_empty() {
        store.upsert_symbols(symbols).await?;
    }
    if !edges.is_empty() {
        store.upsert_edges(edges).await?;
    }

    Ok(symbol_nodes)
}

fn should_embed_symbol(node: &SymbolNode) -> bool {
    matches!(
        node.kind,
        SymbolKind::Function | SymbolKind::Type | SymbolKind::Test
    )
}

fn symbol_text_for_embedding(source: &str, node: &SymbolNode) -> String {
    let mut out = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        if line_no < node.span.start_line {
            continue;
        }
        if line_no > node.span.end_line {
            break;
        }
        out.push(line);
    }

    let text = out.join("\n");
    if text.trim().is_empty() {
        node.name.clone()
    } else {
        text
    }
}

fn symbol_kind_label(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "Function",
        SymbolKind::Type => "Type",
        SymbolKind::Module => "Module",
        SymbolKind::Test => "Test",
        SymbolKind::Doc => "Doc",
    }
}
