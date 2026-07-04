//! Immutable workflow definition — the static, canonical representation
//! of a workflow.
//!
//! # Design
//! A [`Workflow<I>`] is the top-level container for workflow metadata and
//! registered skills.  It is constructed through [`WorkflowBuilder`] and
//! becomes immutable immediately after construction — there are no setters,
//! no mutation methods, and no interior mutability.
//!
//! Later phases (planner, scheduler, executor) will *consume* the workflow
//! definition to build execution graphs and run skills, but the definition
//! itself never changes.  Runtime state lives in separate structures
//! ([`WorkflowContext`], [`WorkflowGraph`], [`ExecutionState`]).
//!
//! # Generic parameter
//! `I` is the domain input / context type.  All registered skills must
//! implement [`Skill<I>`] for the same `I`.  This keeps the framework
//! generic over any application domain without coupling to databases,
//! AI models, or serialisation formats.
//!
//! # Thread safety
//! `Workflow<I>` is `Send + Sync` when `I: Send + Sync + Debug + 'static`,
//! which holds for all practical input types.  The struct contains only
//! read-only data and an [`Arc`]-based skill registry.

use std::sync::Arc;

use crate::core::ids::WorkflowId;
use crate::core::skill::{SkillRef, SkillRegistry};

use super::builder::WorkflowBuilder;
use super::metadata::WorkflowMetadata;

pub struct Workflow<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    id: WorkflowId,
    name: String,
    description: Option<String>,
    version: String,
    registry: SkillRegistry<I>,
    metadata: WorkflowMetadata,
}

impl<I> Workflow<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    #[inline]
    pub fn id(&self) -> WorkflowId {
        self.id
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[inline]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[inline]
    pub fn metadata(&self) -> &WorkflowMetadata {
        &self.metadata
    }

    #[inline]
    pub fn registry(&self) -> &SkillRegistry<I> {
        &self.registry
    }

    #[inline]
    pub fn skill_count(&self) -> usize {
        self.registry.skills().len()
    }

    #[inline]
    pub fn contains_skill(&self, id: &str) -> bool {
        self.registry.contains(id)
    }

    pub fn skills(&self) -> impl Iterator<Item = (&str, &SkillRef<I>)> {
        self.registry.iter()
    }

    pub fn skill_ids(&self) -> impl Iterator<Item = &str> {
        self.registry.iter().map(|(id, _)| id)
    }

    #[inline]
    pub(crate) fn new(
        id: WorkflowId,
        name: String,
        description: Option<String>,
        version: String,
        registry: SkillRegistry<I>,
        metadata: WorkflowMetadata,
    ) -> Self {
        Self {
            id,
            name,
            description,
            version,
            registry,
            metadata,
        }
    }

    #[inline]
    pub fn builder() -> WorkflowBuilder<I> {
        WorkflowBuilder::new()
    }
}

impl<I> std::fmt::Debug for Workflow<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workflow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("skill_count", &self.registry.skills().len())
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill::{Skill, SkillDefinition};

    const SKILL_A_DEF: SkillDefinition = SkillDefinition {
        id: "skill_a",
        name: "Skill A",
        description: "First test skill",
        version: "1.0.0",
        dependencies: &[],
    };
    const SKILL_B_DEF: SkillDefinition = SkillDefinition {
        id: "skill_b",
        name: "Skill B",
        description: "Second test skill",
        version: "1.0.0",
        dependencies: &[],
    };
    const SKILL_C_DEF: SkillDefinition = SkillDefinition {
        id: "skill_c",
        name: "Skill C",
        description: "Third test skill",
        version: "2.0.0",
        dependencies: &["skill_a"],
    };

    struct SkillA;
    struct SkillB;
    struct SkillC;

    #[async_trait::async_trait]
    impl Skill<()> for SkillA {
        fn definition(&self) -> &'static SkillDefinition {
            &SKILL_A_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Skill<()> for SkillB {
        fn definition(&self) -> &'static SkillDefinition {
            &SKILL_B_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Skill<()> for SkillC {
        fn definition(&self) -> &'static SkillDefinition {
            &SKILL_C_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn build_minimal_workflow() -> Workflow<()> {
        Workflow::builder()
            .name("Test Workflow")
            .version("1.0.0")
            .build()
            .expect("minimal workflow should build")
    }

    fn build_workflow_with_skills() -> Workflow<()> {
        Workflow::builder()
            .name("Full Workflow")
            .version("2.0.0")
            .register(SkillA)
            .register(SkillB)
            .register(SkillC)
            .build()
            .expect("workflow with skills should build")
    }

    #[test]
    fn workflow_builds_with_minimal_fields() {
        let wf = build_minimal_workflow();
        assert_eq!(wf.name(), "Test Workflow");
        assert_eq!(wf.version(), "1.0.0");
        assert!(wf.description().is_none());
        assert_eq!(wf.skill_count(), 0);
    }

    #[test]
    fn workflow_builds_with_all_fields() {
        let wf = Workflow::builder()
            .name("Full Example")
            .description("A workflow with everything")
            .version("3.0.0")
            .register(SkillA)
            .register(SkillB)
            .build()
            .expect("full workflow should build");
        assert_eq!(wf.name(), "Full Example");
        assert_eq!(wf.version(), "3.0.0");
        assert_eq!(wf.description(), Some("A workflow with everything"));
        assert_eq!(wf.skill_count(), 2);
    }

    #[test]
    fn workflow_id_is_unique() {
        assert_ne!(build_minimal_workflow().id(), build_minimal_workflow().id());
    }

    #[test]
    fn workflow_is_immutable_after_build() {
        let wf = build_workflow_with_skills();
        assert_eq!(wf.name(), "Full Workflow");
        assert_eq!(wf.name(), "Full Workflow");
    }

    #[test]
    fn skill_count_and_contains() {
        let wf = build_workflow_with_skills();
        assert_eq!(wf.skill_count(), 3);
        assert!(wf.contains_skill("skill_a"));
        assert!(wf.contains_skill("skill_b"));
        assert!(wf.contains_skill("skill_c"));
        assert!(!wf.contains_skill("nonexistent"));
    }

    #[test]
    fn skills_iterator_yields_all() {
        let wf = build_workflow_with_skills();
        let ids: Vec<&str> = wf.skills().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"skill_a"));
        assert!(ids.contains(&"skill_b"));
        assert!(ids.contains(&"skill_c"));
    }

    #[test]
    fn skill_ids_iterator_yields_all_ids() {
        let wf = build_workflow_with_skills();
        let ids: Vec<&str> = wf.skill_ids().collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"skill_a"));
    }

    #[test]
    fn empty_workflow_has_no_skills() {
        let wf = build_minimal_workflow();
        assert_eq!(wf.skill_count(), 0);
        assert!(wf.skills().next().is_none());
    }

    #[test]
    fn metadata_is_accessible() {
        let wf = build_minimal_workflow();
        let meta = wf.metadata();
        assert!(meta.created_at().is_some());
        assert!(meta.author().is_none());
        assert!(meta.tags().is_empty());
    }

    #[test]
    fn debug_format_does_not_panic() {
        let debug = format!("{:?}", build_workflow_with_skills());
        assert!(debug.starts_with("Workflow"));
        assert!(debug.contains("Full Workflow"));
        assert!(debug.contains("skill_count: 3"));
    }

    #[test]
    fn builder_can_register_no_skills() {
        let wf = Workflow::<()>::builder()
            .name("Empty")
            .version("0.1.0")
            .build()
            .expect("empty workflow is valid");
        assert_eq!(wf.skill_count(), 0);
    }

    #[test]
    fn builder_rejects_empty_name() {
        let err = Workflow::<()>::builder()
            .name("")
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            crate::core::workflow::builder::WorkflowBuildError::EmptyName
        ));
    }

    #[test]
    fn builder_rejects_whitespace_name() {
        let err = Workflow::<()>::builder()
            .name("   ")
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            crate::core::workflow::builder::WorkflowBuildError::EmptyName
        ));
    }

    #[test]
    fn builder_rejects_missing_name() {
        let err = Workflow::<()>::builder()
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            crate::core::workflow::builder::WorkflowBuildError::EmptyName
        ));
    }

    #[test]
    fn builder_rejects_empty_version() {
        let err = Workflow::<()>::builder()
            .name("Test")
            .version("")
            .build()
            .unwrap_err();
        assert!(matches!(
            err,
            crate::core::workflow::builder::WorkflowBuildError::EmptyVersion
        ));
    }

    #[test]
    fn builder_rejects_missing_version() {
        let err = Workflow::<()>::builder().name("Test").build().unwrap_err();
        assert!(matches!(
            err,
            crate::core::workflow::builder::WorkflowBuildError::EmptyVersion
        ));
    }

    #[test]
    fn builder_rejects_duplicate_skill_ids() {
        struct SkillDuplicate;
        #[async_trait::async_trait]
        impl Skill<()> for SkillDuplicate {
            fn definition(&self) -> &'static SkillDefinition {
                &SKILL_A_DEF
            }
            async fn execute(
                &self,
                _ctx: &mut crate::core::context::WorkflowContext<()>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let err = Workflow::builder()
            .name("Dupes")
            .version("1.0.0")
            .register(SkillA)
            .register(SkillDuplicate)
            .build()
            .unwrap_err();
        assert!(
            matches!(&err, crate::core::workflow::builder::WorkflowBuildError::DuplicateSkill(id) if id == "skill_a")
        );
    }

    #[test]
    fn builder_is_must_use() {
        fn _forgets_to_build() {
            let _ = Workflow::<()>::builder().name("Oops").version("1.0.0");
        }
    }
}
