//! Tool skills — skills that interact with real external systems.
//!
//! These are the "hands" of the digital worker. Unlike analysis skills
//! that only call the LLM, tool skills reach out to the world:
//! web search, HTTP calls, file I/O, code execution, etc.
//!
//! All tool skills implement `Skill<RuntimeInput>` so they can be
//! registered in the same registry as analysis skills.

use std::path::PathBuf;

use anyhow::{Context, Result};

pub mod calculator;
pub mod file_read;
pub mod file_write;
pub mod shell_command;
pub mod web_search;

/// Resolve a path relative to CWD, rejecting traversal outside, but
/// without requiring the final path component to exist.
///
/// This is useful for [`file_write`] where the file does not exist yet.
pub fn resolve_safe_parent(path: &str) -> Result<PathBuf> {
    // Pre-check: reject obvious path traversal before any I/O.
    let components: Vec<&str> = path.split(['/', '\\']).collect();
    if components.contains(&"..") {
        anyhow::bail!(
            "path traversal denied: '{}' contains '..'",
            path
        );
    }

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let cwd_canonical = cwd.canonicalize().context("failed to canonicalize CWD")?;
    let resolved = cwd.join(path);

    // Canonicalize the parent directory (which must exist).
    if let Some(parent) = resolved.parent() {
        let canonical_parent = parent
            .canonicalize()
            .with_context(|| format!("parent of '{}' does not exist or cannot be accessed", path))?;

        let canonical_parent_str = canonical_parent.to_string_lossy().to_lowercase();
        let cwd_str = cwd_canonical.to_string_lossy().to_lowercase();
        if !canonical_parent_str.starts_with(&cwd_str) {
            anyhow::bail!(
                "path traversal denied: '{}' parent is outside the working directory",
                path
            );
        }
    }

    // Return the non-canonical resolved path (file doesn't exist yet).
    Ok(resolved)
}

/// Resolve a path relative to CWD, rejecting traversal outside.
///
/// Shared by [`file_read`] and [`file_write`] for consistent path
/// safety enforcement.
pub fn resolve_safe_path(path: &str) -> Result<PathBuf> {
    // Pre-check: reject obvious path traversal before any I/O.
    let components: Vec<&str> = path.split(['/', '\\']).collect();
    if components.contains(&"..") {
        anyhow::bail!(
            "path traversal denied: '{}' contains '..'",
            path
        );
    }

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let cwd_canonical = cwd.canonicalize().context("failed to canonicalize CWD")?;
    let resolved = cwd.join(path);

    // Canonicalize to resolve symlinks and verify the path exists.
    let canonical = resolved
        .canonicalize()
        .with_context(|| format!("path '{}' does not exist or cannot be accessed", path))?;

    // Ensure the canonical path is under CWD.  Uses the canonical form
    // of both paths for reliable prefix matching (handles Windows path
    // prefixes and case differences).
    let canonical_str = canonical.to_string_lossy().to_lowercase();
    let cwd_str = cwd_canonical.to_string_lossy().to_lowercase();
    if !canonical_str.starts_with(&cwd_str) {
        anyhow::bail!(
            "path traversal denied: '{}' resolves outside the working directory",
            path
        );
    }

    Ok(canonical)
}
