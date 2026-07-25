//! Tool skill — File Read
//!
//! Reads a file from disk, CWD-restricted.  The file path is extracted
//! from the user's goal string.
//!
//! Slot read:    `ctx.input.goal`  — goal from which the path is parsed
//! Slot written: `"file_read"`     → [`FileReadOutput`]
//! Dependencies: none

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

/// Output produced by the file read skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadOutput {
    pub path: String,
    pub contents: String,
    pub bytes: usize,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "file_read",
    name: "File Read",
    description: "Reads a file from disk and returns its contents.",
    version: "1.0.0",
    dependencies: &[],
};

pub struct FileReadSkill;

impl FileReadSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Skill<RuntimeInput> for FileReadSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let goal = &ctx.input.goal;
        let path = extract_path(goal)?;

        // CWD restriction — reject path traversal outside current directory.
        let safe_path = super::resolve_safe_path(&path)?;

        tracing::info!(
            skill = DEFINITION.id,
            original = %path,
            resolved = %safe_path.display(),
            "Reading file"
        );

        let contents = fs::read_to_string(&safe_path)
            .await
            .with_context(|| format!("failed to read '{}'", safe_path.display()))?;

        let bytes = contents.len();
        let output = FileReadOutput {
            path: safe_path.to_string_lossy().into_owned(),
            contents,
            bytes,
        };

        tracing::info!(
            skill = DEFINITION.id,
            bytes,
            "File read complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

/// Extract a file path from the goal string.
///
/// Heuristic: take the last word that looks like a path (contains '/' or
/// '.', or matches a known path pattern).
fn extract_path(goal: &str) -> Result<String> {
    // Try to find a word containing path-like characters.
    let candidates: Vec<&str> = goal
        .split_whitespace()
        .filter(|w| w.contains('/') || w.contains('\\') || w.contains('.'))
        .collect();

    if let Some(path) = candidates.last() {
        return Ok(path.to_string());
    }

    // Fallback: use the entire goal as a path if it's short enough.
    let trimmed = goal.trim();
    if !trimmed.is_empty() && trimmed.len() < 512 {
        return Ok(trimmed.to_string());
    }

    Err(anyhow!(
        "could not extract a file path from goal: '{}'",
        goal
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::WorkflowContext;

    #[tokio::test]
    async fn reads_existing_file() {
        let skill = FileReadSkill::new();
        let input = RuntimeInput {
            goal: "read Cargo.toml".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill.execute(&mut ctx).await.expect("should read Cargo.toml");

        let output = ctx
            .get::<FileReadOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!(output.contents.contains("[package]"), "should contain Cargo.toml content");
        assert!(output.bytes > 0, "should read non-zero bytes");
        assert!(output.path.contains("Cargo.toml"), "path should include Cargo.toml");
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let skill = FileReadSkill::new();
        let input = RuntimeInput {
            goal: "read ../../../etc/passwd".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "path traversal should be rejected");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("traversal") || err.contains("denied"),
            "error should mention traversal: {}",
            err
        );
    }

    #[tokio::test]
    async fn fails_on_nonexistent_file() {
        let skill = FileReadSkill::new();
        let input = RuntimeInput {
            goal: "read /nonexistent_file_xyz.txt".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "nonexistent file should fail");
    }
}
