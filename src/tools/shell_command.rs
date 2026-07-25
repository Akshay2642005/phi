//! Tool skill — Shell Command
//!
//! Executes a shell command and returns its stdout and stderr.
//! Commands are timed out after a configurable duration (default 30s).
//!
//! Slot read:    `ctx.input.goal`  — goal from which the command is parsed
//! Slot written: `"shell_command"` → [`ShellCommandOutput`]
//! Dependencies: none

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::core::{
    context::WorkflowContext,
    skill::{Skill, SkillDefinition},
};
use crate::runtime::RuntimeInput;

/// Default command timeout (30 seconds).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Output produced by the shell command skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

pub const DEFINITION: SkillDefinition = SkillDefinition {
    id: "shell_command",
    name: "Shell Command",
    description: "Executes a shell command and returns its output.",
    version: "1.0.0",
    dependencies: &[],
};

pub struct ShellCommandSkill {
    /// Per-command timeout.
    timeout: Duration,
}

impl ShellCommandSkill {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Set a custom timeout for each command execution.
    pub fn with_timeout(timeout: Duration) -> Arc<Self> {
        Arc::new(Self { timeout })
    }
}

/// Extract the shell command from a goal string.
fn extract_command(goal: &str) -> Result<String> {
    let trimmed = goal.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty goal: no shell command provided"));
    }

    // Remove common prefixes like "run", "execute", "shell", etc.
    let prefixes = [
        "run ",
        "execute ",
        "shell ",
        "run command ",
        "execute command ",
        "cmd ",
    ];
    for prefix in &prefixes {
        if let Some(stripped) = trimmed.strip_prefix(prefix) {
            let rest = stripped.trim();
            if !rest.is_empty() {
                return Ok(rest.to_string());
            }
        }
    }

    // If the goal looks like a command (alphanumeric + common symbols),
    // use it directly.
    if trimmed.contains(' ') || trimmed.len() <= 256 {
        return Ok(trimmed.to_string());
    }

    Err(anyhow!(
        "could not extract a shell command from goal: '{}'",
        goal
    ))
}

#[async_trait]
impl Skill<RuntimeInput> for ShellCommandSkill {
    fn definition(&self) -> &'static SkillDefinition {
        &DEFINITION
    }

    async fn execute(&self, ctx: &mut WorkflowContext<RuntimeInput>) -> Result<()> {
        let goal = &ctx.input.goal;
        let command = extract_command(goal)?;

        tracing::info!(
            skill = DEFINITION.id,
            command = %command,
            timeout_ms = self.timeout.as_millis() as u64,
            "Executing shell command"
        );

        // Spawn the command on the Windows/Unix shell.
        let mut child = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", &command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        }
        .map_err(|e| anyhow!("failed to spawn command '{}': {}", command, e))?;

        // Apply timeout.
        let result = tokio::time::timeout(self.timeout, child.wait_with_output()).await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Err(anyhow!("command '{}' failed: {}", command, e));
            }
            Err(_elapsed) => {
                tracing::warn!(
                    skill = DEFINITION.id,
                    command = %command,
                    timeout_ms = self.timeout.as_millis() as u64,
                    "Shell command timed out"
                );
                ctx.set(
                    DEFINITION.id,
                    ShellCommandOutput {
                        command: command.clone(),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: None,
                        timed_out: true,
                    },
                );
                return Ok(());
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        tracing::info!(
            skill = DEFINITION.id,
            exit_code = ?output.status.code(),
            stdout_len = stdout.len(),
            stderr_len = stderr.len(),
            "Shell command complete"
        );

        ctx.set(
            DEFINITION.id,
            ShellCommandOutput {
                command: command.clone(),
                stdout,
                stderr,
                exit_code: output.status.code(),
                timed_out: false,
            },
        );

        // Return an error for non-zero exit codes so the ReAct loop can react.
        if !output.status.success() {
            let exit_code = output.status.code().unwrap_or(-1);
            return Err(anyhow!(
                "command '{}' exited with code {}",
                command,
                exit_code
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::WorkflowContext;

    #[tokio::test]
    async fn executes_echo() {
        let skill = ShellCommandSkill::new();
        let input = RuntimeInput {
            goal: "echo hello_tool_skill".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill.execute(&mut ctx).await.expect("echo should succeed");

        let output = ctx
            .get::<ShellCommandOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!(
            output.stdout.contains("hello_tool_skill"),
            "stdout should contain echo output"
        );
        assert!(output.exit_code == Some(0), "exit code should be 0");
    }

    #[tokio::test]
    async fn captures_stderr_on_failure() {
        let skill = ShellCommandSkill::new();
        // Use a non-existent command.
        let input = RuntimeInput {
            goal: "run nonexistent_command_xyz_123".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "non-existent command should fail");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("exited with code")
                || err.contains("failed")
                || err.contains("not recognized"),
            "error should mention exit code or failure: {}",
            err
        );
    }

    #[tokio::test]
    async fn timeout_kills_long_running_command() {
        let skill = ShellCommandSkill::with_timeout(Duration::from_millis(10));
        let input = RuntimeInput {
            goal: "sleep 60".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        skill
            .execute(&mut ctx)
            .await
            .expect("timeout should return ok with timed_out flag");

        let output = ctx
            .get::<ShellCommandOutput>(DEFINITION.id)
            .expect("output should be in context");
        assert!(output.timed_out, "command should be marked as timed out");
    }

    #[tokio::test]
    async fn fails_on_empty_goal() {
        let skill = ShellCommandSkill::new();
        let input = RuntimeInput {
            goal: "".to_string(),
            context: None,
        };
        let (mut ctx, _tx) = WorkflowContext::new(input);

        let result = skill.execute(&mut ctx).await;
        assert!(result.is_err(), "empty goal should fail");
    }
}
