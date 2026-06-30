use crate::core::skill::Skill;
use anyhow::{Result, anyhow};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct SkillRegistry {
    skills: HashMap<String, Arc<dyn Skill>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    pub fn register(&mut self, skill: Arc<dyn Skill>) {
        self.skills.insert(skill.definition().id.to_string(), skill);
    }

    pub fn resolve_execution_order(&self) -> Result<Vec<Arc<dyn Skill>>> {
        let mut ordered = Vec::new();
        let mut visited = HashSet::new();
        let mut visiting = HashSet::new();

        for skill in self.skills.values() {
            self.visit(skill.clone(), &mut visited, &mut visiting, &mut ordered)?;
        }

        Ok(ordered)
    }

    fn visit(
        &self,
        skill: Arc<dyn Skill>,
        visited: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
        ordered: &mut Vec<Arc<dyn Skill>>,
    ) -> Result<()> {
        let definition = skill.definition();

        if visiting.contains(definition.id) {
            return Err(anyhow!(
                "Cycle detected: skill '{}' is already in the current visit path",
                definition.id
            ));
        }

        if visited.contains(definition.id) {
            return Ok(());
        }

        visiting.insert(definition.id.to_string());

        for dep in definition.dependencies {
            let dependency = self.skills.get(*dep).ok_or_else(|| {
                anyhow!(
                    "Missing dependency '{}' required by '{}'",
                    dep,
                    definition.id
                )
            })?;

            self.visit(dependency.clone(), visited, visiting, ordered)?;
        }

        visiting.remove(definition.id);
        visited.insert(definition.id.to_string());

        ordered.push(skill);

        Ok(())
    }
}
