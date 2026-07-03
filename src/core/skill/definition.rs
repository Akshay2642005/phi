//! Skill definition — static metadata for a skill.
//!
//! [`SkillDefinition`] is `'static` so it can live in a `const` without any
//! allocation.  Every skill implementation returns a reference to its own
//! definition from [`Skill::definition`].

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
    /// Snake-case by convention (e.g., `"schema_classification"`).
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
