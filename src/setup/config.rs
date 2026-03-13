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
fn erebyx_server_entry(api_key: &str, api_url: &str) -> Value {
    json!({
        "command": "uvx",
        "args": ["--from", "erebyx-os[mcp]", "python", "-m", "core.erebyx_mcp.server"],
        "env": {
            "EREBYX_API_KEY": api_key,
            "EREBYX_API_URL": api_url,
            "EREBYX_INSTANCE_ID": "default"
        }
    })
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
                    "path": "uvx",
                    "args": ["--from", "erebyx-os[mcp]", "python", "-m", "core.erebyx_mcp.server"],
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
                "command": "uvx",
                "args": ["--from", "erebyx-os[mcp]", "python", "-m", "core.erebyx_mcp.server"],
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

/// Write JSON to a file with pretty formatting.
/// Sets 0600 permissions (owner read/write only) since configs contain API keys.
fn write_json(path: &PathBuf, value: &Value) -> Result<()> {
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

    Ok(())
}
