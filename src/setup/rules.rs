// SPDX-License-Identifier: Apache-2.0
//! Rules file templates for each AI client.
//!
//! Each client gets behavioral instructions that guide proactive memory use.
//! Uses <!-- EREBYX:START --> / <!-- EREBYX:END --> markers for idempotent updates.

use anyhow::{Context, Result};
use std::path::PathBuf;

use super::detect::{AiClient, ClientKind};

/// The universal rules content that works across all AI models.
/// Aligned to v0.1.1 launch surface — five cognitive verbs only.
const RULES_CONTENT: &str = r#"# Erebyx Memory Integration

You have access to persistent memory through Erebyx. Use it proactively.

## Session Start
1. Call `restore_identity()` — loads your persistent identity and values.
2. Call `load_context()` — loads where you left off, recent handoffs, related memories.

## During Conversation
- **Before answering substantive questions:** Call `remember('topic')` to check for relevant past context. There's no reason to skip it.
- **When the user shares important information:** Call `save()` immediately. Don't ask permission. Save decisions, preferences, project details, and anything you'd want to know next time.

## Session End
- **Always call `wrap_up()`** before the session ends. This creates a handoff so the next session starts with full context instead of cold.

## Key Principle
Memory makes your responses dramatically more relevant. The user expects you to remember past conversations. Check memory first, respond second.
"#;

/// Write the rules file for a specific client.
/// Uses markers for idempotent re-runs.
pub fn write_rules_file(client: &AiClient) -> Result<PathBuf> {
    // Ensure parent directory exists
    if let Some(parent) = client.rules_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    match client.kind {
        ClientKind::Windsurf => write_append_rules(client),
        ClientKind::VsCodeCopilot => write_append_rules(client),
        _ => write_standalone_rules(client),
    }
}

/// Write a standalone rules file (Claude Code, Cursor, Continue, Zed).
/// These clients support dedicated rules files per tool.
fn write_standalone_rules(client: &AiClient) -> Result<PathBuf> {
    let content = format_with_markers(RULES_CONTENT);
    std::fs::write(&client.rules_path, &content)
        .with_context(|| format!("Failed to write rules to {}", client.rules_path.display()))?;
    Ok(client.rules_path.clone())
}

/// Append rules to an existing file (Windsurf .windsurfrules, Copilot instructions).
/// These clients use a single shared rules file, so we append with markers.
fn write_append_rules(client: &AiClient) -> Result<PathBuf> {
    let marked_content = format_with_markers(RULES_CONTENT);

    if client.rules_path.exists() {
        let existing = std::fs::read_to_string(&client.rules_path)
            .with_context(|| format!("Failed to read {}", client.rules_path.display()))?;

        // Remove old erebyx section if present
        let cleaned = remove_erebyx_section(&existing);

        // Append new section
        let new_content = if cleaned.trim().is_empty() {
            marked_content
        } else {
            format!("{}\n\n{}", cleaned.trim_end(), marked_content)
        };

        std::fs::write(&client.rules_path, &new_content)
            .with_context(|| format!("Failed to write {}", client.rules_path.display()))?;
    } else {
        std::fs::write(&client.rules_path, &marked_content)
            .with_context(|| format!("Failed to write {}", client.rules_path.display()))?;
    }

    Ok(client.rules_path.clone())
}

/// Wrap content with EREBYX markers for idempotent updates.
fn format_with_markers(content: &str) -> String {
    format!(
        "<!-- EREBYX:START -->\n{}\n<!-- EREBYX:END -->",
        content.trim()
    )
}

/// Remove existing EREBYX section from a file's content.
fn remove_erebyx_section(content: &str) -> String {
    if let (Some(start), Some(end)) = (
        content.find("<!-- EREBYX:START -->"),
        content.find("<!-- EREBYX:END -->"),
    ) {
        // Defensive: if a user manually edited the file so END appears before
        // START, return content unchanged rather than producing a corrupt slice.
        if start > end {
            return content.to_string();
        }
        let end = end + "<!-- EREBYX:END -->".len();
        let before = &content[..start];
        let after = &content[end..];
        format!("{}{}", before.trim_end(), after.trim_start())
    } else {
        content.to_string()
    }
}
