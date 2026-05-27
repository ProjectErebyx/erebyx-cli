// SPDX-License-Identifier: Apache-2.0
//! MCP configuration file writing for each AI client.
//!
//! Each client has a different config format. We merge into existing configs
//! rather than overwriting them. Config files containing API keys are written
//! with 0600 permissions (owner read/write only).

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

use super::detect::{AiClient, ClientKind};

/// Write MCP server configuration for a specific client.
/// Returns the path where config was written.
pub fn write_mcp_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    // Ensure parent directory exists
    if let Some(parent) = client.config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    match client.kind {
        ClientKind::ClaudeCode => write_claude_code_config(client, api_key, api_url),
        ClientKind::Cursor => write_standard_mcp_config(client, api_key, api_url),
        ClientKind::Windsurf => write_standard_mcp_config(client, api_key, api_url),
        ClientKind::Continue => write_continue_config(client, api_key, api_url),
        ClientKind::Zed => write_zed_config(client, api_key, api_url),
        ClientKind::VsCodeCopilot => write_vscode_config(client, api_key, api_url),
    }
}

/// The MCP server entry common to most clients.
///
/// Points at the local `erebyx` binary running `mcp-serve` — a stdio bridge
/// that forwards JSON-RPC to the substrate over HTTPS. The binary is whatever
/// `erebyx setup` was invoked from, so the AI client launches the same
/// version the customer just installed.
fn erebyx_server_entry(api_key: &str, api_url: &str) -> Value {
    json!({
        "command": erebyx_command(),
        "args": ["mcp-serve"],
        "env": {
            "EREBYX_API_KEY": api_key,
            "EREBYX_API_URL": api_url,
            "EREBYX_INSTANCE_ID": "default"
        }
    })
}

/// Resolve the `erebyx` binary path that AI clients should launch.
///
/// Prefers the absolute path of the currently-running binary so the launched
/// MCP server is always the same version the user just ran `erebyx setup`
/// from. Falls back to the bare command name if the current exe path can't
/// be resolved (the client will then rely on `$PATH`).
fn erebyx_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "erebyx".to_string())
}

/// Extract a mutable JSON object reference, returning a descriptive error
/// when the value is not an object (e.g. malformed config file).
fn require_object_mut(value: &mut Value, config_path: &PathBuf, key_context: &str) -> Result<()> {
    if value.as_object_mut().is_none() {
        bail!(
            "Expected JSON object for '{}' in {}, but found {}. \
             Fix the config file or delete it so erebyx can recreate it.",
            key_context,
            config_path.display(),
            value_type_name(value),
        );
    }
    Ok(())
}

/// Human-readable name for a JSON value type.
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Claude Code: ~/.claude/settings.json
/// Format: { "mcpServers": { "erebyx-os": { ... } } }
fn write_claude_code_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    let mcp_servers = config
        .as_object_mut()
        .expect("validated above")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    require_object_mut(mcp_servers, &client.config_path, "mcpServers")?;
    mcp_servers
        .as_object_mut()
        .expect("validated above")
        .insert("erebyx-os".to_string(), erebyx_server_entry(api_key, api_url));

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Standard MCP config for Cursor, Windsurf
/// Format: { "mcpServers": { "erebyx-os": { ... } } }
fn write_standard_mcp_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    let mcp_servers = config
        .as_object_mut()
        .expect("validated above")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    require_object_mut(mcp_servers, &client.config_path, "mcpServers")?;
    mcp_servers
        .as_object_mut()
        .expect("validated above")
        .insert("erebyx-os".to_string(), erebyx_server_entry(api_key, api_url));

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Continue: ~/.continue/config.json
/// Format: { "experimental": { "mcpServers": { "erebyx-os": { ... } } } }
fn write_continue_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    let experimental = config
        .as_object_mut()
        .expect("validated above")
        .entry("experimental")
        .or_insert_with(|| json!({}));

    require_object_mut(experimental, &client.config_path, "experimental")?;
    let mcp_servers = experimental
        .as_object_mut()
        .expect("validated above")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    require_object_mut(mcp_servers, &client.config_path, "experimental.mcpServers")?;
    mcp_servers
        .as_object_mut()
        .expect("validated above")
        .insert("erebyx-os".to_string(), erebyx_server_entry(api_key, api_url));

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Zed: ~/.config/zed/settings.json
/// Format: { "context_servers": { "erebyx-os": { "command": { ... } } } }
fn write_zed_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    let context_servers = config
        .as_object_mut()
        .expect("validated above")
        .entry("context_servers")
        .or_insert_with(|| json!({}));

    require_object_mut(context_servers, &client.config_path, "context_servers")?;
    context_servers
        .as_object_mut()
        .expect("validated above")
        .insert(
            "erebyx-os".to_string(),
            json!({
                "command": {
                    "path": erebyx_command(),
                    "args": ["mcp-serve"],
                    "env": {
                        "EREBYX_API_KEY": api_key,
                        "EREBYX_API_URL": api_url,
                        "EREBYX_INSTANCE_ID": "default"
                    }
                }
            }),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// VS Code / Copilot: settings.json
/// Format: { "mcp": { "servers": { "erebyx-os": { "type": "stdio", ... } } } }
fn write_vscode_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    let mcp = config
        .as_object_mut()
        .expect("validated above")
        .entry("mcp")
        .or_insert_with(|| json!({}));

    require_object_mut(mcp, &client.config_path, "mcp")?;
    let servers = mcp
        .as_object_mut()
        .expect("validated above")
        .entry("servers")
        .or_insert_with(|| json!({}));

    require_object_mut(servers, &client.config_path, "mcp.servers")?;
    servers
        .as_object_mut()
        .expect("validated above")
        .insert(
            "erebyx-os".to_string(),
            json!({
                "type": "stdio",
                "command": erebyx_command(),
                "args": ["mcp-serve"],
                "env": {
                    "EREBYX_API_KEY": api_key,
                    "EREBYX_API_URL": api_url,
                    "EREBYX_INSTANCE_ID": "default"
                }
            }),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Read an existing JSON file or return an empty object.
fn read_json_or_empty(path: &PathBuf) -> Result<Value> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        // Handle empty files
        if content.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON in {}", path.display()))
    } else {
        Ok(json!({}))
    }
}

/// True if `path` lives inside a git working tree (any ancestor contains
/// a `.git` directory or file). Bool, not Result — a stat failure on an
/// ancestor is treated as "not in a tree" (fail-soft so unrelated
/// permission errors don't block legitimate writes).
fn is_within_git_tree(path: &std::path::Path) -> bool {
    path.ancestors().any(|ancestor| {
        let dot_git = ancestor.join(".git");
        dot_git.exists()
    })
}

/// Write JSON to a file with pretty formatting.
///
/// On Unix, sets 0600 (owner read/write only) so the embedded API key is
/// not world-readable. On Windows, file permissions inherit the parent
/// directory's ACL — typically user-profile-scoped but not guaranteed.
/// Emits a one-line warning so the user knows the difference.
///
/// Refuses to write inside a git working tree without an explicit
/// `EREBYX_ALLOW_GIT_TREE_CONFIG=1` override. Many users sync
/// `~/.claude/` and similar dotfiles to public repos; silently writing
/// an API key into those would leak credentials at next `git add .`.
fn write_json(path: &PathBuf, value: &Value) -> Result<()> {
    // Refuse to write a credential-bearing config inside a git working
    // tree unless the user has explicitly opted in. This catches the
    // public-dotfiles-repo footgun before it lands a credential in
    // version control.
    if is_within_git_tree(path) && std::env::var("EREBYX_ALLOW_GIT_TREE_CONFIG").is_err() {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Many users sync their config dirs to public repos; a \
             plaintext API key there would leak on the next commit. Set \
             EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move your \
             client config outside the tree.",
            path.display()
        );
    }

    let content = serde_json::to_string_pretty(value)
        .context("Failed to serialize JSON")?;
    std::fs::write(path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    // Restrict permissions to owner-only read/write (config files contain API keys)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }
    // P0-4 (2026-05-27): Windows has no portable in-process way to set a
    // user-only DACL without an extra dependency. Document the gap so a
    // Windows operator running `erebyx setup` sees the warning and can
    // tighten permissions out-of-band. The file lands at %APPDATA% or
    // %USERPROFILE% by default, which is already user-profile-scoped on a
    // single-user box — the risk is multi-user Windows hosts and roaming
    // profiles. A future v0.1.2 will wire `windows-acl` to close this.
    #[cfg(windows)]
    {
        eprintln!(
            "  ⚠ {}: file permissions cannot be auto-restricted on Windows. \
             Confirm %APPDATA% is not world-readable.",
            path.display()
        );
    }

    Ok(())
}
