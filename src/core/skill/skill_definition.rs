pub struct SkillDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub dependencies: &'static [&'static str],
}
