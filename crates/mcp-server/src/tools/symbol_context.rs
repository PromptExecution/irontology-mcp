use anyhow::Result;
use serde_json::Value;
use storage_neumann::{EdgeRecord, FactRecord, KnowledgeStore, SemanticQuery, SymbolRecord};

#[derive(Debug, Clone)]
pub struct SymbolContext {
    pub symbols: Vec<SymbolRecord>,
    pub facts: Vec<FactRecord>,
    pub edges: Vec<EdgeRecord>,
}

pub async fn resolve_symbol_context(
    store: &dyn KnowledgeStore,
    id: &str,
    expand: bool,
) -> Result<SymbolContext> {
    let symbols = store
        .query(SemanticQuery::Symbols {
            id: Some(id.to_string()),
            path: None,
            name: None,
            kind: None,
        })
        .await?
        .symbols;

    let facts = store
        .query(SemanticQuery::Facts {
            subject: Some(id.to_string()),
            predicate: None,
        })
        .await?
        .facts;

    let edges = if expand {
        store
            .query(SemanticQuery::Edges {
                from: Some(id.to_string()),
                kind: None,
            })
            .await?
            .edges
    } else {
        Vec::new()
    };

    Ok(SymbolContext { symbols, facts, edges })
}

pub fn fact_text(facts: &[FactRecord], candidates: &[&str]) -> Option<String> {
    facts
        .iter()
        .find(|fact| candidates.contains(&fact.predicate.as_str()))
        .and_then(|fact| fact.object.as_str().map(ToOwned::to_owned).or_else(|| {
            if fact.object.is_null() {
                None
            } else {
                Some(fact.object.to_string())
            }
        }))
}

pub fn facts_json(facts: &[FactRecord]) -> Vec<Value> {
    facts
        .iter()
        .map(|fact| {
            serde_json::json!({
                "subject": fact.subject,
                "predicate": fact.predicate,
                "object": fact.object,
            })
        })
        .collect()
}

pub fn edges_json(edges: &[EdgeRecord]) -> Vec<Value> {
    edges
        .iter()
        .map(|edge| {
            serde_json::json!({
                "from": edge.from,
                "to": edge.to,
                "kind": edge.kind,
                "weight": edge.weight,
            })
        })
        .collect()
}
