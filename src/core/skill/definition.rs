//! Skill definition — static metadata for a skill.
//!
//! [`SkillDefinition`] is `'static` so it can live in a `const` without any
//! allocation.  Every skill implementation returns a reference to its own
//! definition from [`Skill::definition`].

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub dependencies: &'static [&'static str],
}
