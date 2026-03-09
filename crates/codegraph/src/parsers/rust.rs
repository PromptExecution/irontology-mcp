use std::collections::{HashMap, HashSet};

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

    let source_lines: Vec<_> = source.lines().collect();
    let mut graph = SymbolGraph::default();
    let mut name_to_idx = HashMap::new();
    let mut function_spans: Vec<(usize, usize, String)> = Vec::new();
    let mut test_functions = HashSet::new();

    let function_query = Query::new(
        &tree_sitter_rust::language(),
        "(function_item name: (identifier) @fn.name) @fn.item",
    )?;
    let mut cursor = QueryCursor::new();
    for m in cursor.matches(&function_query, tree.root_node(), source.as_bytes()) {
        let mut fn_name = None;
        let mut fn_item_node = None;
        for capture in m.captures {
            match function_query.capture_names()[capture.index as usize] {
                "fn.name" => fn_name = Some(&source[capture.node.byte_range()]),
                "fn.item" => fn_item_node = Some(capture.node),
                _ => {}
            }
        }

        if let (Some(name), Some(item_node)) = (fn_name, fn_item_node) {
            let kind = if has_test_attribute(&source_lines, item_node.start_position().row) {
                test_functions.insert(name.to_string());
                SymbolKind::Test
            } else {
                SymbolKind::Function
            };
            let symbol = SymbolNode {
                id: NodeUri::new(blob_id, name),
                name: name.to_string(),
                kind,
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

    let type_query = Query::new(
        &tree_sitter_rust::language(),
        r#"
(struct_item name: (type_identifier) @type.name) @type.item
(enum_item name: (type_identifier) @type.name) @type.item
(trait_item name: (type_identifier) @type.name) @type.item
"#,
    )?;
    let mut cursor = QueryCursor::new();
    for m in cursor.matches(&type_query, tree.root_node(), source.as_bytes()) {
        let mut type_name = None;
        let mut type_item_node = None;
        for capture in m.captures {
            match type_query.capture_names()[capture.index as usize] {
                "type.name" => type_name = Some(&source[capture.node.byte_range()]),
                "type.item" => type_item_node = Some(capture.node),
                _ => {}
            }
        }

        if let (Some(name), Some(item_node)) = (type_name, type_item_node) {
            graph.add_node(SymbolNode {
                id: NodeUri::new(blob_id, name),
                name: name.to_string(),
                kind: SymbolKind::Type,
                doctext: None,
                span: Span {
                    start_line: item_node.start_position().row + 1,
                    end_line: item_node.end_position().row + 1,
                },
                signature: None,
            });
        }
    }

    let import_query = Query::new(&tree_sitter_rust::language(), "(use_declaration) @imp")?;
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
        &tree_sitter_rust::language(),
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
                .map(|(_, _, name)| name.as_str())
                .and_then(|name| name_to_idx.get(name).copied().map(|idx| (name, idx)));

            if let Some((from_name, from)) = maybe_from {
                graph.add_edge(from, to, EdgeKind::Calls);
                if test_functions.contains(from_name) {
                    graph.add_edge(from, to, EdgeKind::Tests);
                }
            }
        }
    }

    Ok(graph)
}

fn has_test_attribute(source_lines: &[&str], function_row: usize) -> bool {
    let mut row = function_row;
    while row > 0 {
        let line = source_lines[row - 1].trim();
        if line.is_empty() {
            break;
        }
        if !line.starts_with("#[") {
            break;
        }
        if is_test_attribute(line) {
            return true;
        }
        row -= 1;
    }
    false
}

fn is_test_attribute(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("#[test]")
        || trimmed.starts_with("#[test(")
        || trimmed.contains("::test]")
        || trimmed.contains("::test(")
}
