use std::collections::{HashMap, HashSet};

use petgraph::graph::{Graph, NodeIndex};

use crate::{NodeUri, SymbolNode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Calls,
    Imports,
    Defines,
    Tests,
    Implements,
    DependsOn,
}

#[derive(Default)]
pub struct SymbolGraph {
    graph: Graph<SymbolNode, EdgeKind>,
    indices: HashMap<NodeUri, NodeIndex>,
    edge_set: HashSet<(NodeIndex, NodeIndex, EdgeKind)>,
}

impl SymbolGraph {
    pub fn add_node(&mut self, node: SymbolNode) -> NodeIndex {
        if let Some(existing) = self.indices.get(&node.id) {
            return *existing;
        }
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.indices.insert(id, idx);
        idx
    }

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, kind: EdgeKind) {
        let key = (from, to, kind.clone());
        if self.edge_set.insert(key) {
            self.graph.add_edge(from, to, kind);
        }
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &SymbolNode> {
        self.graph.node_weights()
    }

    pub fn edges(&self) -> impl Iterator<Item = &EdgeKind> {
        self.graph.edge_weights()
    }
}
