// SPDX-License-Identifier: Apache-2.0
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
use std::path::PathBuf;

use super::detect::AiClient;

/// The hook script that runs on every user message in Claude Code.
///
/// Trivial wrapper: hands stdin off to `erebyx hook-inject`. All logic
/// (gating, query, REST call, formatting) lives in the native binary.
/// The API key is read from $EREBYX_API_KEY by the binary itself.
fn hook_script(api_url: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Erebyx Memory Injector — Claude Code UserPromptSubmit Hook
# Hands stdin to the native `erebyx hook-inject` handler.
# Fail-open: any error path emits {{}}.

set -u

# Fail open if API key is not set
if [ -z "${{EREBYX_API_KEY:-}}" ]; then
    printf '%s' '{{}}'
    exit 0
fi

# Pass api_url through env so hook-inject doesn't need flag parsing.
export EREBYX_API_URL="${{EREBYX_API_URL:-{api_url}}}"

# Single native call. If it fails for any reason, emit {{}} and exit clean.
exec erebyx hook-inject 2>/dev/null || printf '%s' '{{}}'
"#,
        api_url = api_url,
    )
}

/// Install Claude Code hooks for automatic memory injection.
pub fn install_hooks(client: &AiClient, _api_key: &str, api_url: &str) -> Result<()> {
    // P1-4 (2026-05-27): the hook script uses bash + Unix path conventions.
    // Claude Code on Windows doesn't invoke `.sh` files directly (it wants
    // `.bat` / `.cmd` / `.ps1`). Pre-fix `erebyx setup` reported success on
    // Windows while writing a script that never ran — customers thought
    // hooks were installed and never received memory injection, with no
    // error signal. v0.1.2 will ship the PowerShell variant; v0.1.1 punts
    // cleanly with a clear message so the customer knows what landed and
    // what didn't.
    #[cfg(target_os = "windows")]
    {
        let _ = api_url; // unused on this branch
        let _ = client;
        eprintln!(
            "  ⚠ Claude Code hooks: Windows support is not yet implemented. \
             The MCP server entry is still configured — `restore_identity` \
             works manually. Hook-based auto-injection arrives in v0.1.2."
        );
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

    // Register hook in Claude Code settings
    register_hook_in_settings(client, &script_path)?;

    Ok(())
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
fn register_hook_in_settings(client: &AiClient, script_path: &PathBuf) -> Result<()> {
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
        // Remove only entries we PREVIOUSLY installed. Accept three
        // marker shapes for back-compat with v0.1.0 installs that
        // didn't carry the explicit `_erebyx_managed` flag:
        //   1. Explicit managed marker (current canon).
        //   2. Exact-path match for `erebyx-memory-injector.sh`.
        //   3. Exact `erebyx hook-inject` command string (legacy invocation).
        // Anything else stays untouched — including custom user
        // automation that happens to mention "erebyx" in its path.
        arr.retain(|h| {
            let is_managed = h
                .get("_erebyx_managed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_managed {
                return false;
            }
            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
            !(cmd.ends_with("erebyx-memory-injector.sh") || cmd == "erebyx hook-inject")
        });
        arr.push(hook_entry);
    }

    let content = serde_json::to_string_pretty(&config)
        .context("Failed to serialize settings JSON")?;
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
