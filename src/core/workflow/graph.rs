//! Workflow execution graph.
//!
//! [`WorkflowGraph`] is a directed acyclic graph (DAG) whose nodes are
//! [`WorkflowNode`]s and whose edges represent "must complete before"
//! relationships.
//!
//! # Construction
//! 1. The [`DependencyResolver`] validates the registry and builds the
//!    graph in topological order.
//! 2. The [`WorkflowScheduler`] queries the graph for ready nodes.
//! 3. The [`WorkflowExecutor`] drives node state transitions.
//!
//! # Backing store
//! The graph uses [`petgraph::graph::DiGraph`] for the structural
//! representation and a flat `HashMap<NodeId, NodeIndex>` for O(1) node
//! lookup by skill ID.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::core::{
    skill::SkillRef,
    workflow::{
        node::{NodeId, WorkflowNode},
        state::NodeState,
    },
};

// ── Graph ─────────────────────────────────────────────────────────────────────

/// A directed acyclic graph of [`WorkflowNode`]s.
///
/// An edge `A → B` means "A must complete before B can start".
pub struct WorkflowGraph<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// The petgraph DiGraph holding all nodes.
    graph: DiGraph<WorkflowNode<I>, ()>,

    /// O(1) lookup: skill ID → petgraph `NodeIndex`.
    index_map: HashMap<String, NodeIndex>,
}

impl<I> WorkflowGraph<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Create an empty graph.
    pub(crate) fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a skill as a new node in the graph.
    ///
    /// Returns the `NodeIndex` assigned by petgraph.
    pub(crate) fn add_node(&mut self, skill: SkillRef<I>) -> NodeIndex {
        let id = skill.definition().id.to_string();
        let node = WorkflowNode::new(skill);
        let idx = self.graph.add_node(node);
        self.index_map.insert(id, idx);
        idx
    }

    /// Add a "must complete before" edge: `from` → `to`.
    ///
    /// Panics if either node ID is not in the graph.
    pub(crate) fn add_dependency(&mut self, from: NodeId, to: NodeId) {
        let from_idx = self.index_map[from];
        let to_idx = self.index_map[to];
        self.graph.add_edge(from_idx, to_idx, ());
    }

    // ── Node accessors ────────────────────────────────────────────────────────

    /// Lookup the `NodeIndex` for a skill ID.
    pub fn node_index(&self, id: &str) -> Option<NodeIndex> {
        self.index_map.get(id).copied()
    }

    /// Borrow the node for a given `NodeIndex`.
    pub fn node(&self, idx: NodeIndex) -> &WorkflowNode<I> {
        &self.graph[idx]
    }

    /// Mutably borrow the node for a given `NodeIndex`.
    pub fn node_mut(&mut self, idx: NodeIndex) -> &mut WorkflowNode<I> {
        &mut self.graph[idx]
    }

    /// Borrow a node by skill ID.
    pub fn node_by_id(&self, id: &str) -> Option<&WorkflowNode<I>> {
        let idx = self.index_map.get(id)?;
        Some(&self.graph[*idx])
    }

    /// Mutably borrow a node by skill ID.
    pub fn node_mut_by_id(&mut self, id: &str) -> Option<&mut WorkflowNode<I>> {
        let idx = *self.index_map.get(id)?;
        Some(&mut self.graph[idx])
    }

    // ── Graph queries ─────────────────────────────────────────────────────────

    /// Returns `NodeIndex`es of all nodes whose dependencies have all
    /// completed and whose own state is [`NodeState::Pending`].
    ///
    /// These are the nodes the scheduler should promote to `Ready`.
    pub fn pending_ready_nodes(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let node = &self.graph[idx];
                if node.state != NodeState::Pending {
                    return false;
                }
                // All incoming edges (dependencies) must point from a
                // Completed node.
                self.graph
                    .edges_directed(idx, petgraph::Direction::Incoming)
                    .all(|e| {
                        let src = &self.graph[e.source()];
                        src.state == NodeState::Completed
                    })
            })
            .collect()
    }

    /// Returns `NodeIndex`es of all nodes currently in the `Ready` state.
    pub fn ready_nodes(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].state == NodeState::Ready)
            .collect()
    }

    /// Returns `true` when every node has reached a terminal state.
    pub fn is_complete(&self) -> bool {
        self.graph
            .node_indices()
            .all(|idx| self.graph[idx].state.is_terminal())
    }

    /// Returns `true` when at least one node has failed.
    pub fn has_failures(&self) -> bool {
        self.graph
            .node_indices()
            .any(|idx| matches!(self.graph[idx].state, NodeState::Failed { .. }))
    }

    /// Returns the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of dependency edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Returns an iterator over all node indices.
    pub fn node_indices(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.graph.node_indices()
    }

    /// Transition the node identified by `id` to `new_state`.
    ///
    /// No-op if the ID is not found (logs a warning).
    pub fn transition(&mut self, id: &str, new_state: NodeState) {
        if let Some(&idx) = self.index_map.get(id) {
            self.graph[idx].transition(new_state);
        } else {
            tracing::warn!(node_id = id, "transition called on unknown node");
        }
    }

    /// Returns skill IDs for all nodes currently in the `Running` state.
    pub fn running_node_ids(&self) -> Vec<&str> {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].state == NodeState::Running)
            .map(|idx| self.graph[idx].id())
            .collect()
    }

    /// Count nodes in each terminal state for summary logging.
    pub fn completion_summary(&self) -> (usize, usize, usize) {
        let (mut done, mut failed, mut skipped) = (0, 0, 0);
        for idx in self.graph.node_indices() {
            match &self.graph[idx].state {
                NodeState::Completed => done += 1,
                NodeState::Failed { .. } => failed += 1,
                NodeState::Skipped => skipped += 1,
                _ => {}
            }
        }
        (done, failed, skipped)
    }
}

impl<I> std::fmt::Debug for WorkflowGraph<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowGraph")
            .field("nodes", &self.graph.node_count())
            .field("edges", &self.graph.edge_count())
            .finish()
    }
}
