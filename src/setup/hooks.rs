//! Claude Code hooks -- automatic memory injection before every response.
//!
//! Installs a UserPromptSubmit hook that:
//! 1. Extracts topic from user prompt
//! 2. Calls Erebyx-OS REST API to search memories
//! 3. Injects top memories as additionalContext
//!
//! Smart gate: skips messages <15 chars or greeting patterns.
//! Timeout: 500ms hard limit (fail open).
//!
//! Security: user content is never interpolated into shell or Python source.
//! All untrusted data flows through stdin/pipes to avoid injection.
//! API key is read from $EREBYX_API_KEY at runtime, never embedded in scripts.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

use super::detect::AiClient;

/// Check that python3 is available on PATH.
/// Returns Ok(()) if found, or a descriptive error if missing.
pub fn check_python3_available() -> Result<()> {
    let found = Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !found {
        bail!(
            "python3 is required for Erebyx memory hooks but was not found on PATH.\n\
             Install Python 3 and ensure `python3` is available:\n\
             - macOS: `brew install python3` or download from https://python.org\n\
             - Ubuntu/Debian: `sudo apt install python3`\n\
             - Fedora: `sudo dnf install python3`\n\
             Then re-run `erebyx setup`."
        );
    }

    Ok(())
}

/// The hook script that runs on every user message in Claude Code.
///
/// Security: all user content and API responses flow through stdin/pipes.
/// Nothing untrusted is ever interpolated into shell variables or Python source.
/// The API key is read from $EREBYX_API_KEY at runtime (set by MCP server env config).
fn hook_script(api_url: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Erebyx-OS Memory Injector -- Claude Code UserPromptSubmit Hook
# Automatically retrieves relevant memories before every response.
# Fail-open: if memory is unavailable, proceeds without injection.
# Timeout: 500ms hard limit.
# Requires: EREBYX_API_KEY environment variable

set -euo pipefail

# Fail open if API key is not set
if [ -z "${{EREBYX_API_KEY:-}}" ]; then
    echo '{{}}'
    exit 0
fi

# Read raw hook input from stdin into a temp file so multiple consumers can read it
HOOK_INPUT=$(cat)

# Extract user message via Python, piping raw JSON through stdin (no interpolation)
USER_MESSAGE=$(printf '%s' "$HOOK_INPUT" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(data.get('user_message', ''))
except Exception:
    print('')
" 2>/dev/null || echo "")

# Smart gate: skip short messages and greetings
MSG_LEN=${{#USER_MESSAGE}}
if [ "$MSG_LEN" -lt 15 ]; then
    echo '{{}}'
    exit 0
fi

# Skip common greetings
LOWER_MSG=$(echo "$USER_MESSAGE" | tr '[:upper:]' '[:lower:]')
case "$LOWER_MSG" in
    hey*|hi*|hello*|thanks*|thank\ you*|bye*|ok*|yes|no|sure|cool|nice)
        echo '{{}}'
        exit 0
        ;;
esac

# Build the JSON search payload safely via Python stdin pipe
SEARCH_PAYLOAD=$(printf '%s' "$USER_MESSAGE" | python3 -c "
import sys, json
query = sys.stdin.read()[:200]
print(json.dumps({{'query': query, 'limit': 5}}))" 2>/dev/null || echo "")

if [ -z "$SEARCH_PAYLOAD" ]; then
    echo '{{}}'
    exit 0
fi

# Call Erebyx-OS REST API with 500ms timeout, payload from variable (no interpolation risk)
RESPONSE=$(printf '%s' "$SEARCH_PAYLOAD" | curl -s --max-time 0.5 \
    -X POST "{api_url}/v0/memories/search" \
    -H "Content-Type: application/json" \
    -H "X-API-Key: $EREBYX_API_KEY" \
    -H "X-Instance-ID: default" \
    -d @- \
    2>/dev/null || echo "")

# If no response or error, fail open
if [ -z "$RESPONSE" ]; then
    echo '{{}}'
    exit 0
fi

# Parse API response and format injection context via Python stdin pipe
printf '%s' "$RESPONSE" | python3 -c "
import json, sys

try:
    data = json.load(sys.stdin)
    memories = data.get('memories', data.get('results', []))
    if not memories:
        print(json.dumps({{}}))
        sys.exit(0)

    lines = ['[Erebyx-OS Memory Context]']
    total = 0
    for m in memories[:5]:
        content = m.get('content', m.get('text', ''))[:300]
        if total + len(content) > 1500:
            break
        lines.append(f'- {{content}}')
        total += len(content)

    context_text = '\n'.join(lines)
    if context_text.strip():
        result = {{
            'additionalContext': [{{
                'type': 'text',
                'text': context_text
            }}]
        }}
        print(json.dumps(result))
    else:
        print(json.dumps({{}}))
except Exception:
    print(json.dumps({{}}))
" 2>/dev/null || echo '{{}}'
"#,
        api_url = api_url,
    )
}

/// Install Claude Code hooks for automatic memory injection.
pub fn install_hooks(client: &AiClient, _api_key: &str, api_url: &str) -> Result<()> {
    // Verify python3 is available before installing a hook that depends on it
    check_python3_available()?;

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

    // Build and insert the hook entry
    let hook_entry = serde_json::json!({
        "type": "command",
        "command": script_path.to_string_lossy()
    });

    if let Some(arr) = user_prompt_hooks.as_array_mut() {
        // Remove existing erebyx hooks before adding the current one
        arr.retain(|h| {
            !h.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.contains("erebyx"))
                .unwrap_or(false)
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
