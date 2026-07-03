//! Skill registry — stores all registered skills and resolves their
//! topological execution order.
//!
//! The [`SkillRegistry`] owns every `Arc<dyn Skill<I>>` registered for a
//! workflow.  It exposes the skill map for use by the graph builder and the
//! planner, and provides a convenience method for resolving the execution
//! order via depth-first topological sort.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Result, anyhow};

use crate::core::skill::{SkillRef, skill::Skill};

// ── Registry ──────────────────────────────────────────────────────────────────

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
        self.skills
            .insert(skill.definition().id.to_string(), skill);
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

    /// Resolve the execution order via depth-first topological sort (Kahn's
    /// algorithm / recursive DFS variant).
    ///
    /// Returns skills in an order where every dependency appears before its
    /// dependants.  Returns an error if a cycle is detected or a dependency is
    /// missing.
    pub fn resolve_execution_order(&self) -> Result<Vec<SkillRef<I>>> {
        let mut ordered: Vec<SkillRef<I>> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut visiting: HashSet<String> = HashSet::new();

        // Iterate in a deterministic order for reproducibility.
        let mut ids: Vec<&str> = self.skills.keys().map(String::as_str).collect();
        ids.sort_unstable();

        for id in ids {
            let skill = self.skills[id].clone();
            self.visit(&skill, &mut visited, &mut visiting, &mut ordered)?;
        }

        Ok(ordered)
    }

    // ── Internal DFS helper ───────────────────────────────────────────────────

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
