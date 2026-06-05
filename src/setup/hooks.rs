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

use super::config::secure_write::{atomic_write_secret, ensure_dir_secure, erebyx_command};
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
    // Absolute, symlink-resolved path of the running binary, single-quoted so
    // a path containing spaces or shell metacharacters lands as inert data.
    // GUI-launched Claude Code often runs with a minimal $PATH that doesn't
    // include the install dir, so a bare `erebyx` would silently fail to
    // exec — the absolute path is what makes the hook fire from a desktop
    // client.
    let quoted_exe = shell_single_quote(&erebyx_command());
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

# Single native call via the absolute binary path (single-quoted literal).
# If it fails for any reason, emit {{}} and exit clean.
exec {quoted_exe} hook-inject 2>/dev/null || printf '%s' '{{}}'
"#,
        quoted_default = quoted_default,
        quoted_exe = quoted_exe,
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
    // The UserPromptSubmit bash-hook path is Unix-only; Windows punts cleanly
    // after registering the (cross-platform) SessionStart hook. We branch with
    // a RUNTIME `cfg!` (not `#[cfg]`): #[cfg] either left the Unix path
    // compiled-but-unreachable on Windows (unreachable_code) OR — if we cfg'd
    // the Unix path out — orphaned its helpers (hook_script, ensure_dir_secure,
    // register_hook_in_settings, …) into dead_code on Windows. A runtime branch
    // keeps the whole Unix path COMPILED on every target (helpers stay used) and
    // can't trip unreachable_code, while still skipping the .sh write on Windows
    // at run time. The Unix path is pure-cross-platform Rust except the inner
    // #[cfg(unix)] perms block, so it builds fine on Windows even though it
    // never executes there.
    if cfg!(target_os = "windows") {
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

    // Native (Unix) hook handler — no python3 or external interpreter required.
    // `ensure_dir_secure` creates the hooks dir and, when WE create it, tightens
    // it to 0o700 on Unix (stat-first so a pre-existing dir keeps user perms).
    let hooks_dir = client.home_dir.join("hooks");
    ensure_dir_secure(&hooks_dir)?;

    let script_path = hooks_dir.join("erebyx-memory-injector.sh");
    let script_content = hook_script(api_url);
    std::fs::write(&script_path, &script_content)
        .with_context(|| format!("Failed to write hook script: {}", script_path.display()))?;

    // Owner-only read/write/execute (0o700) on the hook script — not a
    // credential file, but executed on every prompt, so owner-only is right.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&script_path, perms)
            .with_context(|| format!("Failed to set permissions on {}", script_path.display()))?;
    }

    // Register hooks in Claude Code settings.
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

    // Claude Code REQUIRES a matcher-GROUP wrapper for SessionStart entries
    // and iterates `SessionStart[].hooks[]`. A flat `{type,command}` pushed
    // directly into the array (what v0.1.0/v0.1.1 did) is NEVER executed —
    // the hook silently never fired. The correct shape is:
    //   [{ "hooks": [ {type, command} ], "_erebyx_managed": true }]
    //
    // We OMIT the `matcher` field deliberately. For SessionStart, the matcher
    // selects the SOURCE (`startup` | `resume` | `clear` | `compact`); pinning
    // `"startup"` would fire ONLY on a cold CLI start and NEVER on resume,
    // /clear, or post-compaction — which are precisely the moments continuity
    // matters most (re-opening a conversation, re-hydrating after a compact).
    // No matcher = fires on EVERY session start, which is the whole point of
    // restore-identity-on-wake. The `_erebyx_managed` marker lives on the GROUP
    // (the level CC iterates), not the inner handler, so retention can find it.
    //
    // The command uses the absolute, symlink-resolved binary path (not bare
    // `erebyx`) so GUI-launched Claude Code with a minimal $PATH still fires.
    let hook_command = format!("{} hook-session-start", erebyx_command());
    let hook_group = serde_json::json!({
        "hooks": [
            {
                "type": "command",
                "command": hook_command
            }
        ],
        "_erebyx_managed": true
    });

    if let Some(arr) = session_start.as_array_mut() {
        // Remove our managed groups AND legacy flat entries (upgrade path:
        // cleans up the broken v0.1.0/v0.1.1 flat objects), then push the one
        // correct nested group.
        arr.retain(|h| !should_remove_session_start_entry(h));
        arr.push(hook_group);
    }

    let content =
        serde_json::to_string_pretty(&config).context("Failed to serialize settings JSON")?;
    // Atomic write: settings.json holds the user's ENTIRE Claude Code config
    // (other MCP servers, hooks, permissions). A crash mid-write would
    // corrupt all of it — `atomic_write_secret` makes the replace atomic.
    atomic_write_secret(settings_path, content.as_bytes())?;

    Ok(())
}

/// Pure predicate: should this existing SessionStart array element be
/// REMOVED before registering our managed matcher-group?
///
/// Post-[1], our entry is a matcher-GROUP `{matcher, hooks:[…],
/// _erebyx_managed:true}` and the marker lives on the group. This predicate
/// must therefore match three forms:
///
///   1. **Group with our marker** — `_erebyx_managed: true` at the top level
///      of the array element (current canon).
///   2. **Group whose `hooks[]` contains our command** — any handler whose
///      `command` ends with `erebyx hook-session-start` (back-compat for a
///      marker-less group, and the descend [2] requires).
///   3. **Legacy FLAT entry** — a top-level `{type, command}` where `command`
///      ends with `erebyx hook-session-start`. This is the v0.1.0/v0.1.1
///      broken shape; matching it here is the upgrade path that replaces the
///      dead flat entry with the correct nested group.
///
/// Anything else (user-authored groups/hooks) stays untouched.
fn should_remove_session_start_entry(entry: &serde_json::Value) -> bool {
    // 1. Marker on the group (or a legacy flat entry that carried it).
    if entry
        .get("_erebyx_managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    // 2. Group: descend into hooks[] and match our command.
    if let Some(handlers) = entry.get("hooks").and_then(|h| h.as_array()) {
        if handlers.iter().any(is_session_start_command) {
            return true;
        }
    }
    // 3. Legacy flat entry: command at the top level.
    is_session_start_command(entry)
}

/// True if a single hook-handler object's `command` is the erebyx
/// session-start invocation. Tolerant of the absolute-path form
/// (`/abs/path/erebyx hook-session-start`) as well as the legacy bare form
/// (`erebyx hook-session-start`).
fn is_session_start_command(handler: &serde_json::Value) -> bool {
    let cmd = handler
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    cmd == "erebyx hook-session-start" || cmd.ends_with(" hook-session-start")
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

    // Claude Code REQUIRES a matcher-GROUP wrapper for UserPromptSubmit
    // entries and iterates `UserPromptSubmit[].hooks[]`. A flat
    // `{type,command}` pushed directly into the array (what v0.1.0/v0.1.1
    // did) is NEVER executed — the feature silently never fired. The correct
    // shape is:
    //   [{ "hooks": [ {type, command} ], "_erebyx_managed": true }]
    // (UserPromptSubmit takes no `matcher` field — it fires on every prompt.)
    // The `_erebyx_managed` marker lives on the GROUP (the level CC
    // iterates), so retention can find it.
    //
    // P1-5 (2026-05-27): retention is exact-form, never a substring match on
    // "erebyx", so user-authored erebyx-adjacent automation
    // (`~/bin/erebyx-archive-export`, etc.) is preserved.
    let hook_group = serde_json::json!({
        "hooks": [
            {
                "type": "command",
                "command": script_path.to_string_lossy()
            }
        ],
        "_erebyx_managed": true
    });

    if let Some(arr) = user_prompt_hooks.as_array_mut() {
        // Remove our managed groups AND legacy flat entries (upgrade path:
        // cleans up the broken v0.1.0/v0.1.1 flat objects) via the extracted
        // `should_remove_hook_entry` predicate, then push the one correct
        // nested group.
        arr.retain(|h| !should_remove_hook_entry(h));
        arr.push(hook_group);
    }

    let content =
        serde_json::to_string_pretty(&config).context("Failed to serialize settings JSON")?;
    // Atomic write — see `register_session_start_hook`.
    atomic_write_secret(settings_path, content.as_bytes())?;

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

/// Pure predicate: should this existing UserPromptSubmit array element be
/// REMOVED before registering our managed matcher-group?
///
/// Extracted from `register_hook_in_settings` so the precision contract can
/// be tested directly. P1-5 (brutal-review POSTFIX_CLI CC-1) said "hook
/// retention filter is a pure-function pass over a JSON array. Testable. Add
/// tests."
///
/// Post-[1], our entry is a matcher-GROUP `{hooks:[…], _erebyx_managed:true}`
/// and the marker lives on the group. This predicate matches:
///
///   1. **Group with our marker** — `_erebyx_managed: true` at the top level
///      of the array element (current canon).
///   2. **Group whose `hooks[]` contains our command** — any handler whose
///      `command` is our injector script or the legacy `erebyx hook-inject`
///      (the descend [2] requires; back-compat for a marker-less group).
///   3. **Legacy FLAT entry** — a top-level `{type, command}` matching the
///      injector script path or `erebyx hook-inject`. This is the
///      v0.1.0/v0.1.1 broken shape; matching it here is the upgrade path that
///      replaces the dead flat entry with the correct nested group.
///
/// Exact-form only — never a substring match on "erebyx" — so user-authored
/// erebyx-adjacent automation is preserved.
fn should_remove_hook_entry(entry: &serde_json::Value) -> bool {
    // 1. Marker on the group (or a legacy flat entry that carried it).
    if entry
        .get("_erebyx_managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    // 2. Group: descend into hooks[] and match our command.
    if let Some(handlers) = entry.get("hooks").and_then(|h| h.as_array()) {
        if handlers.iter().any(is_inject_command) {
            return true;
        }
    }
    // 3. Legacy flat entry: command at the top level.
    is_inject_command(entry)
}

/// True if a single hook-handler object's `command` is the erebyx
/// memory-injector invocation: the generated `*/erebyx-memory-injector.sh`
/// script, or the legacy bare `erebyx hook-inject` command.
fn is_inject_command(handler: &serde_json::Value) -> bool {
    let cmd = handler
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    cmd.ends_with("erebyx-memory-injector.sh") || cmd == "erebyx hook-inject"
}

/// Whether `setup` actually wired BOTH Claude Code hook groups into a parsed
/// `settings.json` — a managed `SessionStart` group AND a managed
/// `UserPromptSubmit` group. `doctor` calls this so it can detect the common
/// half-installed state where the injector script exists on disk but the
/// settings.json was never updated (or was reverted by an unrelated edit),
/// which silently disables memory injection.
///
/// Reuses the same `should_remove_*` managed-entry predicates that `setup`
/// uses to find/replace its own groups, so doctor and setup can never drift
/// on what "our group" means.
pub(crate) struct HookRegistration {
    pub session_start: bool,
    pub user_prompt_submit: bool,
}

impl HookRegistration {
    /// True only when BOTH managed groups are present.
    pub(crate) fn fully_wired(&self) -> bool {
        self.session_start && self.user_prompt_submit
    }
}

/// Inspect a parsed Claude Code `settings.json` value for our two managed
/// hook groups. A missing `hooks` object, a non-array entry, or an absent
/// key all read as "not registered" for that group (never panics).
pub(crate) fn hook_registration_status(settings: &serde_json::Value) -> HookRegistration {
    let group_present = |key: &str, pred: fn(&serde_json::Value) -> bool| -> bool {
        settings
            .get("hooks")
            .and_then(|h| h.get(key))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(pred))
            .unwrap_or(false)
    };
    HookRegistration {
        session_start: group_present("SessionStart", should_remove_session_start_entry),
        user_prompt_submit: group_present("UserPromptSubmit", should_remove_hook_entry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::detect::ClientKind;
    use serde_json::json;
    use tempfile::TempDir;

    /// Build a Claude-Code-shaped AiClient pointing at a tempdir so the
    /// settings.json writers can be exercised end-to-end.
    fn test_client(dir: &std::path::Path) -> AiClient {
        AiClient {
            kind: ClientKind::ClaudeCode,
            name: "Test Claude Code",
            config_path: dir.join("settings.json"),
            rules_path: dir.join("rules").join("erebyx-memory.md"),
            config_exists: false,
            home_dir: dir.to_path_buf(),
        }
    }

    // -------------------------------------------------------------
    // [1] HOOK NESTING — the written JSON must be a matcher-group with
    //     .hooks[] handlers, NOT a flat entry. (The headline fix.)
    // -------------------------------------------------------------

    /// UserPromptSubmit: the written array element is a GROUP carrying
    /// `_erebyx_managed` + a `hooks[]` of `{type, command}` handlers. A flat
    /// `{type,command}` element (the v0.1.0/v0.1.1 bug) is never executed by
    /// Claude Code, so this pins the nesting precisely.
    #[test]
    fn user_prompt_hook_is_written_as_matcher_group() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());
        let script_path = td.path().join("hooks").join("erebyx-memory-injector.sh");

        register_hook_in_settings(&client, &script_path).unwrap();

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();
        let arr = written["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "exactly one UserPromptSubmit entry");

        let group = &arr[0];
        // Marker is on the GROUP (the level CC iterates).
        assert_eq!(
            group["_erebyx_managed"].as_bool(),
            Some(true),
            "_erebyx_managed must be on the matcher-group, not the handler"
        );
        // UserPromptSubmit groups carry no matcher field (fires every prompt).
        // The handler lives under .hooks[].
        let handlers = group["hooks"].as_array().expect("group must have .hooks[]");
        assert_eq!(handlers.len(), 1, "one handler in the group");
        assert_eq!(handlers[0]["type"].as_str(), Some("command"));
        assert_eq!(
            handlers[0]["command"].as_str(),
            Some(script_path.to_string_lossy().as_ref()),
            "handler command is the injector script path"
        );
        // The handler itself must NOT carry the managed marker.
        assert!(
            handlers[0].get("_erebyx_managed").is_none(),
            "marker belongs on the group, not the inner handler"
        );
    }

    /// SessionStart: the written array element is a GROUP with NO `matcher`
    /// (match-all → fires on startup/resume/clear/compact), `_erebyx_managed`,
    /// and a `hooks[]` whose handler command is the ABSOLUTE-path session-start
    /// invocation. A pinned `matcher:"startup"` would skip resume/clear/compact
    /// — the continuity-critical wakes — so the group MUST omit the matcher.
    #[test]
    fn session_start_hook_is_written_as_matcher_group_match_all() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());

        register_session_start_hook(&client).unwrap();

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();
        let arr = written["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "exactly one SessionStart entry");

        let group = &arr[0];
        assert!(
            group.get("matcher").is_none(),
            "SessionStart group must OMIT the matcher (match-all over startup/resume/clear/compact), got {:?}",
            group.get("matcher")
        );
        assert_eq!(group["_erebyx_managed"].as_bool(), Some(true));
        let handlers = group["hooks"].as_array().expect("group must have .hooks[]");
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0]["type"].as_str(), Some("command"));
        let cmd = handlers[0]["command"].as_str().unwrap();
        assert!(
            cmd.ends_with("hook-session-start"),
            "command must invoke hook-session-start, got {cmd:?}"
        );
        // [3]: absolute exe path — under cargo test current_exe resolves, so
        // the command must be an absolute path, NOT bare `erebyx`.
        assert!(
            cmd != "erebyx hook-session-start",
            "command must use the absolute binary path, not bare `erebyx`"
        );
    }

    // -------------------------------------------------------------
    // [2] DEDUP DESCEND + LEGACY CLEANUP
    // -------------------------------------------------------------

    /// Registering twice leaves EXACTLY ONE erebyx matcher-group in each
    /// array (idempotency through the new group-aware retention).
    #[test]
    fn register_twice_leaves_exactly_one_group_each() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());
        let script_path = td.path().join("hooks").join("erebyx-memory-injector.sh");

        register_hook_in_settings(&client, &script_path).unwrap();
        register_session_start_hook(&client).unwrap();
        // Second run (simulating a re-`erebyx setup`).
        register_hook_in_settings(&client, &script_path).unwrap();
        register_session_start_hook(&client).unwrap();

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();
        assert_eq!(
            written["hooks"]["UserPromptSubmit"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "UserPromptSubmit must not accumulate duplicate groups"
        );
        assert_eq!(
            written["hooks"]["SessionStart"].as_array().unwrap().len(),
            1,
            "SessionStart must not accumulate duplicate groups"
        );
    }

    /// A pre-seeded OLD FLAT entry (the v0.1.0/v0.1.1 broken shape) gets
    /// removed and replaced with the correct nested group on the next run —
    /// the upgrade path. User-authored hooks alongside survive.
    #[test]
    fn old_flat_entries_are_cleaned_up_and_replaced_with_nested_groups() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());
        let script_path = td.path().join("hooks").join("erebyx-memory-injector.sh");

        // Seed settings.json with the OLD broken flat entries + a
        // user-authored hook that must be preserved.
        let seed = json!({
            "hooks": {
                "UserPromptSubmit": [
                    // v0.1.1 flat (dead) entry — must be removed.
                    {"type": "command", "command": script_path.to_string_lossy(), "_erebyx_managed": true},
                    // user automation that merely mentions erebyx — must survive.
                    {"type": "command", "command": "/Users/me/bin/erebyx-archive-export"}
                ],
                "SessionStart": [
                    // v0.1.1 flat (dead) entry — must be removed.
                    {"type": "command", "command": "erebyx hook-session-start", "_erebyx_managed": true},
                    // user automation — must survive.
                    {"hooks": [{"type": "command", "command": "echo hi"}]}
                ]
            }
        });
        std::fs::write(
            &client.config_path,
            serde_json::to_string_pretty(&seed).unwrap(),
        )
        .unwrap();

        register_hook_in_settings(&client, &script_path).unwrap();
        register_session_start_hook(&client).unwrap();

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();

        // --- UserPromptSubmit ---
        let ups = written["hooks"]["UserPromptSubmit"].as_array().unwrap();
        // user automation + exactly one new nested erebyx group = 2 entries.
        assert_eq!(
            ups.len(),
            2,
            "old flat + new group must not coexist: {ups:#?}"
        );
        // The user automation survived.
        assert!(
            ups.iter()
                .any(|e| e["command"].as_str() == Some("/Users/me/bin/erebyx-archive-export")),
            "user-authored erebyx-adjacent hook must be preserved"
        );
        // Exactly one nested erebyx group (has .hooks[] with our command).
        let nested_erebyx = ups
            .iter()
            .filter(|e| {
                e.get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| hs.iter().any(is_inject_command))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(nested_erebyx, 1, "exactly one nested erebyx group survives");
        // No FLAT erebyx entry remains.
        assert!(
            !ups.iter()
                .any(|e| e.get("hooks").is_none() && is_inject_command(e)),
            "the old flat erebyx entry must be gone"
        );

        // --- SessionStart ---
        let ss = written["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(
            ss.len(),
            2,
            "old flat + new group must not coexist: {ss:#?}"
        );
        // The user automation (echo hi group) survived.
        assert!(
            ss.iter().any(|e| {
                e.get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| hs.iter().any(|h| h["command"].as_str() == Some("echo hi")))
                    .unwrap_or(false)
            }),
            "user-authored SessionStart group must be preserved"
        );
        let nested_erebyx = ss
            .iter()
            .filter(|e| {
                e.get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| hs.iter().any(is_session_start_command))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            nested_erebyx, 1,
            "exactly one nested erebyx session-start group survives"
        );
        assert!(
            !ss.iter()
                .any(|e| e.get("hooks").is_none() && is_session_start_command(e)),
            "the old flat erebyx session-start entry must be gone"
        );
    }

    /// Group-aware retention without a marker: a matcher-group whose hooks[]
    /// contains our command is removed even if it lacks `_erebyx_managed`.
    #[test]
    fn group_without_marker_but_with_our_command_is_removed() {
        let script_group = json!({
            "hooks": [{"type": "command", "command": "/x/erebyx-memory-injector.sh"}]
        });
        assert!(should_remove_hook_entry(&script_group));

        let ss_group = json!({
            "matcher": "startup",
            "hooks": [{"type": "command", "command": "/abs/erebyx hook-session-start"}]
        });
        assert!(should_remove_session_start_entry(&ss_group));
    }

    /// A user-authored matcher-group whose hooks don't reference erebyx must
    /// be preserved by both predicates.
    #[test]
    fn user_authored_group_is_preserved() {
        let group = json!({
            "hooks": [{"type": "command", "command": "/Users/me/bin/erebyx-archive-export"}]
        });
        assert!(!should_remove_hook_entry(&group));
        let ss_group = json!({
            "matcher": "startup",
            "hooks": [{"type": "command", "command": "my-own-startup"}]
        });
        assert!(!should_remove_session_start_entry(&ss_group));
    }

    // -------------------------------------------------------------
    // [3] ABSOLUTE EXE PATH in the generated script
    // -------------------------------------------------------------

    /// The generated script's exec line uses the absolute, single-quoted
    /// binary path — never a bare `exec erebyx`. Under cargo test
    /// `current_exe` resolves, so the path is absolute.
    #[test]
    fn hook_script_exec_line_uses_absolute_quoted_path() {
        let script = hook_script("https://core.erebyx.com");
        let expected_exe = shell_single_quote(&erebyx_command());
        assert!(
            script.contains(&format!("exec {expected_exe} hook-inject")),
            "exec line must use the single-quoted absolute exe path. Script:\n{script}"
        );
        // The old bare form must be gone.
        assert!(
            !script.contains("exec erebyx hook-inject"),
            "bare `exec erebyx hook-inject` (relies on $PATH) must be gone. Script:\n{script}"
        );
    }

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

    // -------------------------------------------------------------
    // [F4] hook_registration_status — doctor's registration probe.
    // -------------------------------------------------------------

    /// A settings.json that `setup` fully wired (both register fns run) must
    /// read back as `fully_wired()`.
    #[test]
    fn registration_status_reports_fully_wired_after_setup() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());
        let script_path = td.path().join("hooks").join("erebyx-memory-injector.sh");

        register_session_start_hook(&client).unwrap();
        register_hook_in_settings(&client, &script_path).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();
        let reg = hook_registration_status(&settings);
        assert!(reg.session_start, "SessionStart group must be detected");
        assert!(
            reg.user_prompt_submit,
            "UserPromptSubmit group must be detected"
        );
        assert!(reg.fully_wired(), "both groups present => fully wired");
    }

    /// Only the SessionStart group registered (the half-installed state) must
    /// NOT read as fully wired.
    #[test]
    fn registration_status_detects_missing_user_prompt_group() {
        let td = TempDir::new().unwrap();
        let client = test_client(td.path());

        register_session_start_hook(&client).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&client.config_path).unwrap()).unwrap();
        let reg = hook_registration_status(&settings);
        assert!(reg.session_start, "SessionStart present");
        assert!(!reg.user_prompt_submit, "UserPromptSubmit absent");
        assert!(
            !reg.fully_wired(),
            "half-installed must not read fully wired"
        );
    }

    /// An empty / hook-less settings.json reads as not registered for either
    /// group, and never panics on the missing `hooks` object.
    #[test]
    fn registration_status_empty_settings_is_unwired() {
        let reg = hook_registration_status(&json!({}));
        assert!(!reg.session_start);
        assert!(!reg.user_prompt_submit);
        assert!(!reg.fully_wired());

        // A user-authored, non-erebyx hook group must NOT be mistaken for ours.
        let foreign = json!({
            "hooks": {
                "SessionStart": [
                    { "hooks": [ { "type": "command", "command": "/usr/bin/true" } ] }
                ]
            }
        });
        let reg = hook_registration_status(&foreign);
        assert!(
            !reg.session_start,
            "a foreign (non-managed) hook group must not count as our registration"
        );
        assert!(!reg.fully_wired());
    }
}
