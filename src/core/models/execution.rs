//! Execution result and metadata models.
//!
//! These types capture the outcome of a completed workflow run in a
//! structured, domain-agnostic way.  They replace the ad-hoc `Vec<String>`
//! fields on the old `WorkflowResult`.

use std::time::{Duration, SystemTime};

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::{
    ids::{ExecutionId, SkillId, WorkflowId},
    models::diagnostics::NodeDiagnostics,
    workflow::state::{NodeState, WorkflowState},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleTimestamps {
    #[serde(with = "system_time_serde")]
    pub created_at: SystemTime,
    #[serde(with = "option_system_time_serde")]
    pub started_at: Option<SystemTime>,
    #[serde(with = "option_system_time_serde")]
    pub finished_at: Option<SystemTime>,
}

impl LifecycleTimestamps {
    pub fn new() -> Self {
        Self {
            created_at: SystemTime::now(),
            started_at: None,
            finished_at: None,
        }
    }

    pub fn mark_started(&mut self) {
        self.started_at = Some(SystemTime::now());
    }

    pub fn mark_finished(&mut self) {
        self.finished_at = Some(SystemTime::now());
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    pub execution_id: ExecutionId,
    pub workflow_id: WorkflowId,
    pub timestamps: LifecycleTimestamps,
    pub elapsed_ms: u64,
}

impl ExecutionMetadata {
    pub fn new(execution_id: ExecutionId, workflow_id: WorkflowId) -> Self {
        Self {
            execution_id,
            workflow_id,
            timestamps: LifecycleTimestamps::new(),
            elapsed_ms: 0,
        }
    }

    pub fn finish(&mut self) {
        self.timestamps.mark_finished();
        self.elapsed_ms = self.timestamps.elapsed().as_millis() as u64;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutcome {
    pub skill_id: String,
    pub state: NodeState,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

impl SkillOutcome {
    pub fn success(skill_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Completed,
            elapsed_ms,
            error: None,
        }
    }

    pub fn failure(
        skill_id: impl Into<String>,
        elapsed_ms: u64,
        reason: impl Into<String>,
    ) -> Self {
        let reason = reason.into();
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Failed {
                reason: reason.clone(),
            },
            elapsed_ms,
            error: Some(reason),
        }
    }

    pub fn skipped(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            state: NodeState::Skipped,
            elapsed_ms: 0,
            error: None,
        }
    }

    #[inline]
    pub fn is_success(&self) -> bool {
        self.state.is_success()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    pub metadata: ExecutionMetadata,
    pub state: WorkflowState,
    pub skill_outcomes: Vec<SkillOutcome>,
    pub errors: Vec<String>,
    /// Per-node diagnostics captured during execution.
    pub node_diagnostics: Vec<NodeDiagnostics>,
}

impl ExecutionSummary {
    pub fn new(execution_id: ExecutionId, workflow_id: WorkflowId) -> Self {
        Self {
            metadata: ExecutionMetadata::new(execution_id, workflow_id),
            state: WorkflowState::Pending,
            skill_outcomes: Vec::new(),
            errors: Vec::new(),
            node_diagnostics: Vec::new(),
        }
    }

    #[inline]
    pub fn is_success(&self) -> bool {
        self.state.is_success()
    }

    pub fn completed_skills(&self) -> impl Iterator<Item = &str> {
        self.skill_outcomes
            .iter()
            .filter(|o| o.is_success())
            .map(|o| o.skill_id.as_str())
    }

    pub fn failed_skills(&self) -> impl Iterator<Item = &str> {
        self.skill_outcomes
            .iter()
            .filter(|o| matches!(o.state, NodeState::Failed { .. }))
            .map(|o| o.skill_id.as_str())
    }
}

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
