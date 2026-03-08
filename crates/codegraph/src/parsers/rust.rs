use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Query, QueryCursor};

use crate::{EdgeKind, NodeUri, Span, SymbolGraph, SymbolKind, SymbolNode};

pub fn build_rust_graph(blob_id: &str, source: &str) -> Result<SymbolGraph> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .map_err(|e| anyhow!("failed to set rust grammar: {e}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("failed to parse source"))?;

    let mut graph = SymbolGraph::default();

    let fn_query = Query::new(
        tree_sitter_rust::language(),
        "(function_item name: (identifier) @fn.name) @fn.item",
    )?;
    let mut cursor = QueryCursor::new();
    let mut name_to_idx = std::collections::HashMap::new();
    let mut function_spans: Vec<(usize, usize, String)> = Vec::new();

    for m in cursor.matches(&fn_query, tree.root_node(), source.as_bytes()) {
        let mut fn_name = None;
        let mut fn_item_node = None;
        for capture in m.captures {
            match fn_query.capture_names()[capture.index as usize].as_str() {
                "fn.name" => fn_name = Some(&source[capture.node.byte_range()]),
                "fn.item" => fn_item_node = Some(capture.node),
                _ => {}
            }
        }

        if let (Some(name), Some(item_node)) = (fn_name, fn_item_node) {
            let symbol = SymbolNode {
                id: NodeUri::new(blob_id, name),
                name: name.to_string(),
                kind: SymbolKind::Function,
                doctext: None,
                span: Span {
                    start_line: item_node.start_position().row + 1,
                    end_line: item_node.end_position().row + 1,
                },
                signature: None,
            };
            let idx = graph.add_node(symbol);
            name_to_idx.insert(name.to_string(), idx);
            function_spans.push((
                item_node.start_position().row + 1,
                item_node.end_position().row + 1,
                name.to_string(),
            ));
        }
    }

    let import_query = Query::new(tree_sitter_rust::language(), "(use_declaration) @imp")?;
    let mut cursor = QueryCursor::new();
    let imports: Vec<_> = cursor
        .matches(&import_query, tree.root_node(), source.as_bytes())
        .flat_map(|m| m.captures)
        .filter(|c| import_query.capture_names()[c.index as usize] == "imp")
        .collect();

    if !imports.is_empty() {
        for fidx in name_to_idx.values().copied() {
            for imp in &imports {
                let imp_name = source[imp.node.byte_range()].trim();
                let imp_node = SymbolNode {
                    id: NodeUri::new(blob_id, imp_name),
                    name: imp_name.to_string(),
                    kind: SymbolKind::Module,
                    doctext: None,
                    span: Span {
                        start_line: imp.node.start_position().row + 1,
                        end_line: imp.node.end_position().row + 1,
                    },
                    signature: None,
                };
                let iidx = graph.add_node(imp_node);
                graph.add_edge(fidx, iidx, EdgeKind::Imports);
            }
        }
    }

    let call_query = Query::new(
        tree_sitter_rust::language(),
        "(call_expression function: (identifier) @callee)",
    )?;
    let mut cursor = QueryCursor::new();
    for m in cursor.matches(&call_query, tree.root_node(), source.as_bytes()) {
        for cap in m.captures {
            if call_query.capture_names()[cap.index as usize] != "callee" {
                continue;
            }

            let callee = &source[cap.node.byte_range()];
            let Some(&to) = name_to_idx.get(callee) else {
                continue;
            };

            let call_row = cap.node.start_position().row + 1;
            let maybe_from = function_spans
                .iter()
                .find(|(start, end, _)| *start <= call_row && *end >= call_row)
                .and_then(|(_, _, name)| name_to_idx.get(name).copied());

            if let Some(from) = maybe_from {
                graph.add_edge(from, to, EdgeKind::Calls);
            }
        }
    }

    Ok(graph)
}
