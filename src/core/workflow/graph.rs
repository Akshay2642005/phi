//! Workflow execution graph.
//!
//! [`WorkflowGraph`] is the **single abstraction over petgraph** in the
//! framework.  No other module imports or references petgraph types — all
//! graph operations go through this API.
//!
//! # Architecture
//!
//! ```text
//!  Planner ──► WorkflowGraph ──► Scheduler ──► Engine
//! ```
//!
//! The graph owns:
//! - The [`DiGraph`] backing store (private).
//! - The `NodeId → NodeIndex` lookup map (private).
//! - All dependency edges.
//! - All node execution state (delegated to [`WorkflowNode`]).
//!
//! # Responsibilities
//!
//! | Category | Methods |
//! |----------|---------|
//! | Construction | `new`, `add_node`, `add_dependency` |
//! | Lookup | `contains`, `get`, `skill`, `node_ids` |
//! | Traversal | `parents`, `children`, `roots`, `leaves` |
//! | Readiness | `ready_nodes` — the authoritative DAG-scheduling algorithm |
//! | Transitions | `transition`, `mark_running`, `mark_completed`, `mark_failed`, `mark_skipped` |
//! | Queries | `is_complete`, `all_completed`, `has_failures`, `running_count`, `pending_count`, `non_terminal_count`, `node_count`, `edge_count`, `completion_summary` |
//! | Validation | `detect_cycle` |
//! | Iteration | `iter` |
//!
//! # Invariants
//!
//! The graph guarantees:
//! - **No duplicate node IDs** — `add_node` asserts uniqueness.
//! - **No self-edges** — `add_dependency` asserts `from != to`.
//! - **Valid lookups** — all traversal methods handle missing IDs gracefully
//!   (return empty results rather than panicking).
//! - **Structural consistency** — cycle detection is available via
//!   [`detect_cycle`](WorkflowGraph::detect_cycle).
//!
//! # Generic parameter `I`
//!
//! `I` is the workflow domain input type.  The graph is generic so the
//! engine can run any workflow without modification.  The graph knows
//! **nothing** about AI, skills, or LLMs — it executes arbitrary DAGs.

use std::collections::HashMap;

use petgraph::Direction;
use petgraph::graph::DiGraph;
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
///
/// # petgraph encapsulation
///
/// No public or `pub(crate)` method accepts or returns [`petgraph`] types.
/// [`DiGraph`], [`NodeIndex`](petgraph::graph::NodeIndex), and
/// [`EdgeIndex`](petgraph::graph::EdgeIndex) are implementation details
/// confined to this module.
pub struct WorkflowGraph<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// The petgraph DiGraph holding all nodes — never exposed outside this file.
    graph: DiGraph<WorkflowNode<I>, ()>,

    /// O(1) lookup: skill ID → petgraph `NodeIndex`.
    ///
    /// Stored as `String` key because `&'static str` is not hashable with
    /// the `std::collections::HashMap` API in all cases.  The conversion
    /// happens once at insertion time.
    index_map: HashMap<String, petgraph::graph::NodeIndex>,
}

impl<I> WorkflowGraph<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    // ── Construction (crate-private) ───────────────────────────────────────────

    /// Create an empty graph.
    pub(crate) fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a skill as a new node in the graph.
    ///
    /// # Panics
    /// Panics if a node with the same [`NodeId`] already exists.
    /// This is a programming error — the planner must ensure unique IDs.
    pub(crate) fn add_node(&mut self, skill: SkillRef<I>) -> NodeId {
        let id = skill.definition().id;
        let key = id.to_string();

        assert!(
            !self.index_map.contains_key(&key),
            "duplicate node ID: {id}",
        );

        let node = WorkflowNode::new(skill);
        let idx = self.graph.add_node(node);
        self.index_map.insert(key, idx);
        id
    }

    /// Add a "must complete before" edge: `from` → `to`.
    ///
    /// # Panics
    /// - Panics if `from == to` (self-edges are invalid).
    /// - Panics if either ID is not in the graph.
    pub(crate) fn add_dependency(&mut self, from: NodeId, to: NodeId) {
        assert_ne!(from, to, "self-edge on node '{from}' is not allowed");

        let from_idx = self
            .index_map
            .get(from)
            .unwrap_or_else(|| panic!("add_dependency: source '{from}' not found"));
        let to_idx = self
            .index_map
            .get(to)
            .unwrap_or_else(|| panic!("add_dependency: target '{to}' not found"));

        self.graph.add_edge(*from_idx, *to_idx, ());
    }

    // ── Lookup ─────────────────────────────────────────────────────────────────

    /// Returns `true` if a node with the given ID exists in the graph.
    #[inline]
    pub fn contains(&self, id: &str) -> bool {
        self.index_map.contains_key(id)
    }

    /// Borrow a node by ID.
    #[inline]
    pub fn get(&self, id: &str) -> Option<&WorkflowNode<I>> {
        let idx = self.index_map.get(id)?;
        Some(&self.graph[*idx])
    }

    /// Mutably borrow a node by ID.
    ///
    /// # Visibility
    /// `pub(crate)` — intended for use by the scheduler and executor.
    /// External code observes nodes through the read-only [`Self::get`]
    /// accessor.
    #[inline]
    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut WorkflowNode<I>> {
        let idx = *self.index_map.get(id)?;
        Some(&mut self.graph[idx])
    }

    /// Convenience accessor: borrow the wrapped skill for a node.
    ///
    /// Returns `None` if the ID is not found.  The returned `SkillRef<I>`
    /// can be cheaply cloned via [`SkillRef::clone`] (an `Arc` increment).
    #[inline]
    pub fn skill(&self, id: &str) -> Option<&SkillRef<I>> {
        Some(self.get(id)?.skill())
    }

    // ── Traversal ──────────────────────────────────────────────────────────────

    /// Return the IDs of all **parent** nodes (direct dependencies) of `id`.
    ///
    /// Returns an empty `Vec` if the ID is not found or has no parents.
    pub fn parents(&self, id: &str) -> Vec<NodeId> {
        let idx = match self.index_map.get(id) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| self.graph[e.source()].id())
            .collect()
    }

    /// Return the IDs of all **child** nodes (direct dependants) of `id`.
    ///
    /// Returns an empty `Vec` if the ID is not found or has no children.
    pub fn children(&self, id: &str) -> Vec<NodeId> {
        let idx = match self.index_map.get(id) {
            Some(idx) => *idx,
            None => return Vec::new(),
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|e| self.graph[e.target()].id())
            .collect()
    }

    /// Return IDs of all **root** nodes (nodes with no incoming edges).
    ///
    /// Root nodes have no dependencies and can execute immediately once
    /// the workflow starts.
    pub fn roots(&self) -> Vec<NodeId> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                self.graph
                    .edges_directed(idx, Direction::Incoming)
                    .next()
                    .is_none()
            })
            .map(|idx| self.graph[idx].id())
            .collect()
    }

    /// Return IDs of all **leaf** nodes (nodes with no outgoing edges).
    ///
    /// Leaf nodes have no dependants.
    pub fn leaves(&self) -> Vec<NodeId> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                self.graph
                    .edges_directed(idx, Direction::Outgoing)
                    .next()
                    .is_none()
            })
            .map(|idx| self.graph[idx].id())
            .collect()
    }

    // ── Readiness (the core DAG-scheduling algorithm) ──────────────────────────

    /// Return an iterator over IDs of nodes that are ready to execute.
    ///
    /// A node is **ready** when:
    /// 1. Its state is [`NodeState::Pending`].
    /// 2. Every parent (incoming edge) is in [`NodeState::Completed`].
    ///
    /// The scheduler calls this method, collects the results, and promotes
    /// each node to [`NodeState::Ready`] via [`Self::transition`].
    ///
    /// # Why this lives on the graph
    ///
    /// Readiness is a graph-level property — it requires inspecting both
    /// node state **and** edge topology.  Placing it here means the
    /// scheduler does not need to know about edges or indices.
    ///
    /// # Performance
    ///
    /// This method iterates over all nodes and inspects each node's incoming
    /// edges.  For the expected graph size (< 1,000 nodes) this is
    /// negligible.  A future version may maintain a ready-set incrementally
    /// for O(1) readiness checks.
    pub fn ready_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.graph.node_indices().filter_map(move |idx| {
            let node = &self.graph[idx];
            if !node.is_pending() {
                return None;
            }
            let all_parents_completed = self
                .graph
                .edges_directed(idx, Direction::Incoming)
                .all(|e| self.graph[e.source()].is_completed());
            if all_parents_completed {
                Some(node.id())
            } else {
                None
            }
        })
    }

    // ── State transitions ──────────────────────────────────────────────────────

    /// Transition the node identified by `id` to `new_state`.
    ///
    /// No-op if the ID is not found (logs a warning).  Delegates to
    /// [`WorkflowNode::transition`], which handles timers, attempt counters,
    /// and error capture.
    ///
    /// # Future
    /// This method is the natural insertion point for:
    /// - Validating legal state transitions.
    /// - Emitting [`EngineEvent`]s via an optional event bus.
    /// - Updating execution metrics and histograms.
    /// - Recording checkpoint data for persistence and replay.
    pub fn transition(&mut self, id: &str, new_state: NodeState) {
        if let Some(&idx) = self.index_map.get(id) {
            self.graph[idx].transition(new_state);
        } else {
            tracing::warn!(node_id = id, "transition called on unknown node");
        }
    }

    /// Convenience: transition a node to [`NodeState::Running`].
    #[inline]
    pub fn mark_running(&mut self, id: &str) {
        self.transition(id, NodeState::Running);
    }

    /// Convenience: transition a node to [`NodeState::Completed`].
    #[inline]
    pub fn mark_completed(&mut self, id: &str) {
        self.transition(id, NodeState::Completed);
    }

    /// Convenience: transition a node to [`NodeState::Failed`] with a reason.
    #[inline]
    pub fn mark_failed(&mut self, id: &str, reason: impl Into<String>) {
        self.transition(
            id,
            NodeState::Failed {
                reason: reason.into(),
            },
        );
    }

    /// Convenience: transition a node to [`NodeState::Skipped`].
    #[inline]
    pub fn mark_skipped(&mut self, id: &str) {
        self.transition(id, NodeState::Skipped);
    }

    // ── Queries ────────────────────────────────────────────────────────────────

    /// Returns `true` when every node has reached a terminal state
    /// (any of `Completed`, `Failed`, `Skipped`, `Cancelled`).
    ///
    /// For stricter checks use [`Self::all_completed`].
    pub fn is_complete(&self) -> bool {
        self.graph
            .node_indices()
            .all(|idx| self.graph[idx].state().is_terminal())
    }

    /// Returns `true` when every node completed **successfully**
    /// (state is [`NodeState::Completed`]).
    ///
    /// Unlike [`Self::is_complete`], this returns `false` if any node
    /// failed, was skipped, or was cancelled — even if all nodes are
    /// terminal.
    pub fn all_completed(&self) -> bool {
        self.graph
            .node_indices()
            .all(|idx| self.graph[idx].is_completed())
    }

    /// Returns `true` when at least one node has failed.
    pub fn has_failures(&self) -> bool {
        self.graph
            .node_indices()
            .any(|idx| self.graph[idx].is_failed())
    }

    /// Number of nodes currently in [`NodeState::Running`].
    pub fn running_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].is_running())
            .count()
    }

    /// Number of nodes currently in [`NodeState::Pending`].
    pub fn pending_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph[idx].is_pending())
            .count()
    }

    /// Number of nodes not yet in a terminal state.
    ///
    /// Useful for deadlock detection: if `non_terminal_count > 0` but no
    /// nodes are ready or running, the workflow is stuck.
    pub fn non_terminal_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|&idx| !self.graph[idx].state().is_terminal())
            .count()
    }

    /// Returns the total number of nodes in the graph.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of dependency edges in the graph.
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Count nodes in each terminal state for summary logging.
    ///
    /// Returns `(completed, failed, skipped)`.
    pub fn completion_summary(&self) -> (usize, usize, usize) {
        let (mut done, mut failed, mut skipped) = (0, 0, 0);
        for idx in self.graph.node_indices() {
            match self.graph[idx].state() {
                NodeState::Completed => done += 1,
                NodeState::Failed { .. } => failed += 1,
                NodeState::Skipped => skipped += 1,
                _ => {}
            }
        }
        (done, failed, skipped)
    }

    // ── Cycle detection ────────────────────────────────────────────────────────

    /// Detect if the graph contains a cycle.
    ///
    /// Returns the [`NodeId`] of a node involved in a cycle, if one exists.
    /// The returned ID is guaranteed to be part of at least one cycle, but
    /// not necessarily the "root cause" (cycle detection is best-effort for
    /// error reporting).
    ///
    /// Uses `petgraph::algo::toposort` internally.
    pub fn detect_cycle(&self) -> Option<NodeId> {
        match petgraph::algo::toposort(&self.graph, None) {
            Ok(_) => None,
            Err(cycle) => {
                let idx = cycle.node_id();
                Some(self.graph[idx].id())
            }
        }
    }

    // ── Iteration ──────────────────────────────────────────────────────────────

    /// Iterate over all nodes in the graph (order is stable but
    /// implementation-defined).
    pub fn iter(&self) -> impl Iterator<Item = &WorkflowNode<I>> + '_ {
        self.graph.node_indices().map(move |idx| &self.graph[idx])
    }

    /// Iterate over all node IDs in the graph.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.graph
            .node_indices()
            .map(move |idx| self.graph[idx].id())
    }
}

// ── Debug ─────────────────────────────────────────────────────────────────────

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

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        context::WorkflowContext,
        skill::{Skill, SkillDefinition},
    };
    use async_trait::async_trait;
    use std::sync::Arc;

    /// A minimal skill implementation for testing.
    struct TestSkill {
        def: &'static SkillDefinition,
    }

    #[async_trait]
    impl Skill<()> for TestSkill {
        fn definition(&self) -> &'static SkillDefinition {
            self.def
        }

        async fn execute(&self, _ctx: &mut WorkflowContext<()>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    const SKILL_A: SkillDefinition = SkillDefinition {
        id: "a",
        name: "A",
        description: "",
        version: "1.0.0",
        dependencies: &[],
    };

    const SKILL_B: SkillDefinition = SkillDefinition {
        id: "b",
        name: "B",
        description: "",
        version: "1.0.0",
        dependencies: &["a"],
    };

    const SKILL_C: SkillDefinition = SkillDefinition {
        id: "c",
        name: "C",
        description: "",
        version: "1.0.0",
        dependencies: &["a"],
    };

    const SKILL_D: SkillDefinition = SkillDefinition {
        id: "d",
        name: "D",
        description: "",
        version: "1.0.0",
        dependencies: &["b", "c"],
    };

    const SKILL_E: SkillDefinition = SkillDefinition {
        id: "e",
        name: "E",
        description: "",
        version: "1.0.0",
        dependencies: &[],
    };

    fn make_skill(def: &'static SkillDefinition) -> SkillRef<()> {
        Arc::new(TestSkill { def })
    }

    /// Transition a node to `Completed` for testing convenience.
    fn complete_node(graph: &mut WorkflowGraph<()>, id: &str) {
        let node = graph.get_mut(id).expect("node exists");
        node.transition(NodeState::Completed);
    }

    // ── Single node ────────────────────────────────────────────────────────────

    #[test]
    fn single_node_graph() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert!(graph.contains("a"));
        assert!(!graph.contains("b"));
    }

    #[test]
    fn single_node_ready() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));

        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(
            ready,
            vec!["a"],
            "single pending node with no deps is ready"
        );
    }

    #[test]
    fn single_node_lifecycle() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));

        graph.transition("a", NodeState::Ready);
        assert!(graph.get("a").unwrap().is_ready());

        graph.mark_running("a");
        assert!(graph.get("a").unwrap().is_running());

        graph.mark_completed("a");
        assert!(graph.get("a").unwrap().is_completed());
        assert!(graph.all_completed());
        assert!(graph.is_complete());
    }

    #[test]
    fn single_node_failure() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));

        graph.transition("a", NodeState::Ready);
        graph.mark_running("a");
        graph.mark_failed("a", "something broke");

        let node = graph.get("a").unwrap();
        assert!(node.is_failed());
        assert!(!node.is_completed());
        assert!(graph.has_failures());
        assert!(graph.is_complete());
        assert!(!graph.all_completed());
    }

    // ── Linear graph ───────────────────────────────────────────────────────────

    #[test]
    fn linear_graph_ready_nodes() {
        // a → b
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        // Initially only a is ready.
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(ready, vec!["a"], "only root node is ready");

        // Complete a → b becomes ready.
        complete_node(&mut graph, "a");
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(ready, vec!["b"], "b is ready after a completes");
    }

    #[test]
    fn linear_graph_roots_and_leaves() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        assert_eq!(graph.roots(), vec!["a"]);
        assert_eq!(graph.leaves(), vec!["b"]);
    }

    #[test]
    fn linear_graph_parents_children() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        assert_eq!(graph.parents("b"), vec!["a"]);
        assert!(graph.parents("a").is_empty());
        assert_eq!(graph.children("a"), vec!["b"]);
        assert!(graph.children("b").is_empty());
    }

    // ── Diamond graph ──────────────────────────────────────────────────────────

    #[test]
    fn diamond_graph_ready_nodes() {
        //   a
        //  / \
        // b   c
        //  \ /
        //   d
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_node(make_skill(&SKILL_C));
        graph.add_node(make_skill(&SKILL_D));
        graph.add_dependency("a", "b");
        graph.add_dependency("a", "c");
        graph.add_dependency("b", "d");
        graph.add_dependency("c", "d");

        // Only a is ready initially.
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(ready, vec!["a"]);

        // Complete a → both b and c are ready.
        complete_node(&mut graph, "a");
        let mut ready: Vec<&str> = graph.ready_nodes().collect();
        ready.sort_unstable();
        assert_eq!(ready, vec!["b", "c"], "both b and c ready after a");

        // Complete b and c → d is ready.
        complete_node(&mut graph, "b");
        complete_node(&mut graph, "c");
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(ready, vec!["d"], "d ready after b and c");
    }

    #[test]
    fn diamond_graph_d_not_ready_until_both_parents_done() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_node(make_skill(&SKILL_C));
        graph.add_node(make_skill(&SKILL_D));
        graph.add_dependency("a", "b");
        graph.add_dependency("a", "c");
        graph.add_dependency("b", "d");
        graph.add_dependency("c", "d");

        complete_node(&mut graph, "a");
        complete_node(&mut graph, "b");

        // Only c should be ready; d still has c as incomplete parent.
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert_eq!(ready, vec!["c"], "only c ready; d still waiting on c");
    }

    // ── Multiple roots ─────────────────────────────────────────────────────────

    #[test]
    fn multiple_roots() {
        // a (root 1), e (root 2) — no deps
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_E));

        let mut roots = graph.roots();
        roots.sort_unstable();
        assert_eq!(roots, vec!["a", "e"]);

        let mut ready: Vec<&str> = graph.ready_nodes().collect();
        ready.sort_unstable();
        assert_eq!(ready, vec!["a", "e"], "both roots are ready");
    }

    // ── Multiple leaves ────────────────────────────────────────────────────────

    const SKILL_C_WITH_DEP: SkillDefinition = SkillDefinition {
        id: "c",
        name: "C",
        description: "",
        version: "1.0.0",
        dependencies: &["a"],
    };

    #[test]
    fn multiple_leaves() {
        // a → b, a → c (b and c are both leaves)
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_node(make_skill(&SKILL_C_WITH_DEP));
        graph.add_dependency("a", "b");
        graph.add_dependency("a", "c");

        let mut leaves = graph.leaves();
        leaves.sort_unstable();
        assert_eq!(leaves, vec!["b", "c"]);
    }

    // ── Not-ready when parent is not Completed ─────────────────────────────────

    #[test]
    fn not_ready_when_parent_failed() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        graph.transition("a", NodeState::Ready);
        graph.mark_running("a");
        graph.mark_failed("a", "error");

        // b should NOT be ready because a failed (not completed).
        let ready: Vec<&str> = graph.ready_nodes().collect();
        assert!(ready.is_empty(), "b should not be ready when a failed");
    }

    // ── Lookup edge cases ──────────────────────────────────────────────────────

    #[test]
    fn handles_missing_ids_gracefully() {
        let graph: WorkflowGraph<()> = WorkflowGraph::new();
        assert!(!graph.contains("missing"));
        assert!(graph.get("missing").is_none());
        assert!(graph.skill("missing").is_none());
        assert!(graph.parents("missing").is_empty());
        assert!(graph.children("missing").is_empty());
    }

    #[test]
    fn is_complete_on_empty_graph() {
        let graph: WorkflowGraph<()> = WorkflowGraph::new();
        assert!(graph.is_complete(), "empty graph is vacuously complete");
    }

    // ── Completion queries ─────────────────────────────────────────────────────

    #[test]
    fn completion_summary_counts() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        // Nothing completed yet.
        assert_eq!(graph.completion_summary(), (0, 0, 0));

        // Complete a, fail b.
        complete_node(&mut graph, "a");
        graph.transition("b", NodeState::Ready);
        graph.mark_running("b");
        graph.mark_failed("b", "oops");

        assert_eq!(graph.completion_summary(), (1, 1, 0));
        assert!(graph.is_complete());
        assert!(!graph.all_completed());
        assert!(graph.has_failures());
    }

    // ── Skill accessor ─────────────────────────────────────────────────────────

    #[test]
    fn skill_accessor_returns_skill() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));

        let skill_ref = graph.skill("a").expect("skill should exist");
        assert_eq!(skill_ref.definition().id, "a");
    }

    // ── Cycle detection ────────────────────────────────────────────────────────

    #[test]
    fn detect_cycle_returns_none_for_acyclic() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        assert!(graph.detect_cycle().is_none());
    }

    const SKILL_B_CYCLE: SkillDefinition = SkillDefinition {
        id: "b",
        name: "B",
        description: "",
        version: "1.0.0",
        dependencies: &["c"],
    };
    const SKILL_C_CYCLE: SkillDefinition = SkillDefinition {
        id: "c",
        name: "C",
        description: "",
        version: "1.0.0",
        dependencies: &["b"],
    };

    #[test]
    fn detect_cycle_finds_cycle() {
        // b → c → b (two-node cycle)
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_B_CYCLE));
        graph.add_node(make_skill(&SKILL_C_CYCLE));
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "b");

        assert!(graph.detect_cycle().is_some(), "should detect b↔c cycle");
    }

    // ── Iterator ───────────────────────────────────────────────────────────────

    #[test]
    fn iter_yields_all_nodes() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));
        graph.add_dependency("a", "b");

        let ids: Vec<&str> = graph.iter().map(|n| n.id()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[test]
    fn node_ids_yields_all_ids() {
        let mut graph = WorkflowGraph::new();
        graph.add_node(make_skill(&SKILL_A));
        graph.add_node(make_skill(&SKILL_B));

        let mut ids: Vec<&str> = graph.node_ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
