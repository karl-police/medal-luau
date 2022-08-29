use std::collections::HashSet;
use cfg::{dot, function::Function};
use fxhash::FxHashMap;
use itertools::Itertools;
use cfg::block::BasicBlock;
use graph::{algorithms::*, Directed, Edge, Graph, NodeId};

use petgraph::{visit::*, stable_graph::{StableDiGraph, NodeIndex}, algo::dominators::simple_fast};

mod compound;
mod conditional;
mod jump;
mod r#loop;

struct GraphStructurer {
    pub function: Function,
    root: NodeIndex,
    back_edges: Vec<Edge>,
}

impl GraphStructurer {
    fn new(
        function: Function,
        graph: StableDiGraph<BasicBlock, ()>,
        blocks: FxHashMap<NodeIndex, ast::Block>,
        root: NodeIndex,
    ) -> Self {
        pub fn back_edges(graph: &StableDiGraph<BasicBlock, ()>, root: NodeIndex) -> Vec<Edge> {
            let mut back_edges = Vec::new();
            let dominators = simple_fast(graph, root);

            for node in graph.node_indices() {
                /*for successor in graph.successors(node) {
                    if dominators.contains(&successor) {
                        back_edges.push((node, successor));
                    }
                }*/
            }

            back_edges
        }

        let back_edges = back_edges(&graph, root);
        let root = function.entry().unwrap();

        Self {
            function,
            root,
            back_edges,
        }
    }

    fn block_is_no_op(block: &ast::Block) -> bool {
        block
            .iter()
            .filter(|stmt| stmt.as_comment().is_some())
            .count()
            == block.len()
    }

    fn try_match_pattern(&mut self, node: NodeIndex) -> bool {
        let successors = self.function.successor_blocks(node).collect_vec();

        /*if self.try_collapse_loop(node) {
            return true;
        }*/

        let changed = match successors.len() {
            0 => false,
            1 => {
                // remove unnecessary jumps to allow pattern matching
                self.match_jump(node, successors[0])
            }
            2 => {
                let (then_edge, else_edge) = self
                    .function
                    .block(node)
                    .unwrap()
                    .terminator
                    .as_ref()
                    .unwrap()
                    .as_conditional()
                    .unwrap();
                let (then_node, else_node) = (then_edge.node, else_edge.node);
                self.match_compound_conditional(node, then_node, else_node)
                //|| self.match_conditional(node, then_node, else_node)
            }

            _ => unreachable!(),
        };

        //dot::render_to(&self.function, &mut std::io::stdout());

        changed
    }

    fn match_blocks(&mut self) -> bool {
        let dfs = {
            let mut dfs = Dfs::new(self.function.graph(), self.root);
            let mut result = HashSet::new();

            while let Some(n) = dfs.next(self.function.graph()) {
                result.insert(n);
            }

            result
        };
        let mut dfs_postorder = DfsPostOrder::new(self.function.graph(), self.root);

        for node in self
            .function
            .graph()
            .node_indices()
            .filter(|node| !dfs.contains(node))
            .collect_vec()
        {
            self.function.remove_block(node);
        }

        let mut changed = false;
        while let Some(node) = dfs_postorder.next(self.function.graph()) {
            println!("matching {:?}", node);
            changed |= self.try_match_pattern(node);
        }

        cfg::dot::render_to(&self.function, &mut std::io::stdout()).unwrap();

        changed
    }

    fn collapse(&mut self) {
        while self.match_blocks() {}

        let nodes = self.function.graph().node_count();
        if self.function.graph().node_count() != 1 {
            println!("failed to collapse! total nodes: {}", nodes);
        }
    }

    fn structure(mut self) -> ast::Block {
        self.collapse();
        self.function.remove_block(self.root).unwrap().ast
    }
}

pub fn lift(function: cfg::function::Function) -> ast::Block {
    let graph = function.graph().clone();
    let root = function.entry().unwrap();

    //dot::render_to(&graph, &mut std::io::stdout());

    let blocks = function
        .blocks()
        .map(|(node, block)| (node, block.ast.clone()))
        .collect();

    let structurer = GraphStructurer::new(function, graph, blocks, root);
    structurer.structure()
}
