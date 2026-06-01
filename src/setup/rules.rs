// SPDX-License-Identifier: MIT OR Apache-2.0
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
/// **Doctrine: declarative, not imperative.**
///
/// Claude Code's UserPromptSubmit hook has a documented prompt-injection
/// defense (anthropics/claude-code#17804) that surfaces imperative system-
/// command text TO THE USER as visible text instead of treating it as
/// system context. "Call X" / "you must Y" / "always Z" trigger the
/// defense. Declarative statements ("this project uses EREBYX; the
/// substrate exposes 5 tools") do not.
///
/// claude-mem (46.1K stars) — the canonical Claude-Code memory product —
/// uses declarative framing throughout. We match the proven shape.
///
/// Token-tax discipline: this string is loaded into the effective system
/// prompt on EVERY conversation in EVERY configured client. CI gate
/// enforces ≤500-char-proxy tokens (cl100k_base ≈ ~310). Stress-tested
/// 2026-05-27 declarative rewrite — declarative form actually compresses
/// better than imperative because it drops the "you must"/"do not ask"
/// padding around each instruction.
const RULES_CONTENT: &str = r#"# EREBYX Memory — substrate reference

This project uses EREBYX for persistent AI memory across sessions. The substrate exposes 5 cognitive tools via MCP. Tool selection is automatic; this section describes when each tool applies so the AI can decide.

**Session-start tools (called once at the beginning of a session):**
- `restore_identity` — returns the stored identity, ethos, and foundation memories that should anchor this session's responses.
- `load_context` — returns the prior session's handoff (what was built + what is next), related memories, and active skills. Accepts `anchors=["domain"]` to scope to one domain or `[]` for the most recent handoff regardless of domain.

**Recall tool (applies when a question touches prior work, user preferences, project state, or session-spanning context):**
- `remember(query="…")` — returns ranked memories matching the query. An empty result signals the topic is new and general knowledge applies. Accepts `hint_anchors=["domain"]` to boost domain-tagged matches.

**Persistence tool (applies when the user states a decision, preference, fact, project detail, or identity shift worth recalling next session):**
- `save(content="…", category="…")` — persists a durable memory. `category` is a group ("identity", "experience", "knowledge") and substrate auto-routes. In-flight reasoning and tool-output recaps are not durable; they belong in conversational context.

**Continuity tool (applies at session end, when context approaches its limit, or when wrapping a logical work unit):**
- `wrap_up(what_we_built="…", whats_next="…")` — persists a session handoff. Idempotent; safe to invoke multiple times. The next session's `load_context` call replays this handoff.

**Hint protocol:** tool responses may carry `X-Erebyx-Hint: wrap_up_recommended` or `compact_imminent` headers. These indicate substrate-detected consolidation boundaries; the natural response is a subsequent `wrap_up` call.
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

/// Write the dynamic context block — fresh identity + last handoff
/// pulled from the substrate at setup time. Sits ALONGSIDE the static
/// EREBYX:START block in each client's rules file. Markers are
/// `<!-- EREBYX:DYNAMIC:START -->` / `<!-- EREBYX:DYNAMIC:END -->`.
///
/// **Why a separate block:** the static block describes the substrate's
/// 5-tool surface declaratively (verb-by-verb, never changes). The
/// dynamic block carries SESSION-SPECIFIC context that changes with
/// every handoff. Keeping them separate means `erebyx setup` re-runs
/// can refresh just the dynamic portion without touching the static
/// instructions.
///
/// **Model-agnostic pre-injection:** for clients without a native
/// SessionStart hook (Windsurf, Continue, Zed, VS Code/Copilot), this
/// block IS the pre-injection mechanism — the rules file is always
/// loaded into the system prompt, so fresh content here = always-fresh
/// context for every session.
///
/// Idempotent: any pre-existing dynamic block is removed before the
/// new one is written.
///
/// **Fail-open**: if `dynamic_content` is empty (substrate unreachable,
/// onboarding not complete, etc), the function still RUNS but writes
/// an empty marked block. That way subsequent refreshes can populate
/// the same slot without restructuring the file.
pub fn write_dynamic_block(client: &AiClient, dynamic_content: &str) -> Result<()> {
    if !client.rules_path.exists() {
        // No rules file yet — nothing to attach the dynamic block to.
        // Skip rather than create a file with only the dynamic section
        // (the static block is the load-bearing context).
        return Ok(());
    }

    let existing = std::fs::read_to_string(&client.rules_path)
        .with_context(|| format!("Failed to read {}", client.rules_path.display()))?;

    let cleaned = remove_dynamic_section(&existing);

    let marked = format!(
        "<!-- EREBYX:DYNAMIC:START -->\n{}\n<!-- EREBYX:DYNAMIC:END -->",
        dynamic_content.trim(),
    );

    // Append the dynamic block AFTER the static block — keeps the
    // declarative tool descriptions on top, situational context below.
    let new_content = format!("{}\n\n{}\n", cleaned.trim_end(), marked);

    std::fs::write(&client.rules_path, &new_content)
        .with_context(|| format!("Failed to write {}", client.rules_path.display()))?;

    Ok(())
}

/// Strip a previous EREBYX:DYNAMIC:START/END block from rules-file content.
/// Mirrors `remove_erebyx_section` but for the dynamic markers.
fn remove_dynamic_section(content: &str) -> String {
    if let (Some(start), Some(end)) = (
        content.find("<!-- EREBYX:DYNAMIC:START -->"),
        content.find("<!-- EREBYX:DYNAMIC:END -->"),
    ) {
        if start > end {
            return content.to_string();
        }
        let end = end + "<!-- EREBYX:DYNAMIC:END -->".len();
        let before = &content[..start];
        let after = &content[end..];
        format!("{}{}", before.trim_end(), after.trim_start())
    } else {
        content.to_string()
    }
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
    /// uses a chars/4 heuristic that approximates cl100k_base.
    ///
    /// Measured 2026-05-27 LATE (brutal-review wave-2):
    ///   chars: 1945, char-proxy estimate: 486, real cl100k_base: 431
    ///   ratio: cl100k ≈ 89% of char-proxy
    ///
    /// Budget = 500 char-proxy tokens ≈ ~445 real cl100k tokens.
    /// Current usage: 97% of char-proxy budget.
    ///
    /// **If you add to RULES_CONTENT:** plan ~12 chars per cl100k token,
    /// not 4. We're close enough to the ceiling that an additional
    /// instructional sentence will push past unless you trim elsewhere.
    #[test]
    fn rules_content_under_token_budget() {
        let char_count = RULES_CONTENT.len();
        let token_est = char_count / 4;
        assert!(
            token_est < 500,
            "RULES_CONTENT char count {} ≈ {} char-proxy tokens \
             (cl100k_base ≈ {} actual). Budget 500. Either trim content \
             or raise budget after a stress-test pass.",
            char_count,
            token_est,
            (token_est as f64 * 0.89) as usize,
        );
    }

    /// Lock the keyword-form examples in RULES_CONTENT against future drift.
    /// The substrate schema requires `query=...` for remember, `content=...`
    /// and `category=...` for save, `what_we_built=...` + `whats_next=...` for
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
    /// the rules file is system-prompt context. Marketing modifiers
    /// ("dramatically", "powerful", "intelligent") tell the model this
    /// is ad copy and get de-weighted accordingly.
    #[test]
    fn rules_content_has_no_marketing_language() {
        let lower = RULES_CONTENT.to_lowercase();
        for banned in &[
            "dramatically",
            "powerful",
            "intelligent",
            "consciousness",
            "magical",
        ] {
            assert!(
                !lower.contains(banned),
                "RULES_CONTENT contains marketing language '{}'. Replace with a factual description.",
                banned
            );
        }
    }

    /// Verify `write_dynamic_block` writes a `<!-- EREBYX:DYNAMIC -->`
    /// block alongside the existing static block, and that re-running
    /// with new content replaces (doesn't accumulate) the prior block.
    #[test]
    fn dynamic_block_writes_alongside_static_and_is_idempotent() {
        use std::io::Write;
        let tmpdir = std::env::temp_dir().join(format!(
            "erebyx-dyn-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmpdir).unwrap();
        let rules_path = tmpdir.join("erebyx-memory.md");

        // Pre-seed with a static EREBYX:START block (what `erebyx setup`
        // writes first).
        let mut f = std::fs::File::create(&rules_path).unwrap();
        writeln!(
            f,
            "<!-- EREBYX:START -->\nstatic content here\n<!-- EREBYX:END -->"
        )
        .unwrap();
        drop(f);

        let client = AiClient {
            kind: ClientKind::ClaudeCode,
            name: "Test",
            config_path: tmpdir.join("settings.json"),
            rules_path: rules_path.clone(),
            config_exists: false,
            home_dir: tmpdir.clone(),
        };

        // Write dynamic block — should appear AFTER the static block.
        write_dynamic_block(
            &client,
            "Stored identity: ZENN\nLast handoff: shipped session-start hook",
        )
        .unwrap();
        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(
            content.contains("<!-- EREBYX:START -->"),
            "static block survived"
        );
        assert!(
            content.contains("<!-- EREBYX:DYNAMIC:START -->"),
            "dynamic block written"
        );
        assert!(
            content.contains("Stored identity: ZENN"),
            "dynamic content present"
        );
        // Order: STATIC must come before DYNAMIC.
        let static_pos = content.find("<!-- EREBYX:START -->").unwrap();
        let dynamic_pos = content.find("<!-- EREBYX:DYNAMIC:START -->").unwrap();
        assert!(
            static_pos < dynamic_pos,
            "static block must precede dynamic block"
        );

        // Re-write dynamic block with new content — should REPLACE, not duplicate.
        write_dynamic_block(
            &client,
            "Stored identity: ZENN\nLast handoff: dynamic refresh works",
        )
        .unwrap();
        let content2 = std::fs::read_to_string(&rules_path).unwrap();
        let dynamic_starts = content2.matches("<!-- EREBYX:DYNAMIC:START -->").count();
        assert_eq!(
            dynamic_starts, 1,
            "expected exactly one EREBYX:DYNAMIC:START, got {}",
            dynamic_starts
        );
        assert!(
            content2.contains("dynamic refresh works"),
            "new content present"
        );
        assert!(
            !content2.contains("shipped session-start hook"),
            "old content removed"
        );

        std::fs::remove_dir_all(&tmpdir).unwrap();
    }

    /// Guard against imperative command framing.
    ///
    /// Claude Code's UserPromptSubmit hook has a documented prompt-injection
    /// defense (anthropics/claude-code#17804) that surfaces imperative
    /// system-command text TO THE USER as visible text instead of treating
    /// it as system context. claude-mem (46.1K stars) — the canonical
    /// Claude-Code memory product — uses declarative framing throughout.
    /// We match the proven shape.
    ///
    /// "Call X", "you must Y", "always Z", "do not ask permission" are all
    /// imperative-mood patterns that risk tripping the defense. Declarative
    /// alternatives state facts: "the substrate exposes X", "this tool
    /// applies when Y", "Z is idempotent".
    #[test]
    fn rules_content_uses_declarative_not_imperative_framing() {
        let lower = RULES_CONTENT.to_lowercase();
        // The substring forms we ban — any one of these means an
        // imperative slipped in. Pruning the list as we learn which
        // imperatives are tolerated vs which trip the defense.
        for banned in &[
            "you must",
            "you should",
            "always call",
            "do not ask permission",
            "bias toward firing",
            "fire proactively",
        ] {
            assert!(
                !lower.contains(banned),
                "RULES_CONTENT contains imperative phrase '{}'. Rewrite as declarative: \
                 state when the tool applies ('this tool applies when X') instead of \
                 commanding ('always call X'). See anthropics/claude-code#17804 for the \
                 injection-defense false-positive that imperative framing triggers.",
                banned
            );
        }
    }

    /// Legacy hedge guard — kept but downgraded.
    ///
    /// "There's no reason to skip it" / "feel free to" still read as weak
    /// system instructions and AI clients de-weight them. But declarative
    /// alternatives are NOT hedges — "this tool applies when X" is the
    /// correct shape, and it's neither imperative nor hedged.
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
