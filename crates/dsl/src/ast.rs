use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub when_clause: WhenClause,
    pub then_clause: ThenClause,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhenClause {
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    Extension(String),
    MediaType(String),
    ContainsField(String),
    And(Box<Condition>, Box<Condition>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThenClause {
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Handler(String),
    Extract(Vec<String>),
    Embed(Vec<String>),
    Ontology(String),
    Bucket(String),
    Prefix(String),
    Filename(String),
}
