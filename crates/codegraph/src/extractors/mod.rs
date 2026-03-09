use anyhow::Result;

use crate::{parsers, SymbolGraph};

pub enum Language {
    Rust,
    Python,
}

pub fn extract(language: Language, blob_id: &str, source: &str) -> Result<SymbolGraph> {
    match language {
        Language::Rust => parsers::rust::build_rust_graph(blob_id, source),
        Language::Python => parsers::python::build_python_graph(blob_id, source),
    }
}
