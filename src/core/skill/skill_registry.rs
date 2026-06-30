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
        for skill in self.skills.values() {
            self.visit(skill.clone(), &mut visited, &mut ordered)?;
        }

        Ok(ordered)
    }

    fn visit(
        &self,
        skill: Arc<dyn Skill>,
        visited: &mut HashSet<String>,
        ordered: &mut Vec<Arc<dyn Skill>>,
    ) -> Result<()> {
        let definition = skill.definition();

        if visited.contains(definition.id) {
            return Ok(());
        }

        visited.insert(definition.id.to_string());

        for dep in definition.dependencies {
            let dependency = self
                .skills
                .get(*dep)
                .ok_or_else(|| anyhow!("Missing dependency {}", dep))?;

            self.visit(dependency.clone(), visited, ordered)?;
        }

        ordered.push(skill);

        Ok(())
    }
}
