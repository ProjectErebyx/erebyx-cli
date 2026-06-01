// SPDX-License-Identifier: MIT OR Apache-2.0
//! Claude Code hooks — automatic memory injection before every response.
//!
//! Installs a UserPromptSubmit hook that calls `erebyx hook-inject` natively.
//! Smart-gating, JSON parsing, the REST call, and additionalContext formatting
//! all happen inside the Rust binary — no python3 dependency, no shell pipes,
//! single process spawn per message.
//!
//! The hook is fail-open: any error path emits `{}` so Claude Code never blocks.
//! Timeout: 500ms hard limit (set inside `erebyx hook-inject`).

use anyhow::{bail, Context, Result};

use super::detect::AiClient;
use crate::client::is_safe_url;

/// Wrap a string as a POSIX single-quoted shell literal that can NEVER break
/// out of its quoting — closing every embedded `'` with the canonical
/// `'\''` sequence. Inside single quotes the shell treats `$`, backtick,
/// `}`, `"` and every other metacharacter literally, so command
/// substitution / parameter-expansion breakout is structurally impossible.
///
/// This is the second half of the hooks.rs hardening (the first being
/// `is_safe_url` validation up the call chain): even if a malformed
/// `api_url` ever reached here, it lands as inert data, not shell.
fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            // Close quote, emit an escaped quote, reopen quote.
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// The hook script that runs on every user message in Claude Code.
///
/// Trivial wrapper: hands stdin off to `erebyx hook-inject`. All logic
/// (gating, query, REST call, formatting) lives in the native binary.
/// The API key is read from $EREBYX_API_KEY by the binary itself.
///
/// Security (P0-2): `api_url` is validated by `is_safe_url` (HTTPS or
/// localhost) BEFORE this script is generated — see `install_hooks`. As
/// defense-in-depth, the validated value is emitted as a POSIX
/// single-quoted shell literal via `shell_single_quote`, so even a value
/// containing `$(...)`, backticks, `}` or `"` cannot escape its quoting
/// and execute. The prior `${{EREBYX_API_URL:-{api_url}}}` default-expansion
/// interpolated `api_url` unquoted into the default word, where a value like
/// `}$(...)` would close the parameter expansion and run a command
/// substitution at hook-fire time.
fn hook_script(api_url: &str) -> String {
    // Single-quoted literal — already includes the surrounding quotes.
    let quoted_default = shell_single_quote(api_url);
    format!(
        r#"#!/usr/bin/env bash
# EREBYX Memory Injector — Claude Code UserPromptSubmit Hook
# Hands stdin to the native `erebyx hook-inject` handler.
# Fail-open: any error path emits {{}}.

set -u

# Fail open if API key is not set
if [ -z "${{EREBYX_API_KEY:-}}" ]; then
    printf '%s' '{{}}'
    exit 0
fi

# Pass api_url through env so hook-inject doesn't need flag parsing.
# Honor an inherited EREBYX_API_URL; otherwise fall back to the validated
# install-time value, emitted as a single-quoted shell literal so it can
# never break out into command substitution.
export EREBYX_API_URL="${{EREBYX_API_URL:-}}"
if [ -z "${{EREBYX_API_URL}}" ]; then
    export EREBYX_API_URL={quoted_default}
fi

# Single native call. If it fails for any reason, emit {{}} and exit clean.
exec erebyx hook-inject 2>/dev/null || printf '%s' '{{}}'
"#,
        quoted_default = quoted_default,
    )
}

/// Install Claude Code hooks for automatic memory injection.
pub fn install_hooks(client: &AiClient, _api_key: &str, api_url: &str) -> Result<()> {
    // P0-2 / P1-4: refuse to bake an unsafe URL into the generated hook
    // script. `is_safe_url` accepts HTTPS or localhost only — anything else
    // (plain http:// to the internet, or an injection payload containing
    // `$(...)` / backticks / `}` / `"`) is rejected here, before the script
    // is written. `hook_script` also single-quotes the value as
    // defense-in-depth, but failing loud at install time is the correct
    // contract: setup is interactive, so a bad URL must surface, not
    // silently land a neutered script.
    if !is_safe_url(api_url) {
        bail!(
            "EREBYX_API_URL must be https:// (got {}). \
             Plain http:// is only allowed for localhost/127.0.0.1.",
            api_url
        );
    }
    // P1-4 (2026-05-27): the hook script uses bash + Unix path conventions.
    // Claude Code on Windows doesn't invoke `.sh` files directly (it wants
    // `.bat` / `.cmd` / `.ps1`). Pre-fix `erebyx setup` reported success on
    // Windows while writing a script that never ran — customers thought
    // hooks were installed and never received memory injection, with no
    // error signal. v0.1.2 will ship the PowerShell variant; v0.1.1 punts
    // cleanly with a clear message so the customer knows what landed and
    // what didn't.
    // Brutal-review wave-2 (2026-05-27 CLI lane): SessionStart hook is
    // pure settings.json JSON mutation — it works fine on Windows. Only
    // the UserPromptSubmit bash script (`.sh`) is Unix-only. Splitting
    // the two registrations so SessionStart pre-injection still works
    // on Windows is a 6-line change vs leaving Windows users without
    // claude-mem-equivalent pre-injection.
    #[cfg(target_os = "windows")]
    {
        let _ = api_url; // unused on this branch
        eprintln!(
            "  ⚠ Claude Code UserPromptSubmit hook: Windows support is not \
             yet implemented (bash script). SessionStart pre-injection is \
             still registered — that's the load-bearing claude-mem-equivalent \
             mechanic. Per-prompt memory injection arrives in v0.1.2 via a \
             PowerShell variant."
        );
        register_session_start_hook(client)?;
        return Ok(());
    }

    // Native hook handler — no python3 or external interpreter required.

    // Write the hook script
    let hooks_dir = client.home_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("Failed to create hooks directory: {}", hooks_dir.display()))?;

    let script_path = hooks_dir.join("erebyx-memory-injector.sh");
    let script_content = hook_script(api_url);
    std::fs::write(&script_path, &script_content)
        .with_context(|| format!("Failed to write hook script: {}", script_path.display()))?;

    // Set owner-only execute permissions (0o700) on the hook script
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&script_path, perms)
            .with_context(|| format!("Failed to set permissions on {}", script_path.display()))?;
    }

    // Register hooks in Claude Code settings
    register_hook_in_settings(client, &script_path)?;
    register_session_start_hook(client)?;

    Ok(())
}

/// Native SessionStart hook for Claude Code — pre-injects identity +
/// last handoff into the system prompt BEFORE the first user prompt
/// fires.
///
/// This is the claude-mem mechanic (46.1K stars) applied as the
/// gold-standard Claude-Code memory wiring. SessionStart's stdout is
/// injected into Claude Code's context (stdin/stdout contract, exit 0),
/// so this hook calls `erebyx hook-session-start` which fetches the
/// substrate identity + handoff in <800ms and emits the result as
/// additionalContext JSON.
///
/// Fail-open is enforced inside `erebyx hook-session-start` (any error
/// path emits `{}`), so a substrate hiccup never blocks session boot.
fn register_session_start_hook(client: &AiClient) -> Result<()> {
    let settings_path = &client.config_path;

    let mut config = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)
            .with_context(|| format!("Failed to read {}", settings_path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse JSON in {}", settings_path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let config_path_display = settings_path.display().to_string();
    require_object(&config, &config_path_display, "root")?;

    let hooks = config
        .as_object_mut()
        .expect("validated above")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    require_object(hooks, &config_path_display, "hooks")?;

    let session_start = hooks
        .as_object_mut()
        .expect("validated above")
        .entry("SessionStart")
        .or_insert_with(|| serde_json::json!([]));

    // Hook command invokes `erebyx hook-session-start`. Marked with
    // `_erebyx_managed: true` so the same retention logic that protects
    // user-authored UserPromptSubmit hooks also protects user-authored
    // SessionStart hooks.
    let hook_entry = serde_json::json!({
        "type": "command",
        "command": "erebyx hook-session-start",
        "_erebyx_managed": true
    });

    if let Some(arr) = session_start.as_array_mut() {
        arr.retain(|h| !should_remove_session_start_entry(h));
        arr.push(hook_entry);
    }

    let content =
        serde_json::to_string_pretty(&config).context("Failed to serialize settings JSON")?;
    std::fs::write(settings_path, content)
        .with_context(|| format!("Failed to write {}", settings_path.display()))?;

    Ok(())
}

/// Pure predicate: should this existing SessionStart hook entry be
/// REMOVED before registering our managed entry? Mirrors
/// `should_remove_hook_entry` but matches the SessionStart-specific
/// command shape (`erebyx hook-session-start`).
fn should_remove_session_start_entry(entry: &serde_json::Value) -> bool {
    let is_managed = entry
        .get("_erebyx_managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_managed {
        return true;
    }
    let cmd = entry.get("command").and_then(|c| c.as_str()).unwrap_or("");
    cmd == "erebyx hook-session-start"
}

/// Validate a JSON value is an object, returning a descriptive error if not.
fn require_object(value: &serde_json::Value, config_path: &str, key_context: &str) -> Result<()> {
    if !value.is_object() {
        bail!(
            "Expected JSON object for '{}' in {}, but found {}. \
             Fix the config file or delete it so erebyx can recreate it.",
            key_context,
            config_path,
            value_type_name(value),
        );
    }
    Ok(())
}

/// Register the hook in Claude Code's settings.json.
fn register_hook_in_settings(client: &AiClient, script_path: &std::path::Path) -> Result<()> {
    let settings_path = &client.config_path; // ~/.claude/settings.json

    let mut config = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)
            .with_context(|| format!("Failed to read {}", settings_path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse JSON in {}", settings_path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let config_path_display = settings_path.display().to_string();

    // Validate root is an object, then access it
    require_object(&config, &config_path_display, "root")?;
    let hooks = config
        .as_object_mut()
        .expect("validated above")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    // Validate hooks is an object, then access it
    require_object(hooks, &config_path_display, "hooks")?;
    let user_prompt_hooks = hooks
        .as_object_mut()
        .expect("validated above")
        .entry("UserPromptSubmit")
        .or_insert_with(|| serde_json::json!([]));

    // Build and insert the hook entry with an explicit managed-marker so
    // future re-runs only touch our own entries — not user-authored
    // adjacent automation. P1-5 (2026-05-27): the prior substring filter
    // (`c.contains("erebyx")`) would wipe ANY hook whose command mentioned
    // erebyx (e.g. `~/bin/erebyx-archive-export`, custom helpers under
    // `~/scripts/my-erebyx-extras.sh`). Customers writing their own
    // erebyx-adjacent automation would lose it silently on the next
    // `erebyx setup` run.
    let hook_entry = serde_json::json!({
        "type": "command",
        "command": script_path.to_string_lossy(),
        "_erebyx_managed": true
    });

    if let Some(arr) = user_prompt_hooks.as_array_mut() {
        // Retention precision via the extracted `should_remove_hook_entry`
        // predicate. See its docstring (and unit tests at the bottom of
        // this file) for the three-way match semantics.
        arr.retain(|h| !should_remove_hook_entry(h));
        arr.push(hook_entry);
    }

    let content =
        serde_json::to_string_pretty(&config).context("Failed to serialize settings JSON")?;
    std::fs::write(settings_path, content)
        .with_context(|| format!("Failed to write {}", settings_path.display()))?;

    Ok(())
}

/// Human-readable name for a JSON value type.
fn value_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Pure predicate: should this existing hook entry be REMOVED before
/// registering our managed entry?
///
/// Extracted from `register_hook_in_settings` so the precision contract
/// can be tested directly. P1-5 (brutal-review POSTFIX_CLI CC-1) said
/// "hook retention filter is a pure-function pass over a JSON array.
/// Testable. Add tests."
///
/// Three-way match:
///   1. Explicit `_erebyx_managed: true` marker (current canon).
///   2. Exact-path match for `*/erebyx-memory-injector.sh` (back-compat
///      with v0.1.0 installs that lack the flag).
///   3. Exact `erebyx hook-inject` command (legacy invocation).
///
/// Anything else stays untouched.
fn should_remove_hook_entry(entry: &serde_json::Value) -> bool {
    let is_managed = entry
        .get("_erebyx_managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_managed {
        return true;
    }
    let cmd = entry.get("command").and_then(|c| c.as_str()).unwrap_or("");
    cmd.ends_with("erebyx-memory-injector.sh") || cmd == "erebyx hook-inject"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -------------------------------------------------------------
    // should_remove_hook_entry — retention precision (P1-5)
    // -------------------------------------------------------------

    #[test]
    fn explicit_managed_marker_triggers_removal() {
        let entry = json!({
            "type": "command",
            "command": "/anywhere/whatever.sh",
            "_erebyx_managed": true,
        });
        assert!(should_remove_hook_entry(&entry));
    }

    #[test]
    fn legacy_path_suffix_triggers_removal_without_marker() {
        // Back-compat with v0.1.0 installs that lack the flag.
        let entry = json!({
            "type": "command",
            "command": "/Users/mikey/.claude/hooks/erebyx-memory-injector.sh",
        });
        assert!(should_remove_hook_entry(&entry));
    }

    #[test]
    fn legacy_exact_command_triggers_removal() {
        let entry = json!({
            "type": "command",
            "command": "erebyx hook-inject",
        });
        assert!(should_remove_hook_entry(&entry));
    }

    #[test]
    fn user_automation_mentioning_erebyx_is_preserved() {
        // Pre-fix the substring filter `c.contains("erebyx")` would
        // wipe this. Now we accept only exact-form matches.
        for cmd in &[
            "/Users/mikey/bin/erebyx-archive-export",
            "/Users/mikey/scripts/my-erebyx-extras.sh",
            "/usr/local/bin/erebyx-backup",
            "erebyx --version && other-tool", // wraps in shell
            "python /opt/erebyx-tools/sync.py",
        ] {
            let entry = json!({
                "type": "command",
                "command": cmd,
            });
            assert!(
                !should_remove_hook_entry(&entry),
                "user automation {cmd:?} must be preserved"
            );
        }
    }

    #[test]
    fn non_managed_unrelated_hooks_preserved() {
        let entry = json!({
            "type": "command",
            "command": "echo hello",
        });
        assert!(!should_remove_hook_entry(&entry));
    }

    #[test]
    fn entry_with_marker_false_is_not_managed() {
        // _erebyx_managed: false is treated as "not ours" — only
        // explicit true triggers removal.
        let entry = json!({
            "type": "command",
            "command": "/some/path.sh",
            "_erebyx_managed": false,
        });
        assert!(!should_remove_hook_entry(&entry));
    }

    #[test]
    fn entry_missing_command_field_doesnt_panic() {
        let entry = json!({"type": "command"});
        assert!(!should_remove_hook_entry(&entry));
    }

    #[test]
    fn entry_with_null_command_doesnt_panic() {
        let entry = json!({"type": "command", "command": null});
        assert!(!should_remove_hook_entry(&entry));
    }

    // -------------------------------------------------------------
    // P0-2: shell-injection hardening of the generated hook script
    // -------------------------------------------------------------

    /// `shell_single_quote` produces an inert single-quoted literal:
    /// command-substitution, backticks, `}`, and `"` survive verbatim and
    /// cannot break out of the quoting.
    #[test]
    fn shell_single_quote_neutralizes_metacharacters() {
        // No embedded single-quote → simple wrap.
        assert_eq!(
            shell_single_quote("https://core.erebyx.com"),
            "'https://core.erebyx.com'"
        );
        // Embedded single-quote → '\'' splice, no other char is special.
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        // Metacharacters that the OLD `${VAR:-...}` interpolation let escape
        // are now inert data inside single quotes.
        let payload = r#"}$(touch /tmp/pwn)`id`"#;
        let quoted = shell_single_quote(payload);
        assert!(quoted.starts_with('\''));
        assert!(quoted.ends_with('\''));
        // The whole payload is preserved as ONE single-quoted run (no
        // `'\''` splice because the payload contains no single quote), so
        // nothing inside it is shell-active.
        assert_eq!(quoted, format!("'{payload}'"));
    }

    /// REGRESSION (P0-2): the generated hook script must NOT contain a
    /// command-substitution / backtick / `}`-breakout sequence OUTSIDE of a
    /// single-quoted literal. Against the ORIGINAL `hooks.rs` this FAILS:
    /// the url was interpolated into `${EREBYX_API_URL:-<payload>}` where a
    /// payload of `}$(...)` closed the parameter expansion and produced a
    /// live `$(...)` at hook-fire time.
    ///
    /// Note: `install_hooks` now also REJECTS such a url via `is_safe_url`,
    /// but we test `hook_script` directly to pin the defense-in-depth
    /// quoting contract independent of the validation gate.
    #[test]
    fn hook_script_neutralizes_command_substitution_payload() {
        let payload = r#"http://evil}$(touch /tmp/erebyx_pwn)"#;
        let script = hook_script(payload);
        // The dangerous payload must appear ONLY inside its single-quoted
        // literal. Concretely: the `$(` must be immediately preceded by
        // characters that keep it inside the single-quote run — i.e. there
        // must be NO `${EREBYX_API_URL:-...$(` default-expansion form.
        assert!(
            !script.contains("${EREBYX_API_URL:-http://evil}"),
            "url was interpolated into an unquoted parameter-expansion default \
             — `}}` breaks out and `$(...)` executes. Script:\n{script}"
        );
        // The literal payload, single-quoted, is what should be present.
        assert!(
            script.contains(&format!(
                "export EREBYX_API_URL={}",
                shell_single_quote(payload)
            )),
            "expected the api_url emitted as a single-quoted shell literal. Script:\n{script}"
        );
    }

    /// REGRESSION (P0-2): a payload containing a literal backtick and `}`
    /// is fully contained in the single-quoted literal — no backtick
    /// command substitution is left active in the script.
    #[test]
    fn hook_script_neutralizes_backtick_payload() {
        let payload = "https://x`id`y";
        let script = hook_script(payload);
        // The only occurrence of the backtick payload is inside the
        // single-quoted export line.
        let expected_line = format!("export EREBYX_API_URL={}", shell_single_quote(payload));
        assert!(script.contains(&expected_line), "Script:\n{script}");
        // And the OLD vulnerable form is gone.
        assert!(
            !script.contains("${EREBYX_API_URL:-https://x`id`y}"),
            "backtick payload interpolated into bare default expansion. Script:\n{script}"
        );
    }

    /// REGRESSION (P1-4): `install_hooks`' `is_safe_url` gate rejects
    /// wrong-SCHEME URLs (plain http:// to the internet, ftp://, etc.)
    /// BEFORE writing any script. Against the original code there was NO
    /// gate in `install_hooks`, so a plain-http URL would be baked into the
    /// hook script and the bearer later POSTed over it.
    #[test]
    fn install_hooks_guard_rejects_wrong_scheme() {
        for bad in &[
            "http://evil.example.com", // plain http to internet
            "ftp://evil.example.com",  // wrong scheme entirely
            "file:///etc/passwd",      // not http(s)
        ] {
            assert!(
                !is_safe_url(bad),
                "install_hooks guard must reject wrong-scheme api_url {bad:?}"
            );
        }
        for ok in &[
            "https://core.erebyx.com",
            "http://localhost:8080",
            "http://127.0.0.1:9000",
        ] {
            assert!(is_safe_url(ok), "valid api_url {ok:?} must pass");
        }
    }

    /// REGRESSION (P0-2) — the load-bearing one: `is_safe_url` is a SCHEME
    /// guard, NOT a shell-metacharacter guard, so a `https://`-prefixed
    /// injection payload PASSES validation. This documents WHY validation
    /// alone is insufficient and the single-quoting in `hook_script` is the
    /// actual breakout defense. The generated script must neutralize the
    /// payload even though `is_safe_url` accepts it.
    #[test]
    fn https_prefixed_injection_passes_scheme_guard_but_script_neutralizes_it() {
        let payload = r#"https://evil}$(touch /tmp/erebyx_pwn)`id`"#;
        // Validation alone does NOT catch this (scheme is https://):
        assert!(
            is_safe_url(payload),
            "is_safe_url is scheme-only; this https:// payload is expected to pass \
             — which is exactly why defense-in-depth single-quoting is required"
        );
        // But the generated script renders it inert (single-quoted literal),
        // NOT as a live `${EREBYX_API_URL:-...}` default-expansion. Against
        // the ORIGINAL hooks.rs this assertion FAILS — the payload landed in
        // `${EREBYX_API_URL:-https://evil}$(...)` and `}` + `$(...)` executed.
        let script = hook_script(payload);
        assert!(
            script.contains(&format!(
                "export EREBYX_API_URL={}",
                shell_single_quote(payload)
            )),
            "payload must be emitted as a single-quoted shell literal. Script:\n{script}"
        );
        assert!(
            !script.contains("${EREBYX_API_URL:-https://evil}"),
            "payload must NOT appear in an unquoted parameter-expansion default. Script:\n{script}"
        );
    }
}
