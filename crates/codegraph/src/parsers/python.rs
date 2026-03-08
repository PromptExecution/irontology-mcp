use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Query, QueryCursor};

use crate::{NodeUri, Span, SymbolGraph, SymbolKind, SymbolNode};

pub fn build_python_graph(blob_id: &str, source: &str) -> Result<SymbolGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::language())
        .map_err(|e| anyhow!("failed to set python grammar: {e}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("failed to parse source"))?;

    let mut graph = SymbolGraph::default();
    let query = Query::new(
        tree_sitter_python::language(),
        "(function_definition name: (identifier) @fn.name)",
    )?;
    let mut cursor = QueryCursor::new();
    for m in cursor.matches(&query, tree.root_node(), source.as_bytes()) {
        for cap in m.captures {
            if query.capture_names()[cap.index as usize] == "fn.name" {
                let node = cap.node;
                let name = &source[node.byte_range()];
                graph.add_node(SymbolNode {
                    id: NodeUri::new(blob_id, name),
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    doctext: None,
                    span: Span {
                        start_line: node.start_position().row + 1,
                        end_line: node.end_position().row + 1,
                    },
                    signature: None,
                });
            }
        }
    }
    Ok(graph)
}
