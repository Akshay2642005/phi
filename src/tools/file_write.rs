//! Tool skill — File Write
//!
//! Writes content to a file on disk, CWD-restricted with path escape
//! validation.  The file path and content are extracted from the goal.
//!
//! Slot read:    `ctx.input.goal`  — goal containing path and content
//! Slot written: `"file_write"`    → [`FileWriteOutput`]
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

/// Output produced by the file write skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteOutput {
    pub path: String,
    pub bytes_written: usize,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "file_write",
    name: "File Write",
    description: "Writes content to a file on disk with CWD-restricted path validation.",
    version: "1.0.0",
    dependencies: &[],
};

pub struct FileWriteSkill;

impl FileWriteSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Skill<RuntimeInput> for FileWriteSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let goal = &ctx.input.goal;
        let (path, content) = extract_path_and_content(goal)?;

        // CWD restriction — reject path traversal.
        let safe_path = super::resolve_safe_parent(&path)?;

        tracing::info!(
            skill = DEFINITION.id,
            path = %safe_path.display(),
            content_len = content.len(),
            "Writing file"
        );

        // Create parent directories if needed.
        if let Some(parent) = safe_path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create parent directories")?;
        }

        fs::write(&safe_path, &content)
            .await
            .with_context(|| format!("failed to write '{}'", safe_path.display()))?;

        let bytes = content.len();
        let output = FileWriteOutput {
            path: safe_path.to_string_lossy().into_owned(),
            bytes_written: bytes,
        };

        tracing::info!(
            skill = DEFINITION.id,
            bytes,
            "File write complete"
        );

        ctx.set(DEFINITION.id, output);
        Ok(())
    }
}

/// Extract (path, content) from a goal string.
///
/// Uses a simple heuristic: the first word containing path-like characters
/// is treated as the path, and the rest is the content.  If only a path
/// is found, content is empty.
fn extract_path_and_content(goal: &str) -> Result<(String, String)> {
    let words: Vec<&str> = goal.split_whitespace().collect();
    if words.is_empty() {
        return Err(anyhow!("empty goal: no path or content provided"));
    }

    // Find the first word that looks like a file path (contains a path
    // separator, dot, or underscore).
    let path_idx = words.iter().position(|w| {
        w.contains('/') || w.contains('\\') || w.contains('.') || w.contains('_')
    });

    if let Some(idx) = path_idx {
        let path = words[idx].to_string();
        let content: String = words[(idx + 1)..].join(" ");
        Ok((path, content))
    } else {
        // No path-like word found; treat the whole goal as a path with
        // empty content.
        Ok((goal.trim().to_string(), String::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::WorkflowContext;
    use tokio::fs;

    #[tokio::test]
    async fn writes_file_in_cwd() {
        let skill = FileWriteSkill::new();
        let goal = format!(
            "write _test_file_write.txt hello world from file_write skill"
        );
        let input = RuntimeInput {
            goal: goal.clone(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill.execute(&mut ctx).await.expect("should write file");

        let output = ctx
            .get::<FileWriteOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!(output.bytes_written > 0, "should write non-zero bytes");
        assert!(output.path.contains("_test_file_write.txt"));

        // Verify contents on disk.
        let contents = fs::read_to_string("_test_file_write.txt")
            .await
            .expect("should read back");
        assert!(
            contents.contains("hello world"),
            "should contain written content"
        );

        // Cleanup.
        fs::remove_file("_test_file_write.txt")
            .await
            .ok();
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let skill = FileWriteSkill::new();
        let input = RuntimeInput {
            goal: "write ../../../etc/evil.conf malicious content".to_string(),
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
    async fn fails_on_empty_goal() {
        let skill = FileWriteSkill::new();
        let input = RuntimeInput {
            goal: "".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "empty goal should fail");
    }
}
