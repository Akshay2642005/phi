//! Execution result and metadata models.
//!
//! These types capture the outcome of a completed workflow run in a
//! structured, domain-agnostic way.  They replace the ad-hoc `Vec<String>`
//! fields on the old `WorkflowResult`.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::core::{
    ids::{ExecutionId, SkillId, WorkflowId},
    workflow::state::{NodeState, WorkflowState},
};

// ── Lifecycle timestamps ──────────────────────────────────────────────────────

/// Wall-clock timestamps for the key lifecycle transitions of a workflow run.
///
/// Uses [`SystemTime`] (wall clock) rather than [`std::time::Instant`]
/// (monotonic) so that timestamps can be serialised, persisted, and displayed
/// in human-readable form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleTimestamps {
    /// When the run was created / submitted.
    #[serde(with = "system_time_serde")]
    pub created_at: SystemTime,

    /// When the first skill started executing (`None` if never started).
    #[serde(with = "option_system_time_serde")]
    pub started_at: Option<SystemTime>,

    /// When the workflow reached a terminal state (`None` if not yet done).
    #[serde(with = "option_system_time_serde")]
    pub finished_at: Option<SystemTime>,
}

impl LifecycleTimestamps {
    /// Create timestamps with only `created_at` set.
    pub fn new() -> Self {
        Self {
            created_at: SystemTime::now(),
            started_at: None,
            finished_at: None,
        }
    }

    /// Mark the run as started (now).
    pub fn mark_started(&mut self) {
        self.started_at = Some(SystemTime::now());
    }

    /// Mark the run as finished (now).
    pub fn mark_finished(&mut self) {
        self.finished_at = Some(SystemTime::now());
    }

    /// Total elapsed duration from creation to finish, or from creation to
    /// now if not yet finished.
    pub fn elapsed(&self) -> Duration {
        let end = self.finished_at.unwrap_or_else(SystemTime::now);
        end.duration_since(self.created_at)
            .unwrap_or(Duration::ZERO)
    }
}

impl Default for LifecycleTimestamps {
    fn default() -> Self {
        Self::new()
    }
}

// ── Execution metadata ────────────────────────────────────────────────────────

/// Identity and timing metadata for a single workflow execution.
///
/// Attached to every [`ExecutionSummary`] so that post-run analysis can
/// correlate results with the triggering context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// Unique execution instance identifier.
    pub execution_id: ExecutionId,

    /// The workflow that was executed.
    pub workflow_id: WorkflowId,

    /// Lifecycle timestamps.
    pub timestamps: LifecycleTimestamps,

    /// Total wall-clock execution time in milliseconds.
    ///
    /// Derived from [`LifecycleTimestamps`]; stored explicitly for cheap
    /// access without recomputing.
    pub elapsed_ms: u64,
}

impl ExecutionMetadata {
    /// Create metadata for a new run.
    pub fn new(execution_id: ExecutionId, workflow_id: WorkflowId) -> Self {
        Self {
            execution_id,
            workflow_id,
            timestamps: LifecycleTimestamps::new(),
            elapsed_ms: 0,
        }
    }

    /// Finalise the metadata after the run completes.
    pub fn finish(&mut self) {
        self.timestamps.mark_finished();
        self.elapsed_ms = self.timestamps.elapsed().as_millis() as u64;
    }
}

// ── Per-skill outcome ─────────────────────────────────────────────────────────

/// The result of executing a single skill node.
///
/// Collected by the engine and aggregated into [`ExecutionSummary`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutcome {
    /// The skill that was executed.
    ///
    /// Using a typed `SkillId` here; in the current engine implementation the
    /// skill `id` is a `&'static str` — use `SkillId::new()` at registration
    /// time and carry it forward.
    ///
    /// For now we store the string form to remain compatible with the
    /// `&'static str`-keyed skill definitions.
    pub skill_id: String,

    /// Terminal node state after execution.
    pub state: NodeState,

    /// Execution duration in milliseconds.
    pub elapsed_ms: u64,

    /// Human-readable error message if the skill failed.
    pub error: Option<String>,
}

impl SkillOutcome {
    /// Construct a successful outcome.
    pub fn success(skill_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Completed,
            elapsed_ms,
            error: None,
        }
    }

    /// Construct a failed outcome.
    pub fn failure(skill_id: impl Into<String>, elapsed_ms: u64, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Failed { reason: reason.clone() },
            elapsed_ms,
            error: Some(reason),
        }
    }

    /// Construct a skipped outcome.
    pub fn skipped(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Skipped,
            elapsed_ms: 0,
            error: None,
        }
    }

    /// `true` if the skill completed successfully.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.state.is_success()
    }
}

// ── Execution summary ─────────────────────────────────────────────────────────

/// The complete outcome of a workflow run.
///
/// Replaces the ad-hoc `Vec<String>` fields in the old `WorkflowResult` with
/// typed, structured data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Identity and timing for this run.
    pub metadata: ExecutionMetadata,

    /// Terminal state of the workflow.
    pub state: WorkflowState,

    /// Per-skill outcomes in execution order.
    pub skill_outcomes: Vec<SkillOutcome>,

    /// Top-level error messages if the workflow failed.
    pub errors: Vec<String>,
}

impl ExecutionSummary {
    /// Create a summary for a new run.
    pub fn new(execution_id: ExecutionId, workflow_id: WorkflowId) -> Self {
        Self {
            metadata: ExecutionMetadata::new(execution_id, workflow_id),
            state: WorkflowState::Pending,
            skill_outcomes: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// `true` when the workflow completed without errors.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.state.is_success()
    }

    /// IDs of all skills that completed successfully.
    pub fn completed_skills(&self) -> impl Iterator<Item = &str> {
        self.skill_outcomes
            .iter()
            .filter(|o| o.is_success())
            .map(|o| o.skill_id.as_str())
    }

    /// IDs of all skills that failed.
    pub fn failed_skills(&self) -> impl Iterator<Item = &str> {
        self.skill_outcomes
            .iter()
            .filter(|o| matches!(o.state, NodeState::Failed { .. }))
            .map(|o| o.skill_id.as_str())
    }
}

// ── serde helpers for SystemTime ──────────────────────────────────────────────

mod system_time_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let ms = t
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        ms.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_millis(ms))
    }
}

mod option_system_time_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(t: &Option<SystemTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => {
                let ms = t
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64;
                Some(ms).serialize(s)
            }
            None => None::<u64>.serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SystemTime>, D::Error> {
        let opt = Option::<u64>::deserialize(d)?;
        Ok(opt.map(|ms| UNIX_EPOCH + Duration::from_millis(ms)))
    }
}
