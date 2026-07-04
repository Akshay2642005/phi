//! Builder for constructing an immutable [`Workflow<I>`].
//!
//! # Usage
//! ```ignore
//! let workflow = Workflow::builder()
//!     .name("Example Workflow")
//!     .description("Demonstrates the builder API")
//!     .version("1.0.0")
//!     .author("developer")
//!     .tag("demo")
//!     .register(MySkill)
//!     .register(AnotherSkill)
//!     .build()?;
//! ```
//!
//! # Validation
//! The following checks run at `build()` time:
//!
//! | Check | Error |
//! |-------|-------|
//! | `name` is non-empty after trimming | [`EmptyName`](WorkflowBuildError::EmptyName) |
//! | `version` is non-empty after trimming | [`EmptyVersion`](WorkflowBuildError::EmptyVersion) |
//! | No duplicate skill IDs | [`DuplicateSkill`](WorkflowBuildError::DuplicateSkill) |
//!
//! Dependency validation (e.g., a skill references a missing dependency)
//! belongs to the graph validator in a later phase — it is **not** checked
//! here.
//!
//! # Ownership
//! All setter methods take and return `Self` (consuming the builder) to
//! support a fluent-chain API.  The builder is marked `#[must_use]` so
//! that callers are warned if they forget to call `.build()`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use thiserror::Error;

use crate::core::ids::WorkflowId;
use crate::core::skill::{Skill, SkillRef, SkillRegistry};

use super::definition::Workflow;
use super::metadata::WorkflowMetadata;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum WorkflowBuildError {
    #[error("workflow name is required and cannot be empty")]
    EmptyName,
    #[error("workflow version is required and cannot be empty")]
    EmptyVersion,
    #[error("duplicate skill registration: skill '{0}' is already registered")]
    DuplicateSkill(String),
}

#[must_use = "WorkflowBuilder does nothing until .build() is called"]
pub struct WorkflowBuilder<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    skills: Vec<(String, SkillRef<I>)>,
    metadata: WorkflowMetadata,
}

impl<I> std::fmt::Debug for WorkflowBuilder<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowBuilder")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("skill_count", &self.skills.len())
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl<I> WorkflowBuilder<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    #[inline]
    pub fn new() -> Self {
        Self {
            name: None,
            description: None,
            version: None,
            skills: Vec::new(),
            metadata: WorkflowMetadata::new(),
        }
    }

    #[inline]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[inline]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[inline]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    #[inline]
    pub fn register<S>(mut self, skill: S) -> Self
    where
        S: Skill<I> + 'static,
    {
        let skill_ref: SkillRef<I> = Arc::new(skill);
        let id = skill_ref.definition().id.to_string();
        self.skills.push((id, skill_ref));
        self
    }

    #[inline]
    pub fn register_arc(mut self, skill: SkillRef<I>) -> Self {
        let id = skill.definition().id.to_string();
        self.skills.push((id, skill));
        self
    }

    #[inline]
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.metadata.set_author(author.into());
        self
    }

    #[inline]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.metadata.add_tag(tag.into());
        self
    }

    #[inline]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert_label(key.into(), value.into());
        self
    }

    #[inline]
    pub fn custom_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert_custom(key.into(), value.into());
        self
    }

    pub fn build(mut self) -> Result<Workflow<I>, WorkflowBuildError> {
        let name = self.name.ok_or(WorkflowBuildError::EmptyName)?;
        if name.trim().is_empty() {
            return Err(WorkflowBuildError::EmptyName);
        }
        let version = self.version.ok_or(WorkflowBuildError::EmptyVersion)?;
        if version.trim().is_empty() {
            return Err(WorkflowBuildError::EmptyVersion);
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(self.skills.len());
        for (id, _) in &self.skills {
            if !seen.insert(id.as_str()) {
                return Err(WorkflowBuildError::DuplicateSkill(id.clone()));
            }
        }
        let mut registry = SkillRegistry::new();
        for (_, skill) in self.skills {
            registry.register(skill);
        }
        self.metadata.set_created_at(SystemTime::now());
        Ok(Workflow::new(
            WorkflowId::new(),
            name,
            self.description,
            version,
            registry,
            self.metadata,
        ))
    }
}

impl<I> Default for WorkflowBuilder<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill::SkillDefinition;

    const ALPHA_DEF: SkillDefinition = SkillDefinition {
        id: "alpha",
        name: "Alpha",
        description: "First test skill for builder tests",
        version: "1.0.0",
        dependencies: &[],
    };

    const BETA_DEF: SkillDefinition = SkillDefinition {
        id: "beta",
        name: "Beta",
        description: "Second test skill for builder tests",
        version: "1.0.0",
        dependencies: &[],
    };

    struct Alpha;
    struct Beta;
    struct AlphaDuplicate;

    #[async_trait::async_trait]
    impl Skill<()> for Alpha {
        fn definition(&self) -> &'static SkillDefinition {
            &ALPHA_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Skill<()> for Beta {
        fn definition(&self) -> &'static SkillDefinition {
            &BETA_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl Skill<()> for AlphaDuplicate {
        fn definition(&self) -> &'static SkillDefinition {
            &ALPHA_DEF
        }
        async fn execute(
            &self,
            _ctx: &mut crate::core::context::WorkflowContext<()>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn builder_new_is_empty() {
        let err = WorkflowBuilder::<()>::new().build().unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyName);
    }

    #[test]
    fn builder_accepts_minimal_fields() {
        let wf = Workflow::<()>::builder()
            .name("Minimal")
            .version("0.0.1")
            .build()
            .expect("minimal build should succeed");
        assert_eq!(wf.name(), "Minimal");
        assert_eq!(wf.version(), "0.0.1");
    }

    #[test]
    fn builder_rejects_missing_name() {
        let err = Workflow::<()>::builder()
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyName);
    }

    #[test]
    fn builder_rejects_empty_name() {
        let err = Workflow::<()>::builder()
            .name("")
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyName);
    }

    #[test]
    fn builder_rejects_whitespace_name() {
        let err = Workflow::<()>::builder()
            .name("   ")
            .version("1.0.0")
            .build()
            .unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyName);
    }

    #[test]
    fn builder_rejects_missing_version() {
        let err = Workflow::<()>::builder().name("Test").build().unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyVersion);
    }

    #[test]
    fn builder_rejects_empty_version() {
        let err = Workflow::<()>::builder()
            .name("Test")
            .version("")
            .build()
            .unwrap_err();
        assert_eq!(err, WorkflowBuildError::EmptyVersion);
    }

    #[test]
    fn builder_rejects_duplicate_skills() {
        let err = Workflow::<()>::builder()
            .name("Dupes")
            .version("1.0.0")
            .register(Alpha)
            .register(AlphaDuplicate)
            .build()
            .unwrap_err();
        assert_eq!(err, WorkflowBuildError::DuplicateSkill("alpha".into()));
    }

    #[test]
    fn builder_accepts_distinct_skills() {
        let wf = Workflow::<()>::builder()
            .name("Skills")
            .version("1.0.0")
            .register(Alpha)
            .register(Beta)
            .build()
            .expect("distinct skills should succeed");
        assert_eq!(wf.skill_count(), 2);
    }

    #[test]
    fn builder_register_arc_accepts_pre_wrapped_skill() {
        let skill: SkillRef<()> = Arc::new(Alpha);
        let wf = Workflow::<()>::builder()
            .name("Arc")
            .version("1.0.0")
            .register_arc(skill)
            .build()
            .expect("register_arc should work");
        assert!(wf.contains_skill("alpha"));
    }

    #[test]
    fn builder_sets_description() {
        let wf = Workflow::<()>::builder()
            .name("Desc")
            .version("1.0.0")
            .description("A test workflow")
            .build()
            .expect("description should work");
        assert_eq!(wf.description(), Some("A test workflow"));
    }

    #[test]
    fn builder_metadata_author() {
        let meta = Workflow::<()>::builder()
            .name("Authored")
            .version("1.0.0")
            .author("Dr. Jones")
            .build()
            .expect("author should work")
            .metadata()
            .author()
            .unwrap()
            .to_string();
        assert_eq!(meta, "Dr. Jones");
    }

    #[test]
    fn builder_metadata_tags() {
        let meta = Workflow::<()>::builder()
            .name("Tagged")
            .version("1.0.0")
            .tag("critical")
            .tag("production")
            .build()
            .expect("tags should work")
            .metadata()
            .clone();
        assert_eq!(meta.tags().len(), 2);
        assert_eq!(meta.tags()[0], "critical");
        assert_eq!(meta.tags()[1], "production");
    }

    #[test]
    fn builder_metadata_labels() {
        let meta = Workflow::<()>::builder()
            .name("Labeled")
            .version("1.0.0")
            .label("env", "prod")
            .label("team", "platform")
            .build()
            .expect("labels should work")
            .metadata()
            .clone();
        assert_eq!(meta.labels().len(), 2);
        assert_eq!(meta.labels().get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn builder_metadata_custom() {
        let meta = Workflow::<()>::builder()
            .name("Custom")
            .version("1.0.0")
            .custom_meta("source", "github")
            .build()
            .expect("custom meta should work")
            .metadata()
            .clone();
        assert_eq!(
            meta.custom().get("source").map(String::as_str),
            Some("github")
        );
    }

    #[test]
    fn builder_stamps_creation_time() {
        let before = SystemTime::now();
        let wf = Workflow::<()>::builder()
            .name("Timely")
            .version("1.0.0")
            .build()
            .expect("should build");
        let after = SystemTime::now();
        let created = wf.metadata().created_at().unwrap();
        assert!(
            created >= before && created <= after,
            "created_at must be between before and after"
        );
    }

    #[test]
    fn builder_consumes_on_build() {
        let builder = Workflow::<()>::builder().name("Consumed").version("1.0.0");
        let _wf = builder.build().expect("build should succeed");
    }

    #[test]
    fn builder_default_is_new() {
        let r1 = WorkflowBuilder::<()>::default()
            .name("Test")
            .version("1.0.0")
            .build();
        let r2 = WorkflowBuilder::<()>::new()
            .name("Test")
            .version("1.0.0")
            .build();
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }

    #[test]
    fn builder_workflow_build_error_is_debug_and_display() {
        let err = WorkflowBuildError::EmptyName;
        assert_eq!(
            format!("{err}"),
            "workflow name is required and cannot be empty"
        );
        assert!(format!("{err:?}").contains("EmptyName"));
    }

    #[test]
    fn builder_accepts_multiple_distinct_skills() {
        const GAMMA_DEF: SkillDefinition = SkillDefinition {
            id: "gamma",
            name: "Gamma",
            description: "Third test skill",
            version: "1.0.0",
            dependencies: &[],
        };
        struct Gamma;
        #[async_trait::async_trait]
        impl Skill<()> for Gamma {
            fn definition(&self) -> &'static SkillDefinition {
                &GAMMA_DEF
            }
            async fn execute(
                &self,
                _ctx: &mut crate::core::context::WorkflowContext<()>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }
        let wf = Workflow::<()>::builder()
            .name("Multiple")
            .version("1.0.0")
            .register(Alpha)
            .register(Beta)
            .register(Gamma)
            .build()
            .expect("three distinct skills should succeed");
        assert_eq!(wf.skill_count(), 3);
        assert!(wf.contains_skill("alpha"));
        assert!(wf.contains_skill("beta"));
        assert!(wf.contains_skill("gamma"));
    }

    #[test]
    fn builder_error_clone_and_eq() {
        let e1 = WorkflowBuildError::EmptyName;
        let e2 = WorkflowBuildError::EmptyName;
        assert_eq!(e1, e2);
        let e3 = WorkflowBuildError::DuplicateSkill("x".into());
        let e4 = WorkflowBuildError::DuplicateSkill("x".into());
        assert_eq!(e3, e4);
        let e5 = WorkflowBuildError::DuplicateSkill("x".into());
        let e6 = WorkflowBuildError::DuplicateSkill("y".into());
        assert_ne!(e5, e6);
    }
}
