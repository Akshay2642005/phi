//! Generic skill layer — the fundamental units of work in a workflow.
//!
//! Key exports:
//! - [`Skill`] — the trait every skill must implement.
//! - [`SkillRef`] — engine-internal `Arc<dyn Skill<I>>`.
//! - [`SkillDefinition`] — static metadata (id, name, deps).
//! - [`SkillRegistry`] — storage and topological ordering.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;

use crate::core::{capability::Capability, context::WorkflowContext};

// ── SkillDefinition ───────────────────────────────────────────────────────────

/// Static metadata describing a skill.
///
/// Definitions are typically declared as module-level `const` values and
/// referenced via the [`Skill::definition`] method.
///
/// # Example
/// ```ignore
/// pub const MY_SKILL: SkillDefinition = SkillDefinition {
///     id: "my_skill",
///     name: "My Skill",
///     description: "Does something useful",
///     version: "1.0.0",
///     dependencies: &["other_skill"],
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    /// Unique, stable identifier used as the dependency key and slot-map key.
    ///
    /// Snake-case by convention (e.g., `"document_summarizer"`).
    pub id: &'static str,

    /// Human-readable display name.
    pub name: &'static str,

    /// One-line description of what the skill produces.
    pub description: &'static str,

    /// Semver-style version string.
    pub version: &'static str,

    /// IDs of skills that must complete before this skill can execute.
    ///
    /// The engine uses this to build the execution DAG.  An empty slice
    /// means the skill has no prerequisites and may run immediately.
    pub dependencies: &'static [&'static str],
}

// ── Skill trait ───────────────────────────────────────────────────────────────

/// A single, reusable unit of AI work within a workflow.
///
/// `I` is the domain input type stored in the [`WorkflowContext`].  The
/// engine is generic over `I`, so any skill that implements `Skill<I>` for
/// the same `I` can be registered with the same engine instance.
///
/// # Contract
/// - `definition()` must return the same static value every call.
/// - `execute()` must be idempotent with respect to side-effects when
///   practical (so it can be safely retried on transient failures).
/// - `execute()` must write its primary output to `ctx.slots` under
///   `definition().id` before returning `Ok(())`.
///
/// # Capabilities
/// Skills may override [`required_capabilities`] and [`provided_capabilities`]
/// to declare what the runtime environment must provide vs. what they
/// contribute.  The default implementations return empty slices, making
/// capability declarations opt-in.
#[async_trait]
pub trait Skill<I>: Send + Sync
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Static metadata: id, name, version, dependencies.
    fn definition(&self) -> &'static SkillDefinition;

    /// Execute the skill against the shared context.
    ///
    /// The engine calls this after all declared dependencies have completed.
    /// The skill should:
    /// 1. Read prerequisite outputs from `ctx.get::<T>(dep_id)`.
    /// 2. Perform its work (typically an LLM call).
    /// 3. Write its output to `ctx.set(self.definition().id, output)`.
    ///
    /// Returning `Err` causes the engine to record the skill as failed and,
    /// depending on the retry policy, either retry or abort the workflow.
    async fn execute(&self, ctx: &mut WorkflowContext<I>) -> Result<()>;

    /// Capabilities this skill **requires** to function correctly.
    ///
    /// The engine checks that all required capabilities are present in the
    /// runtime environment before scheduling this skill.
    ///
    /// Default: empty (no special requirements).
    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    /// Capabilities this skill **provides** to the runtime.
    ///
    /// For example, a skill that wraps an LLM call may declare
    /// `[Capability::Llm]` here.
    ///
    /// Default: empty (no provisions).
    fn provided_capabilities(&self) -> &[Capability] {
        &[]
    }
}

/// A cheaply-cloneable reference to any `Skill<I>` implementation.
pub type SkillRef<I> = Arc<dyn Skill<I>>;

// ── SkillRegistry ─────────────────────────────────────────────────────────────

/// Stores all skills registered for a workflow and resolves dependencies.
///
/// `I` is the domain input type; all skills in a single registry must share
/// the same `I`.
pub struct SkillRegistry<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Registered skills keyed by `SkillDefinition.id`.
    skills: HashMap<String, SkillRef<I>>,
}

impl<I> SkillRegistry<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Register a skill.
    ///
    /// If a skill with the same ID was already registered, the new one
    /// replaces it.
    pub fn register(&mut self, skill: Arc<dyn Skill<I>>) {
        self.skills.insert(skill.definition().id.to_string(), skill);
    }

    /// Returns `true` if a skill with `id` is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.skills.contains_key(id)
    }

    /// Returns an iterator over all registered `(id, skill)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &SkillRef<I>)> {
        self.skills.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Borrow the internal skill map.
    pub fn skills(&self) -> &HashMap<String, SkillRef<I>> {
        &self.skills
    }

    /// Resolve the execution order via depth-first topological sort.
    ///
    /// Returns skills in an order where every dependency appears before its
    /// dependants.  Returns an error if a cycle is detected or a dependency is
    /// missing.
    pub fn resolve_execution_order(&self) -> Result<Vec<SkillRef<I>>> {
        let mut ordered: Vec<SkillRef<I>> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut visiting: HashSet<String> = HashSet::new();

        let mut ids: Vec<&str> = self.skills.keys().map(String::as_str).collect();
        ids.sort_unstable();

        for id in ids {
            let skill = self.skills[id].clone();
            self.visit(&skill, &mut visited, &mut visiting, &mut ordered)?;
        }

        Ok(ordered)
    }

    fn visit(
        &self,
        skill: &SkillRef<I>,
        visited: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
        ordered: &mut Vec<SkillRef<I>>,
    ) -> Result<()> {
        let def = skill.definition();

        if visiting.contains(def.id) {
            return Err(anyhow!(
                "dependency cycle detected involving skill '{}'",
                def.id
            ));
        }

        if visited.contains(def.id) {
            return Ok(());
        }

        visiting.insert(def.id.to_string());

        for dep_id in def.dependencies {
            let dep = self.skills.get(*dep_id).ok_or_else(|| {
                anyhow!(
                    "skill '{}' requires '{}' which is not registered",
                    def.id,
                    dep_id
                )
            })?;
            self.visit(dep, visited, visiting, ordered)?;
        }

        visiting.remove(def.id);
        visited.insert(def.id.to_string());
        ordered.push(Arc::clone(skill));

        Ok(())
    }
}

impl<I> Default for SkillRegistry<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
