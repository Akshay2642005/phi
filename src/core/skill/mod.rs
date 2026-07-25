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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub dependencies: &'static [&'static str],
}

#[async_trait]
pub trait Skill<I>: Send + Sync
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    fn definition(&self) -> &'static SkillDefinition;
    async fn execute(&self, ctx: &mut WorkflowContext<I>) -> Result<()>;

    fn required_capabilities(&self) -> &[Capability] {
        &[]
    }

    fn provided_capabilities(&self) -> &[Capability] {
        &[]
    }
}

pub type SkillRef<I> = Arc<dyn Skill<I>>;

pub struct SkillRegistry<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    skills: HashMap<String, SkillRef<I>>,
}

impl<I> SkillRegistry<I>
where
    I: Send + Sync + std::fmt::Debug + 'static,
{
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    pub fn register(&mut self, skill: Arc<dyn Skill<I>>) {
        self.skills.insert(skill.definition().id.to_string(), skill);
    }

    pub fn contains(&self, id: &str) -> bool {
        self.skills.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &SkillRef<I>)> {
        self.skills.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn skills(&self) -> &HashMap<String, SkillRef<I>> {
        &self.skills
    }

    /// Create a new registry containing only skills whose IDs are in
    /// `ids`.  Useful for building sub-registries in the ReAct loop.
    pub fn filter_to_ids(&self, ids: &[&str]) -> Self {
        let mut filtered = HashMap::new();
        for id in ids {
            if let Some(skill) = self.skills.get(*id) {
                filtered.insert(id.to_string(), Arc::clone(skill));
            }
        }
        Self { skills: filtered }
    }

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
