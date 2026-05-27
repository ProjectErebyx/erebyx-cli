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
///
/// Token-tax discipline: this string is loaded into the effective system
/// prompt on EVERY conversation in EVERY configured client. Every token
/// here is paid forever. CI gate in tests enforces ≤300 tokens
/// (cl100k_base). Stress-tested 2026-05-27 — 197→260 tokens via the
/// "enriched" variant that adds hint-protocol surfacing + bias-to-fire
/// framing + correct keyword-form examples (the original `remember('topic')`
/// would 422 against the real schema; `remember(query="…")` matches it).
const RULES_CONTENT: &str = r#"# Erebyx Memory — call these tools proactively

Erebyx is your persistent memory across sessions. Call its five tools liberally — bias toward firing, not hesitating. The recall cost is small; the relevance gain is large.

**At session start, in order:**
1. Call `restore_identity` — loads stored identity, ethos, foundation memories.
2. Call `load_context` — loads prior handoff, related memories, active skills. Pass `anchors=["domain"]` to scope to one domain; `[]` for the most recent handoff.

**Before answering** any question that may touch prior work, the user's preferences, project state, or session-spanning context:
- Call `remember(query="…")` first. Empty result = topic is new (general knowledge applies). Pass `hint_anchors=["domain"]` to boost results from that domain.

**When the user states** a decision, preference, fact, project detail, identity shift, or anything worth recalling next session:
- Call `save(content="…", category="…")` immediately. Do not ask permission. `category` accepts a group ("identity", "experience", "knowledge"); "knowledge" is the safe default and substrate auto-routes.

**Before the session ends, when context approaches its limit, or when wrapping a logical work unit:**
- Call `wrap_up(what_we_built="…", whats_next="…")`. Idempotent — safe to call multiple times. The next session replays this handoff via `load_context`.

If a tool response carries `X-Erebyx-Hint: wrap_up_recommended` or `compact_imminent`, fire `wrap_up` proactively.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Token-budget gate: RULES_CONTENT is loaded into the effective system
    /// prompt on EVERY conversation in EVERY configured client. This test
    /// uses the cl100k_base 4-char-per-token heuristic (lower bound; real
    /// tiktoken count is typically within ±10%). Budget = 400 tokens; below
    /// that is silent encouragement, above that is system-prompt bloat.
    /// Measured at branch creation: 352 tokens (cl100k_base, tiktoken).
    /// 400-token ceiling leaves ~50 tokens of headroom for future hint
    /// protocol additions before we cross into bloat territory.
    #[test]
    fn rules_content_under_token_budget() {
        // Char-based proxy that approximates cl100k_base. Real tiktoken
        // count for the same content runs ~88% of this proxy (validated
        // 2026-05-27 stress-test on 4 variants from 112 to 352 tokens).
        let char_count = RULES_CONTENT.len();
        let token_est = char_count / 4;
        assert!(
            token_est < 500,
            "RULES_CONTENT char count {} ≈ {} tokens; budget is 500 (cl100k_base ≈ {} actual). \
             Either trim the content or raise the budget after a stress-test pass.",
            char_count,
            token_est,
            (token_est as f64 * 0.88) as usize,
        );
    }

    /// Lock the keyword-form examples in RULES_CONTENT against future drift.
    /// The substrate schema requires `query=...` for remember, `content=...`
    /// + `category=...` for save, `what_we_built=...` + `whats_next=...` for
    /// wrap_up. A previous version of this file used `remember('topic')`,
    /// `save()`, and `wrap_up()` — all three would 422 against the live
    /// schema. This test fails if anyone tries to revert.
    #[test]
    fn rules_content_uses_keyword_form_examples() {
        assert!(
            RULES_CONTENT.contains("remember(query="),
            "remember example must use keyword form (query=...), not positional"
        );
        assert!(
            RULES_CONTENT.contains("save(content="),
            "save example must show required `content` param"
        );
        assert!(
            RULES_CONTENT.contains("wrap_up(what_we_built="),
            "wrap_up example must show required `what_we_built` param"
        );
        assert!(
            RULES_CONTENT.contains("whats_next="),
            "wrap_up example must show required `whats_next` param"
        );
    }

    /// Guard against marketing language drift. The substrate is mechanical;
    /// the rules file is system-prompt instruction. Marketing modifiers
    /// (\"dramatically\", \"powerful\", \"intelligent\") tell the model this
    /// is ad copy, not instruction, and get de-weighted accordingly.
    #[test]
    fn rules_content_has_no_marketing_language() {
        let lower = RULES_CONTENT.to_lowercase();
        for banned in &["dramatically", "powerful", "intelligent", "consciousness", "magical"] {
            assert!(
                !lower.contains(banned),
                "RULES_CONTENT contains marketing language '{}'. Replace with an imperative trigger.",
                banned
            );
        }
    }

    /// Guard against aspirational hedge language that AI clients de-weight.
    /// "There's no reason to skip it" reads as a defensive note, not an
    /// instruction; doctrine says use imperative form ("Call when…").
    #[test]
    fn rules_content_has_no_aspirational_hedge() {
        let lower = RULES_CONTENT.to_lowercase();
        for banned in &[
            "there's no reason to skip",
            "feel free to",
            "you can call",
            "you may want to",
            "consider calling",
        ] {
            assert!(
                !lower.contains(banned),
                "RULES_CONTENT contains aspirational hedge '{}'. Rewrite as imperative.",
                banned
            );
        }
    }
}
