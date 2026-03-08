use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeUri(String);

impl NodeUri {
    pub fn new(blob_id: &str, symbol: &str) -> Self {
        Self(format!("git:blob:{blob_id}:{symbol}"))
    }
}

impl std::fmt::Display for NodeUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Type,
    Module,
    Test,
    Doc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolNode {
    pub id: NodeUri,
    pub name: String,
    pub kind: SymbolKind,
    pub doctext: Option<String>,
    pub span: Span,
    pub signature: Option<String>,
}
