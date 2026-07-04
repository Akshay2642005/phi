//! Strongly typed engine event structs.
//!
//! Every event is a plain Rust struct with named fields.  The [`EngineEvent`]
//! enum is a wrapper that carries one of these structs, so subscribers can
//! pattern-match on the discriminant without needing to deserialise a generic
//! payload.
//!
//! # Design principles
//! - One struct per event — no stringly-typed payloads.
//! - All fields are concrete Rust types — no `serde_json::Value`.
//! - Every event carries the `execution_id` so subscribers can correlate
//!   events across concurrent runs.
//! - Events are `Clone` so they can be broadcast to multiple listeners.

use serde::{Deserialize, Serialize};

use crate::core::ids::{ArtifactId, ExecutionId, WorkflowId};

// ── Workflow-level events ─────────────────────────────────────────────────────

/// Emitted when a workflow run begins executing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStartedEvent {
    /// Unique identifier for this execution instance.
    pub execution_id: ExecutionId,
    /// The workflow being executed.
    pub workflow_id: WorkflowId,
    /// Total number of skills in the execution plan.
    pub skill_count: usize,
}

/// Emitted when the planner finishes producing an execution plan (DAG).
///
/// Useful for observability — subscribers can inspect the planned node
/// topology before execution begins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanBuiltEvent {
    /// Execution that will run this plan.
    pub execution_id: ExecutionId,
    /// Ordered skill IDs in execution order.
    pub ordered_skills: Vec<String>,
    /// Total number of nodes in the plan.
    pub node_count: usize,
    /// Total number of dependency edges.
    pub edge_count: usize,
}

/// Emitted when all skills in a workflow have reached a terminal state
/// successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCompletedEvent {
    /// Execution that finished.
    pub execution_id: ExecutionId,
    /// Total wall-clock time in milliseconds.
    pub elapsed_ms: u64,
    /// Number of skills that completed successfully.
    pub completed_count: usize,
}

/// Emitted when the workflow was cancelled before completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCancelledEvent {
    /// Execution that was cancelled.
    pub execution_id: ExecutionId,
    /// How far into the run cancellation occurred (ms).
    pub elapsed_ms: u64,
    /// Optional reason for cancellation.
    pub reason: Option<String>,
}

/// Emitted when the workflow terminates due to a fatal error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFailedEvent {
    /// Execution that failed.
    pub execution_id: ExecutionId,
    /// Wall-clock time at failure (ms).
    pub elapsed_ms: u64,
    /// Human-readable error description.
    pub error: String,
    /// ID of the skill that caused the failure, if applicable.
    pub failing_skill_id: Option<String>,
}

// ── Skill-level events ────────────────────────────────────────────────────────

/// Emitted immediately before a skill begins executing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStartedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill being executed (static ID string).
    pub skill_id: String,
    /// Attempt number (1-based; >1 on retries).
    pub attempt: u32,
}

/// Emitted immediately after a skill completes successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCompletedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill that completed.
    pub skill_id: String,
    /// Time spent executing this skill (ms).
    pub elapsed_ms: u64,
}

/// Emitted when a skill fails (after all retry attempts are exhausted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFailedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill that failed.
    pub skill_id: String,
    /// Total attempts made.
    pub attempts: u32,
    /// Human-readable error description.
    pub error: String,
}

// ── Artifact events ───────────────────────────────────────────────────────────

/// Emitted when a skill writes an artifact to the context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCreatedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// Unique identifier for the new artifact.
    pub artifact_id: ArtifactId,
    /// The skill that produced the artifact.
    pub skill_id: String,
    /// Domain-agnostic kind tag (e.g., `"word_count"`, `"sentiment"`).
    pub kind_tag: String,
}

// ── LLM events ────────────────────────────────────────────────────────────────

/// Emitted just before an LLM request is dispatched.
///
/// Useful for latency measurement, quota tracking, and audit logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequestStartedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill that initiated the request.
    pub skill_id: String,
    /// Approximate prompt length in characters (for cost estimation).
    pub prompt_len: usize,
}

/// Emitted when an LLM response is received.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequestCompletedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill that received the response.
    pub skill_id: String,
    /// Round-trip time in milliseconds.
    pub elapsed_ms: u64,
    /// Approximate response length in characters.
    pub response_len: usize,
    /// Whether the response was successfully parsed.
    pub parse_success: bool,
}

// ── Tool events ───────────────────────────────────────────────────────────────

/// Emitted when a skill invokes an external tool (web search, code runner, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvokedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill initiating the tool call.
    pub skill_id: String,
    /// Logical tool name (e.g., `"web_search"`, `"code_runner"`).
    pub tool_name: String,
}

/// Emitted when an external tool call returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCompletedEvent {
    /// Parent execution.
    pub execution_id: ExecutionId,
    /// The skill that made the tool call.
    pub skill_id: String,
    /// Logical tool name.
    pub tool_name: String,
    /// Round-trip time in milliseconds.
    pub elapsed_ms: u64,
    /// Whether the tool call succeeded.
    pub success: bool,
}

// ── Wrapper enum ──────────────────────────────────────────────────────────────

/// Top-level event wrapper.
///
/// Subscribers receive `EngineEvent` values and pattern-match on the variant
/// to handle only the events they care about.
///
/// # Example
/// ```ignore
/// match event {
///     EngineEvent::WorkflowCompleted(e) => metrics.record_run(e.elapsed_ms),
///     EngineEvent::SkillFailed(e) => alerts.send(&e.error),
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EngineEvent {
    WorkflowStarted(WorkflowStartedEvent),
    WorkflowCompleted(WorkflowCompletedEvent),
    WorkflowCancelled(WorkflowCancelledEvent),
    WorkflowFailed(WorkflowFailedEvent),
    ExecutionPlanBuilt(ExecutionPlanBuiltEvent),
    SkillStarted(SkillStartedEvent),
    SkillCompleted(SkillCompletedEvent),
    SkillFailed(SkillFailedEvent),
    ArtifactCreated(ArtifactCreatedEvent),
    LlmRequestStarted(LlmRequestStartedEvent),
    LlmRequestCompleted(LlmRequestCompletedEvent),
    ToolInvoked(ToolInvokedEvent),
    ToolCompleted(ToolCompletedEvent),
}
