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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStartedEvent {
    pub execution_id: ExecutionId,
    pub workflow_id: WorkflowId,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanBuiltEvent {
    pub execution_id: ExecutionId,
    pub ordered_skills: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCompletedEvent {
    pub execution_id: ExecutionId,
    pub elapsed_ms: u64,
    pub completed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCancelledEvent {
    pub execution_id: ExecutionId,
    pub elapsed_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFailedEvent {
    pub execution_id: ExecutionId,
    pub elapsed_ms: u64,
    pub error: String,
    pub failing_skill_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStartedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCompletedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFailedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub attempts: u32,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactCreatedEvent {
    pub execution_id: ExecutionId,
    pub artifact_id: ArtifactId,
    pub skill_id: String,
    pub kind_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequestStartedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub prompt_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequestCompletedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub elapsed_ms: u64,
    pub response_len: usize,
    pub parse_success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvokedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeQueuedEvent {
    pub execution_id: ExecutionId,
    /// The skill/node that was promoted from Pending to Ready.
    pub skill_id: String,
    /// Number of currently ready nodes after this promotion.
    pub ready_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCompletedEvent {
    pub execution_id: ExecutionId,
    pub skill_id: String,
    pub tool_name: String,
    pub elapsed_ms: u64,
    pub success: bool,
}

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
    NodeQueued(NodeQueuedEvent),
}
