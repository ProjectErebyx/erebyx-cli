// SPDX-License-Identifier: MIT OR Apache-2.0
//! MCP configuration file writing for each AI client.
//!
//! Each client has a different config format. We merge into existing configs
//! rather than overwriting them. Config files containing API keys are written
//! with 0600 permissions (owner read/write only).

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

use super::detect::{AiClient, ClientKind};

// Shared secure-write helpers. Declared here (rather than in `mod.rs`) so the
// new module is compiled + reachable as `super::config::secure_write` from
// `hooks.rs` and `rules.rs` without modifying `mod.rs`.
#[path = "secure_write.rs"]
pub mod secure_write;

use secure_write::{atomic_write_secret, erebyx_command};

/// Write MCP server configuration for a specific client.
/// Returns the path where config was written.
pub fn write_mcp_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    // Ensure parent directory exists (0o700 on Unix when we create it).
    if let Some(parent) = client.config_path.parent() {
        secure_write::ensure_dir_secure(parent)?;
    }

    match client.kind {
        ClientKind::ClaudeCode => write_claude_code_config(client, api_key, api_url, instance_id),
        ClientKind::Cursor => write_standard_mcp_config(client, api_key, api_url, instance_id),
        ClientKind::Windsurf => write_standard_mcp_config(client, api_key, api_url, instance_id),
        ClientKind::Continue => write_continue_config(client, api_key, api_url, instance_id),
        ClientKind::Zed => write_zed_config(client, api_key, api_url, instance_id),
        ClientKind::VsCodeCopilot => write_vscode_config(client, api_key, api_url, instance_id),
        // v0.1.3 — JSON `mcpServers`-object family (only the path differs from
        // Cursor/Windsurf). Merge-preserves any other MCP servers the user has.
        ClientKind::GeminiCli
        | ClientKind::ClaudeDesktop
        | ClientKind::Cline
        | ClientKind::Antigravity => {
            write_standard_mcp_config(client, api_key, api_url, instance_id)
        }
        // v0.1.3 — distinct serializer families.
        ClientKind::Codex => write_codex_config(client, api_key, api_url, instance_id),
        ClientKind::GrokCli => write_grok_config(client, api_key, api_url, instance_id),
        ClientKind::Goose => write_goose_config(client, api_key, api_url, instance_id),
    }
}

/// Render the EXACT config snippet `write_mcp_config` would merge in, against
/// an empty base — for the `--dry-run` preview. Pure (no disk I/O), so it can
/// run offline and is the schema reviewers see in dry-run output. The real
/// writer merges this into the user's existing file (preserving other servers);
/// this preview shows the erebyx contribution in isolation.
pub fn preview_mcp_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> String {
    match client.kind {
        // JSON `mcpServers`-object family.
        ClientKind::ClaudeCode
        | ClientKind::Cursor
        | ClientKind::Windsurf
        | ClientKind::GeminiCli
        | ClientKind::ClaudeDesktop
        | ClientKind::Cline
        | ClientKind::Antigravity => {
            let v = json!({ "mcpServers": { "erebyx-os": erebyx_server_entry(api_key, api_url, instance_id) } });
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        ClientKind::Continue => {
            let is_yaml = client
                .config_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
                .unwrap_or(true);
            if is_yaml {
                let entry = format!(
                    "  erebyx-os:\n    command: {cmd}\n    args:\n      - mcp-serve\n    env:\n      EREBYX_API_KEY: {key}\n      EREBYX_API_URL: {url}\n      EREBYX_INSTANCE_ID: {instance}\n",
                    cmd = erebyx_command(),
                    key = yaml_dq_string(api_key),
                    url = yaml_dq_string(api_url),
                    instance = yaml_dq_string(instance_id),
                );
                format!("# EREBYX-START\nmcpServers:\n{entry}# EREBYX-END")
            } else {
                let v = json!({ "experimental": { "mcpServers": { "erebyx-os": erebyx_server_entry(api_key, api_url, instance_id) } } });
                serde_json::to_string_pretty(&v).unwrap_or_default()
            }
        }
        ClientKind::Zed => {
            let v = json!({ "context_servers": { "erebyx-os": {
                "command": { "path": erebyx_command(), "args": ["mcp-serve"], "env": {
                    "EREBYX_API_KEY": api_key, "EREBYX_API_URL": api_url, "EREBYX_INSTANCE_ID": instance_id } } } } });
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        ClientKind::VsCodeCopilot => {
            let v = json!({ "mcp": { "servers": { "erebyx-os": {
                "type": "stdio", "command": erebyx_command(), "args": ["mcp-serve"], "env": {
                    "EREBYX_API_KEY": api_key, "EREBYX_API_URL": api_url, "EREBYX_INSTANCE_ID": instance_id } } } } });
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        ClientKind::Codex => {
            let cmd = erebyx_command();
            format!(
                "# EREBYX-START\n[mcp_servers.\"erebyx-os\"]\ncommand = {cmd}\nargs = [{arg}]\n\n[mcp_servers.\"erebyx-os\".env]\nEREBYX_API_KEY = {key}\nEREBYX_API_URL = {url}\nEREBYX_INSTANCE_ID = {inst}\n# EREBYX-END",
                cmd = toml_basic_string(&cmd),
                arg = toml_basic_string("mcp-serve"),
                key = toml_basic_string(api_key),
                url = toml_basic_string(api_url),
                inst = toml_basic_string(instance_id),
            )
        }
        ClientKind::GrokCli => {
            let v = json!({ "mcp": { "servers": [ {
                "id": "erebyx-os", "label": "erebyx-os", "enabled": true, "transport": "stdio",
                "command": erebyx_command(), "args": ["mcp-serve"], "env": {
                    "EREBYX_API_KEY": api_key, "EREBYX_API_URL": api_url, "EREBYX_INSTANCE_ID": instance_id } } ] } });
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        ClientKind::Goose => {
            let cmd = erebyx_command();
            let entry = format!(
                "  erebyx-os:\n    type: stdio\n    name: erebyx-os\n    enabled: true\n    cmd: {cmd}\n    args:\n      - mcp-serve\n    envs:\n      EREBYX_API_KEY: {key}\n      EREBYX_API_URL: {url}\n      EREBYX_INSTANCE_ID: {instance}\n    timeout: 300\n",
                cmd = yaml_dq_string(&cmd),
                key = yaml_dq_string(api_key),
                url = yaml_dq_string(api_url),
                instance = yaml_dq_string(instance_id),
            );
            format!("# EREBYX-START\nextensions:\n{entry}# EREBYX-END")
        }
    }
}

/// The MCP server entry common to most clients.
///
/// Points at the local `erebyx` binary running `mcp-serve` — a stdio bridge
/// that forwards JSON-RPC to the substrate over HTTPS. The binary is whatever
/// `erebyx setup` was invoked from, so the AI client launches the same
/// version the customer just installed.
fn erebyx_server_entry(api_key: &str, api_url: &str, instance_id: &str) -> Value {
    json!({
        "command": erebyx_command(),
        "args": ["mcp-serve"],
        "env": {
            "EREBYX_API_KEY": api_key,
            "EREBYX_API_URL": api_url,
            "EREBYX_INSTANCE_ID": instance_id
        }
    })
}

/// Extract a mutable JSON object reference, returning a descriptive error
/// when the value is not an object (e.g. malformed config file).
fn require_object_mut(
    value: &mut Value,
    config_path: &std::path::Path,
    key_context: &str,
) -> Result<()> {
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
fn write_claude_code_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
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
        .insert(
            "erebyx-os".to_string(),
            erebyx_server_entry(api_key, api_url, instance_id),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Standard MCP config for Cursor, Windsurf
/// Format: { "mcpServers": { "erebyx-os": { ... } } }
fn write_standard_mcp_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
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
        .insert(
            "erebyx-os".to_string(),
            erebyx_server_entry(api_key, api_url, instance_id),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Continue: ~/.continue/config.yaml (current) or config.json (legacy).
///
/// YAML format (Continue ~2026-Q1+):
///   mcpServers:
///     erebyx-os:
///       command: erebyx
///       args: [mcp-serve]
///       env:
///         EREBYX_API_KEY: ...
///
/// JSON format (legacy, pre-YAML migration):
///   { "experimental": { "mcpServers": { "erebyx-os": { ... } } } }
///
/// Format chosen by file extension on `client.config_path` (set in detect.rs).
fn write_continue_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    let is_yaml = client
        .config_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
        .unwrap_or(false);

    if is_yaml {
        write_continue_yaml(client, api_key, api_url, instance_id)
    } else {
        write_continue_json_legacy(client, api_key, api_url, instance_id)
    }
}

/// Continue YAML — current format. Top-level `mcpServers`, no nesting.
fn write_continue_yaml(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    // Apply the SAME
    // git-tree credential guard the JSON path enforces. Continue users
    // frequently sync `~/.continue/` to public dotfiles repos; without
    // this guard, the YAML writer would silently leak the API key on
    // the next commit. Same allowlisted override (`EREBYX_ALLOW_GIT_TREE_CONFIG`).
    if is_within_git_tree(&client.config_path) && !env_flag_truthy("EREBYX_ALLOW_GIT_TREE_CONFIG") {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Set EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move \
             your Continue config outside the tree.",
            client.config_path.display()
        );
    }

    // YAML-escape api_key + api_url before
    // raw interpolation into the double-quoted YAML string. Real EREBYX
    // keys are alphanumeric (`erebyx_<48 hex>`), but the contract isn't
    // validated upstream — a key with `"` or `\` would silently produce
    // invalid YAML. Use the YAML double-quoted-flow-scalar escape rules:
    // backslash and double-quote each get a backslash prefix.
    let api_key_escaped = api_key.replace('\\', "\\\\").replace('"', "\\\"");
    let api_url_escaped = api_url.replace('\\', "\\\\").replace('"', "\\\"");
    let instance_id_escaped = instance_id.replace('\\', "\\\\").replace('"', "\\\"");

    // Read existing YAML if any. Continue's YAML is straightforward; we
    // do a string-level merge rather than depend on a yaml crate, to keep
    // the CLI dep footprint small and avoid reformatting user content.
    let existing = std::fs::read_to_string(&client.config_path).unwrap_or_default();
    let cleaned = strip_existing_erebyx_yaml_block(&existing);

    let entry = format!(
        "  erebyx-os:\n    command: {cmd}\n    args:\n      - mcp-serve\n    env:\n      EREBYX_API_KEY: \"{key}\"\n      EREBYX_API_URL: \"{url}\"\n      EREBYX_INSTANCE_ID: \"{instance}\"\n",
        cmd = erebyx_command(),
        key = api_key_escaped,
        url = api_url_escaped,
        instance = instance_id_escaped,
    );

    // If a top-level `mcpServers:` block exists, append our entry under
    // it. Otherwise, add the block at the end.
    //
    // The `\nmcpServers:` heuristic matches
    // any line starting with `mcpServers:` — INCLUDING commented-out
    // lines (`# mcpServers:`). Tighten by requiring the byte preceding
    // `\n` is also the start of a "real" line (not after `# `). Cheap
    // additional check.
    let real_mcp_servers_start = if cleaned.starts_with("mcpServers:") {
        Some(0usize)
    } else {
        // Scan for `\nmcpServers:` and verify the preceding line doesn't
        // start with a comment marker. This is approximate; perfect
        // YAML-aware parsing would require a yaml crate.
        let mut search_from = 0;
        loop {
            let Some(rel) = cleaned[search_from..].find("\nmcpServers:") else {
                break None;
            };
            let abs = search_from + rel + 1; // position of `m`
                                             // Walk back to the start of THIS line to inspect any leading whitespace + `#`.
            let line_start = cleaned[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix = &cleaned[line_start..abs];
            if !prefix.trim_start().starts_with('#') {
                break Some(abs);
            }
            search_from = abs;
        }
    };

    let new_content = if let Some(marker) = real_mcp_servers_start {
        let line_end = cleaned[marker..]
            .find('\n')
            .map(|i| marker + i + 1)
            .unwrap_or(cleaned.len());
        let mut out = String::with_capacity(cleaned.len() + entry.len() + 64);
        out.push_str(&cleaned[..line_end]);
        out.push_str(&format!("  # EREBYX-START\n{}  # EREBYX-END\n", entry));
        out.push_str(&cleaned[line_end..]);
        out
    } else {
        let mut out = cleaned.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("# EREBYX-START\nmcpServers:\n");
        out.push_str(&entry);
        out.push_str("# EREBYX-END\n");
        out
    };

    if let Some(parent) = client.config_path.parent() {
        secure_write::ensure_dir_secure(parent)?;
    }

    // Atomic + owner-only write. Previously a bare `fs::write` + post-hoc
    // chmod, which (a) left a window where the key-bearing YAML was
    // world-readable, and (b) could truncate the user's whole Continue
    // config on a crash. `atomic_write_secret` closes both — and folds the
    // Windows DACL tighten in at the write boundary, so no caller-side
    // hardening is needed here.
    atomic_write_secret(&client.config_path, new_content.as_bytes())?;

    Ok(client.config_path.clone())
}

/// Strip a previous `# EREBYX-START` / `# EREBYX-END`-marked block from the YAML,
/// so re-running setup is idempotent and doesn't accumulate stale entries.
fn strip_existing_erebyx_yaml_block(content: &str) -> String {
    if let (Some(s), Some(e)) = (content.find("# EREBYX-START"), content.find("# EREBYX-END")) {
        if s < e {
            let end = e + "# EREBYX-END".len();
            // Consume trailing newline + leading newline so we don't leave a gap.
            let real_end = content[end..]
                .find('\n')
                .map(|i| end + i + 1)
                .unwrap_or(end);
            let mut out = String::with_capacity(content.len());
            out.push_str(content[..s].trim_end());
            if !out.is_empty() && !content[real_end..].is_empty() {
                out.push('\n');
            }
            out.push_str(&content[real_end..]);
            return out;
        }
    }
    content.to_string()
}

/// Continue JSON legacy — pre-YAML migration. Kept for users who haven't moved yet.
fn write_continue_json_legacy(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
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
        .insert(
            "erebyx-os".to_string(),
            erebyx_server_entry(api_key, api_url, instance_id),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Zed: ~/.config/zed/settings.json
/// Format: { "context_servers": { "erebyx-os": { "command": { ... } } } }
fn write_zed_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
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
                        "EREBYX_INSTANCE_ID": instance_id
                    }
                }
            }),
        );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// VS Code / Copilot: settings.json
/// Format: { "mcp": { "servers": { "erebyx-os": { "type": "stdio", ... } } } }
fn write_vscode_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
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
    servers.as_object_mut().expect("validated above").insert(
        "erebyx-os".to_string(),
        json!({
            "type": "stdio",
            "command": erebyx_command(),
            "args": ["mcp-serve"],
            "env": {
                "EREBYX_API_KEY": api_key,
                "EREBYX_API_URL": api_url,
                "EREBYX_INSTANCE_ID": instance_id
            }
        }),
    );

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Grok Build CLI (`@vibe-kit/grok-cli`): ~/.grok/user-settings.json
///
/// Distinct from the JSON-OBJECT family: Grok stores MCP servers as a JSON
/// ARRAY at `mcp.servers`, where each element is a flat object carrying
/// `id`/`label`/`enabled`/`transport` plus the stdio fields. We merge BY NAME:
/// replace any existing `erebyx-os` element, keep every other server.
///
/// Schema (verified against superagent-ai/grok-cli source — `src/utils/settings.ts`):
///   { "mcp": { "servers": [ { id, label, enabled, transport:"stdio",
///                             command, args, env } ] } }
fn write_grok_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    let mut config = read_json_or_empty(&client.config_path)?;
    require_object_mut(&mut config, &client.config_path, "root")?;

    // Descend root -> mcp (object).
    let mcp = config
        .as_object_mut()
        .expect("validated above")
        .entry("mcp")
        .or_insert_with(|| json!({}));
    require_object_mut(mcp, &client.config_path, "mcp")?;

    // mcp.servers (ARRAY).
    let servers = mcp
        .as_object_mut()
        .expect("validated above")
        .entry("servers")
        .or_insert_with(|| json!([]));
    if !servers.is_array() {
        bail!(
            "Expected JSON array for 'mcp.servers' in {}, but found {}. \
             Fix the config file or delete it so erebyx can recreate it.",
            client.config_path.display(),
            value_type_name(servers),
        );
    }
    let arr = servers.as_array_mut().expect("validated above");

    // The flat erebyx server element. Grok's TS interface REQUIRES
    // id/label/enabled/transport; command/args/env are the stdio fields.
    let entry = json!({
        "id": "erebyx-os",
        "label": "erebyx-os",
        "enabled": true,
        "transport": "stdio",
        "command": erebyx_command(),
        "args": ["mcp-serve"],
        "env": {
            "EREBYX_API_KEY": api_key,
            "EREBYX_API_URL": api_url,
            "EREBYX_INSTANCE_ID": instance_id
        }
    });

    // Merge by name: drop any prior erebyx element (matched on id/name/label),
    // then push the fresh one. Preserves every other server's position/order.
    arr.retain(|el| {
        !["id", "name", "label"].iter().any(|k| {
            el.get(*k)
                .and_then(|v| v.as_str())
                .map(|s| s == "erebyx-os")
                .unwrap_or(false)
        })
    });
    arr.push(entry);

    write_json(&client.config_path, &config)?;
    Ok(client.config_path.clone())
}

/// Codex (OpenAI Codex CLI): ~/.codex/config.toml
///
/// TOML format. We hand-serialize (no TOML crate in this crate's dep set,
/// mirroring the hand-rolled Continue-YAML writer) a fenced block:
///
///   # EREBYX-START
///   [mcp_servers."erebyx-os"]
///   command = "…"
///   args = ["mcp-serve"]
///
///   [mcp_servers."erebyx-os".env]
///   EREBYX_API_KEY = "…"
///   EREBYX_API_URL = "https://core.erebyx.com"
///   EREBYX_INSTANCE_ID = "default"
///   # EREBYX-END
///
/// Merge-preserves every other `[mcp_servers.*]` table: we strip only the
/// prior `# EREBYX-START`/`# EREBYX-END` fence and re-append, never touching
/// user-authored tables.
fn write_codex_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    // Same git-tree credential guard the JSON/YAML writers enforce — the TOML
    // carries the API key in plaintext.
    if is_within_git_tree(&client.config_path) && !env_flag_truthy("EREBYX_ALLOW_GIT_TREE_CONFIG") {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Set EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move \
             your Codex config outside the tree.",
            client.config_path.display()
        );
    }

    let cmd = erebyx_command();
    let block = format!(
        "# EREBYX-START\n\
         [mcp_servers.\"erebyx-os\"]\n\
         command = {cmd}\n\
         args = [{arg}]\n\
         \n\
         [mcp_servers.\"erebyx-os\".env]\n\
         EREBYX_API_KEY = {key}\n\
         EREBYX_API_URL = {url}\n\
         EREBYX_INSTANCE_ID = {inst}\n\
         # EREBYX-END\n",
        cmd = toml_basic_string(&cmd),
        arg = toml_basic_string("mcp-serve"),
        key = toml_basic_string(api_key),
        url = toml_basic_string(api_url),
        inst = toml_basic_string(instance_id),
    );

    let existing = std::fs::read_to_string(&client.config_path).unwrap_or_default();
    let cleaned = strip_existing_erebyx_marked_block(&existing);

    let new_content = if cleaned.trim().is_empty() {
        block
    } else {
        format!("{}\n\n{}", cleaned.trim_end(), block)
    };

    if let Some(parent) = client.config_path.parent() {
        secure_write::ensure_dir_secure(parent)?;
    }
    atomic_write_secret(&client.config_path, new_content.as_bytes())?;
    Ok(client.config_path.clone())
}

/// Goose (Block "codename goose"): ~/.config/goose/config.yaml
///
/// YAML format. Top-level `extensions:` map (NOT `mcpServers`). Note the key
/// is `cmd` (NOT `command`) and `envs` (NOT `env`) per Goose's `ExtensionConfig`
/// schema. We hand-serialize a fenced block under `extensions:` and merge-
/// preserve every other extension, mirroring the Continue-YAML writer.
///
///   extensions:
///     # EREBYX-START
///     erebyx-os:
///       type: stdio
///       name: erebyx-os
///       enabled: true
///       cmd: …
///       args:
///         - mcp-serve
///       envs:
///         EREBYX_API_KEY: …
///         EREBYX_API_URL: https://core.erebyx.com
///         EREBYX_INSTANCE_ID: default
///       timeout: 300
///     # EREBYX-END
fn write_goose_config(
    client: &AiClient,
    api_key: &str,
    api_url: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    // Same git-tree credential guard — the YAML carries the API key in plaintext.
    if is_within_git_tree(&client.config_path) && !env_flag_truthy("EREBYX_ALLOW_GIT_TREE_CONFIG") {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Set EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move \
             your Goose config outside the tree.",
            client.config_path.display()
        );
    }

    let cmd = erebyx_command();
    // Indented two spaces under `extensions:`. Values that could be misread as
    // YAML scalars (the api_key/url/cmd) are double-quoted + escaped.
    let entry = format!(
        "  erebyx-os:\n    type: stdio\n    name: erebyx-os\n    enabled: true\n    cmd: {cmd}\n    args:\n      - mcp-serve\n    envs:\n      EREBYX_API_KEY: {key}\n      EREBYX_API_URL: {url}\n      EREBYX_INSTANCE_ID: {instance}\n    timeout: 300\n",
        cmd = yaml_dq_string(&cmd),
        key = yaml_dq_string(api_key),
        url = yaml_dq_string(api_url),
        instance = yaml_dq_string(instance_id),
    );

    let existing = std::fs::read_to_string(&client.config_path).unwrap_or_default();
    let cleaned = strip_existing_erebyx_yaml_block(&existing);

    // Find a real top-level `extensions:` block (column 0, not commented) and
    // append our fenced entry under it. Otherwise create the block.
    let real_extensions_start = if cleaned.starts_with("extensions:") {
        Some(0usize)
    } else {
        let mut search_from = 0;
        loop {
            let Some(rel) = cleaned[search_from..].find("\nextensions:") else {
                break None;
            };
            let abs = search_from + rel + 1; // position of `e`
            let line_start = cleaned[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix = &cleaned[line_start..abs];
            if !prefix.trim_start().starts_with('#') {
                break Some(abs);
            }
            search_from = abs;
        }
    };

    let new_content = if let Some(marker) = real_extensions_start {
        let line_end = cleaned[marker..]
            .find('\n')
            .map(|i| marker + i + 1)
            .unwrap_or(cleaned.len());
        let mut out = String::with_capacity(cleaned.len() + entry.len() + 64);
        out.push_str(&cleaned[..line_end]);
        out.push_str(&format!("  # EREBYX-START\n{}  # EREBYX-END\n", entry));
        out.push_str(&cleaned[line_end..]);
        out
    } else {
        let mut out = cleaned.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("# EREBYX-START\nextensions:\n");
        out.push_str(&entry);
        out.push_str("# EREBYX-END\n");
        out
    };

    if let Some(parent) = client.config_path.parent() {
        secure_write::ensure_dir_secure(parent)?;
    }
    atomic_write_secret(&client.config_path, new_content.as_bytes())?;
    Ok(client.config_path.clone())
}

/// Serialize a string as a TOML basic (double-quoted) string with the minimal
/// escape set TOML requires: backslash, double-quote, and the C0 controls that
/// appear in real values. EREBYX keys/URLs are ASCII, but the contract isn't
/// validated upstream so we escape defensively to never emit invalid TOML.
fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Serialize a string as a YAML double-quoted flow scalar. Backslash and
/// double-quote each get a backslash prefix (YAML double-quoted escape rules);
/// matches the inline escaping the Continue-YAML writer already uses.
fn yaml_dq_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Strip a previous `# EREBYX-START` / `# EREBYX-END` fence from a plain-text
/// (TOML) config, so re-running setup is idempotent. Variant of
/// `strip_existing_erebyx_yaml_block` that does NOT assume YAML indentation —
/// the fence is at column 0 in the Codex TOML writer.
fn strip_existing_erebyx_marked_block(content: &str) -> String {
    if let (Some(s), Some(e)) = (content.find("# EREBYX-START"), content.find("# EREBYX-END")) {
        if s < e {
            let end = e + "# EREBYX-END".len();
            let real_end = content[end..]
                .find('\n')
                .map(|i| end + i + 1)
                .unwrap_or(end);
            let mut out = String::with_capacity(content.len());
            out.push_str(content[..s].trim_end());
            if !out.is_empty() && !content[real_end..].trim().is_empty() {
                out.push('\n');
            }
            out.push_str(&content[real_end..]);
            return out;
        }
    }
    content.to_string()
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

/// True if `path` lives inside a git working tree we should refuse to
/// write a credential into.
///
/// The prior
/// implementation walked the literal ancestors and matched any `.git`
/// existence — three problems:
///
/// 1. **`$HOME` false-positive.** Many developers keep their home as a
///    git-managed dotfiles repo (`yadm`, `chezmoi --bare`, manual
///    `git init` in `~`). Walking ancestors hits `~/.git/` and bails
///    on every legitimate `~/.claude/settings.json` write. The override
///    env var was a setup-time discovery problem — users couldn't read
///    the error message until they hit it.
/// 2. **Symlink miss.** If `~/.claude` symlinks into a synced dotfiles
///    repo, the literal path walk MISSES the git tree — exactly the
///    footgun this check was supposed to catch.
/// 3. **Type imprecision.** `.git` can be a file (worktrees,
///    submodules) or a directory; the check accepted any kind.
///
/// New behavior:
/// - Canonicalize the path first so symlinked dotfiles get resolved.
/// - Require `.git` to be a directory or a file (matches real
///   worktree/submodule layout).
/// - If the matching ancestor IS `$HOME`, treat it as the dotfiles-
///   bare-repo case and ALLOW unless the user explicitly opted into
///   refusing it via `EREBYX_REFUSE_HOME_DOTFILES=1`.
pub(super) fn is_within_git_tree(path: &std::path::Path) -> bool {
    // Resolve symlinks. The synced-dotfiles footgun is precisely the
    // case where the literal path doesn't contain `.git` but the
    // resolved path does.
    let canonical = path
        .canonicalize()
        .or_else(|_| {
            // Path may not exist yet (we're about to write it).
            // Resolve the parent dir and re-attach the basename.
            let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
            let resolved_parent = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            let basename = path.file_name().unwrap_or_default();
            Ok::<std::path::PathBuf, std::io::Error>(resolved_parent.join(basename))
        })
        .unwrap_or_else(|_| path.to_path_buf());

    let home = dirs::home_dir();
    let refuse_home = env_flag_truthy("EREBYX_REFUSE_HOME_DOTFILES");

    for ancestor in canonical.ancestors() {
        let dot_git = ancestor.join(".git");
        if !dot_git.is_dir() && !dot_git.is_file() {
            continue;
        }
        // Special-case $HOME: a bare `git init` in $HOME is intentional
        // dotfiles workflow, not the synced-public-repo footgun.
        // Refuse only when the user explicitly asks.
        if let Some(h) = home.as_deref() {
            if h == ancestor {
                return refuse_home;
            }
        }
        return true;
    }
    false
}

/// True iff the env var is set to a truthiness allowlist value
/// (`"1" | "true" | "yes"`, case-insensitive, trimmed). Matches the
/// substrate's canonical pattern.
///
/// The prior `is_err()`-only check
/// treated `EREBYX_ALLOW_GIT_TREE_CONFIG=0` as a bypass — opposite of
/// what the user expects. This helper enforces the strict allowlist
/// across all CLI env-var toggles.
pub(super) fn env_flag_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Write JSON to a file with pretty formatting.
///
/// On Unix, sets 0600 (owner read/write only) so the embedded API key is
/// not world-readable. On Windows, file permissions inherit the parent
/// directory's ACL — typically user-profile-scoped but not guaranteed.
/// Emits a one-line warning (once per process) so the user knows the
/// difference.
///
/// Refuses to write inside a git working tree without an explicit
/// `EREBYX_ALLOW_GIT_TREE_CONFIG=1` override. Many users sync
/// `~/.claude/` and similar dotfiles to public repos; silently writing
/// an API key into those would leak credentials at next `git add .`.
fn write_json(path: &std::path::Path, value: &Value) -> Result<()> {
    // Refuse to write a credential-bearing config inside a git working
    // tree unless the user has explicitly opted in. The truthiness
    // check uses the {"1","true","yes"} allowlist so that
    // `EREBYX_ALLOW_GIT_TREE_CONFIG=0` does NOT bypass the guard.
    if is_within_git_tree(path) && !env_flag_truthy("EREBYX_ALLOW_GIT_TREE_CONFIG") {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Many users sync their config dirs to public repos; a \
             plaintext API key there would leak on the next commit. Set \
             EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move your \
             client config outside the tree.",
            path.display()
        );
    }

    let content = serde_json::to_string_pretty(value).context("Failed to serialize JSON")?;

    // Atomic + owner-only write. `atomic_write_secret` writes to a sibling
    // temp file, chmods it 0o600 on Unix BEFORE the rename, then atomically
    // renames over the target — so a crash mid-write can never truncate the
    // customer's whole client config, and the key-bearing file is never
    // world-readable even transiently.
    atomic_write_secret(path, content.as_bytes())?;

    // Windows DACL hardening is folded into `atomic_write_secret` (it re-tightens
    // at the write boundary — the atomic rename would otherwise revert any
    // caller-side DACL), so no caller-side `harden_windows_acl` is needed here.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    // -------------------------------------------------------------
    // env_flag_truthy — strict {"1","true","yes"} allowlist
    // -------------------------------------------------------------

    #[test]
    #[serial]
    fn env_flag_truthy_accepts_canonical_truthy_values() {
        for v in &["1", "true", "yes", "TRUE", "Yes", " true ", "YES"] {
            std::env::set_var("EREBYX_TEST_FLAG_TRUTHY", v);
            assert!(
                env_flag_truthy("EREBYX_TEST_FLAG_TRUTHY"),
                "value {v:?} must be truthy"
            );
        }
        std::env::remove_var("EREBYX_TEST_FLAG_TRUTHY");
    }

    #[test]
    #[serial]
    fn env_flag_truthy_rejects_zero_and_falsy_values() {
        // The P1-A regression. Pre-fix, `is_err()`-only treated these
        // as bypass since the var IS set (just to a falsy value).
        for v in &["0", "false", "no", "off", "", "FALSE", "garbage"] {
            std::env::set_var("EREBYX_TEST_FLAG_TRUTHY", v);
            assert!(
                !env_flag_truthy("EREBYX_TEST_FLAG_TRUTHY"),
                "value {v:?} must NOT be truthy"
            );
        }
        std::env::remove_var("EREBYX_TEST_FLAG_TRUTHY");
    }

    #[test]
    #[serial]
    fn env_flag_truthy_returns_false_when_unset() {
        std::env::remove_var("EREBYX_TEST_FLAG_TRUTHY");
        assert!(!env_flag_truthy("EREBYX_TEST_FLAG_TRUTHY"));
    }

    // -------------------------------------------------------------
    // is_within_git_tree — symlink, $HOME, type narrowing
    // -------------------------------------------------------------

    #[test]
    fn is_within_git_tree_returns_true_for_path_inside_git_dir() {
        let td = TempDir::new().expect("tempdir");
        let git_dir = td.path().join(".git");
        fs::create_dir(&git_dir).expect("mkdir .git");
        let target = td.path().join("subdir/settings.json");
        fs::create_dir_all(target.parent().unwrap()).expect("mkdir parent");

        assert!(is_within_git_tree(&target));
    }

    #[test]
    fn is_within_git_tree_returns_true_for_dotgit_as_file_worktree_pattern() {
        // git worktrees + submodules use `.git` as a FILE containing
        // `gitdir: ...` instead of a directory. Both forms must trigger.
        let td = TempDir::new().expect("tempdir");
        let git_file = td.path().join(".git");
        fs::write(&git_file, "gitdir: /elsewhere/.git/worktrees/foo\n").expect("write .git file");
        let target = td.path().join("settings.json");

        assert!(is_within_git_tree(&target));
    }

    #[test]
    fn is_within_git_tree_returns_false_for_path_outside_any_tree() {
        let td = TempDir::new().expect("tempdir");
        // No .git anywhere — clean tree.
        let target = td.path().join("subdir/settings.json");
        fs::create_dir_all(target.parent().unwrap()).expect("mkdir parent");

        assert!(!is_within_git_tree(&target));
    }

    #[test]
    fn is_within_git_tree_ignores_dotgit_named_file_that_isnt_real() {
        // An arbitrary file literally named ".git" with the wrong shape
        // still triggers per the documented contract (we don't parse
        // the file). The override env var is the customer escape hatch.
        // This test pins the documented behavior — change it together
        // with the docs if we ever want stricter detection.
        let td = TempDir::new().expect("tempdir");
        let weird_git = td.path().join(".git");
        fs::write(&weird_git, "this isn't a real git config\n").expect("write fake .git");
        let target = td.path().join("settings.json");

        assert!(is_within_git_tree(&target));
    }

    // -------------------------------------------------------------
    // write_json — git-tree refusal end-to-end + override truthiness
    // -------------------------------------------------------------

    #[test]
    #[serial]
    fn write_json_refuses_to_write_inside_git_tree_by_default() {
        let td = TempDir::new().expect("tempdir");
        fs::create_dir(td.path().join(".git")).expect("mkdir .git");
        let target = td.path().join("config.json");

        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let err = write_json(&target, &Value::String("test".into()))
            .expect_err("must refuse to write inside git tree without override");
        let msg = format!("{err}");
        assert!(
            msg.contains("Refusing to write"),
            "error message must explain refusal, got: {msg}"
        );
        assert!(!target.exists(), "file must NOT have been written");
    }

    #[test]
    #[serial]
    fn write_json_allows_write_when_override_is_truthy() {
        let td = TempDir::new().expect("tempdir");
        fs::create_dir(td.path().join(".git")).expect("mkdir .git");
        let target = td.path().join("config.json");

        std::env::set_var("EREBYX_ALLOW_GIT_TREE_CONFIG", "1");
        let result = write_json(&target, &Value::String("test".into()));
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");

        assert!(result.is_ok(), "override must permit the write: {result:?}");
        assert!(target.exists(), "file must have been written");
    }

    #[test]
    #[serial]
    fn write_json_refuses_when_override_is_falsy_zero() {
        // P1-A regression: `EREBYX_ALLOW_GIT_TREE_CONFIG=0` MUST NOT
        // bypass the guard. Pre-fix, `is_err()` treated it as a bypass.
        let td = TempDir::new().expect("tempdir");
        fs::create_dir(td.path().join(".git")).expect("mkdir .git");
        let target = td.path().join("config.json");

        std::env::set_var("EREBYX_ALLOW_GIT_TREE_CONFIG", "0");
        let err = write_json(&target, &Value::String("test".into()))
            .expect_err("EREBYX_ALLOW_GIT_TREE_CONFIG=0 MUST NOT bypass the guard");
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");

        assert!(format!("{err}").contains("Refusing to write"));
    }

    #[test]
    #[serial]
    fn write_json_allows_write_outside_git_tree() {
        let td = TempDir::new().expect("tempdir");
        // No .git anywhere.
        let target = td.path().join("config.json");

        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let result = write_json(&target, &Value::String("test".into()));

        assert!(result.is_ok(), "clean tree must permit write: {result:?}");
        assert!(target.exists(), "file must have been written");
    }

    // -------------------------------------------------------------
    // v0.1.3 — new-client writers (merge-preserve + schema shape)
    // -------------------------------------------------------------

    /// Build a throwaway `AiClient` pointing at a config path in `td`. The
    /// new-client writers all enforce the git-tree guard, so callers run
    /// under `#[serial]` and set `EREBYX_ALLOW_GIT_TREE_CONFIG` as needed —
    /// TempDir is typically outside any git tree, so writes succeed by default.
    fn test_client(kind: ClientKind, config_path: PathBuf) -> AiClient {
        AiClient {
            kind,
            name: "Test",
            rules_path: config_path.with_file_name("rules.md"),
            config_exists: false,
            home_dir: config_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default(),
            config_path,
        }
    }

    #[test]
    #[serial]
    fn grok_writer_merges_array_by_name_preserving_others() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let td = TempDir::new().expect("tempdir");
        let path = td.path().join("user-settings.json");
        // Pre-seed with an unrelated server + a stale erebyx-os element.
        fs::write(
            &path,
            r#"{ "theme": "dark", "mcp": { "servers": [
                { "id": "other", "transport": "stdio", "command": "x" },
                { "id": "erebyx-os", "command": "STALE" }
            ] } }"#,
        )
        .unwrap();

        let client = test_client(ClientKind::GrokCli, path.clone());
        write_grok_config(
            &client,
            "erebyx_testkey",
            "https://core.erebyx.com",
            "studio-ai",
        )
        .unwrap();

        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        // Unrelated top-level key preserved.
        assert_eq!(v.get("theme").and_then(|t| t.as_str()), Some("dark"));
        let servers = v["mcp"]["servers"].as_array().unwrap();
        // The "other" server survives; exactly ONE erebyx-os (stale replaced).
        assert!(servers.iter().any(|s| s["id"] == "other"), "other kept");
        let erebyx: Vec<_> = servers.iter().filter(|s| s["id"] == "erebyx-os").collect();
        assert_eq!(erebyx.len(), 1, "exactly one erebyx-os element");
        let e = erebyx[0];
        assert_eq!(e["transport"], "stdio");
        assert_eq!(e["enabled"], true);
        assert_eq!(e["label"], "erebyx-os");
        assert_eq!(e["args"][0], "mcp-serve");
        assert_eq!(e["env"]["EREBYX_API_KEY"], "erebyx_testkey");
        assert_eq!(e["env"]["EREBYX_API_URL"], "https://core.erebyx.com");
        assert_eq!(e["env"]["EREBYX_INSTANCE_ID"], "studio-ai");
        // STALE command must be gone.
        assert_ne!(e["command"], "STALE");
    }

    #[test]
    #[serial]
    fn grok_writer_creates_array_on_empty_file() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let td = TempDir::new().expect("tempdir");
        let path = td.path().join("user-settings.json");
        let client = test_client(ClientKind::GrokCli, path.clone());
        write_grok_config(&client, "k", "https://core.erebyx.com", "studio-ai").unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v["mcp"]["servers"].is_array());
        assert_eq!(v["mcp"]["servers"].as_array().unwrap().len(), 1);
    }

    #[test]
    #[serial]
    fn codex_writer_preserves_other_tables_and_is_idempotent() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let td = TempDir::new().expect("tempdir");
        let path = td.path().join("config.toml");
        // Pre-seed with an unrelated top-level setting + another mcp server.
        fs::write(
            &path,
            "model = \"o4-mini\"\n\n[mcp_servers.\"other\"]\ncommand = \"foo\"\nargs = [\"bar\"]\n",
        )
        .unwrap();

        let client = test_client(ClientKind::Codex, path.clone());
        write_codex_config(&client, "erebyx_k", "https://core.erebyx.com", "studio-ai").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // User content preserved.
        assert!(content.contains("model = \"o4-mini\""), "top-level kept");
        assert!(
            content.contains("[mcp_servers.\"other\"]"),
            "other server table kept"
        );
        // Our table + nested env present, with the right shape.
        assert!(content.contains("# EREBYX-START"));
        assert!(content.contains("[mcp_servers.\"erebyx-os\"]"));
        assert!(content.contains("args = [\"mcp-serve\"]"));
        assert!(content.contains("[mcp_servers.\"erebyx-os\".env]"));
        assert!(content.contains("EREBYX_API_KEY = \"erebyx_k\""));
        assert!(content.contains("EREBYX_API_URL = \"https://core.erebyx.com\""));
        assert!(content.contains("EREBYX_INSTANCE_ID = \"studio-ai\""));

        // Re-run: must replace the prior fenced block, not duplicate it.
        write_codex_config(&client, "erebyx_k2", "https://core.erebyx.com", "studio-ai").unwrap();
        let content2 = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content2.matches("# EREBYX-START").count(),
            1,
            "exactly one EREBYX fence after re-run"
        );
        assert!(content2.contains("erebyx_k2"), "new key present");
        assert!(!content2.contains("erebyx_k\""), "old key removed");
        assert!(
            content2.contains("[mcp_servers.\"other\"]"),
            "other still kept"
        );
    }

    #[test]
    #[serial]
    fn goose_writer_merges_under_extensions_preserving_others() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let td = TempDir::new().expect("tempdir");
        let path = td.path().join("config.yaml");
        // Pre-seed with a real extensions block containing another extension.
        fs::write(
            &path,
            "GOOSE_MODEL: gpt-4o\nextensions:\n  developer:\n    type: builtin\n    enabled: true\n",
        )
        .unwrap();

        let client = test_client(ClientKind::Goose, path.clone());
        write_goose_config(&client, "erebyx_k", "https://core.erebyx.com", "studio-ai").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // Other top-level + the developer extension preserved.
        assert!(content.contains("GOOSE_MODEL: gpt-4o"));
        assert!(content.contains("developer:"), "other extension kept");
        // Our entry uses `cmd` (NOT command) and `envs` (NOT env).
        assert!(content.contains("# EREBYX-START"));
        assert!(content.contains("erebyx-os:"));
        assert!(content.contains("type: stdio"));
        assert!(content.contains("cmd: "), "Goose uses cmd, not command");
        assert!(!content.contains("command: "), "must NOT emit `command:`");
        assert!(content.contains("envs:"), "Goose uses envs, not env");
        assert!(content.contains("EREBYX_API_KEY: \"erebyx_k\""));
        assert!(content.contains("EREBYX_INSTANCE_ID: \"studio-ai\""));
        assert!(content.contains("timeout: 300"));

        // Re-run idempotency.
        write_goose_config(&client, "erebyx_k2", "https://core.erebyx.com", "studio-ai").unwrap();
        let content2 = fs::read_to_string(&path).unwrap();
        assert_eq!(content2.matches("# EREBYX-START").count(), 1);
        assert!(
            content2.contains("developer:"),
            "other extension still kept"
        );
    }

    #[test]
    #[serial]
    fn goose_writer_creates_extensions_block_on_empty_file() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        let td = TempDir::new().expect("tempdir");
        let path = td.path().join("config.yaml");
        let client = test_client(ClientKind::Goose, path.clone());
        write_goose_config(&client, "k", "https://core.erebyx.com", "studio-ai").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("extensions:"));
        assert!(content.contains("erebyx-os:"));
    }

    #[test]
    #[serial]
    fn json_object_family_merges_preserving_existing_servers() {
        std::env::remove_var("EREBYX_ALLOW_GIT_TREE_CONFIG");
        // Claude Desktop / Gemini / Cline / Antigravity all route through
        // write_standard_mcp_config (top-level mcpServers object).
        for kind in [
            ClientKind::ClaudeDesktop,
            ClientKind::GeminiCli,
            ClientKind::Cline,
            ClientKind::Antigravity,
        ] {
            let td = TempDir::new().expect("tempdir");
            let path = td.path().join("cfg.json");
            fs::write(
                &path,
                r#"{ "theme": "x", "mcpServers": { "existing": { "command": "keepme" } } }"#,
            )
            .unwrap();
            let client = test_client(kind.clone(), path.clone());
            write_standard_mcp_config(&client, "erebyx_k", "https://core.erebyx.com", "studio-ai")
                .unwrap();
            let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(v["theme"], "x", "{kind:?}: top-level preserved");
            assert_eq!(
                v["mcpServers"]["existing"]["command"], "keepme",
                "{kind:?}: existing server preserved"
            );
            assert_eq!(
                v["mcpServers"]["erebyx-os"]["args"][0], "mcp-serve",
                "{kind:?}: erebyx entry added"
            );
            assert_eq!(
                v["mcpServers"]["erebyx-os"]["env"]["EREBYX_API_URL"],
                "https://core.erebyx.com"
            );
            assert_eq!(
                v["mcpServers"]["erebyx-os"]["env"]["EREBYX_INSTANCE_ID"],
                "studio-ai"
            );
        }
    }

    /// The TOML / YAML string serializers must escape quotes + backslashes so a
    /// pathological key never produces a malformed config.
    #[test]
    fn toml_and_yaml_string_escapers_handle_quotes_and_backslashes() {
        assert_eq!(toml_basic_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(yaml_dq_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        // Control char (U+0001) in TOML must be escaped, never emitted raw.
        assert_eq!(toml_basic_string("a\u{1}b"), "\"a\\u0001b\"");
    }

    /// `preview_mcp_config` must render the same schema the writers produce
    /// (it's what the dry-run shows) — covers every kind.
    #[test]
    fn preview_renders_per_serializer_for_every_kind() {
        let kinds = [
            (ClientKind::ClaudeDesktop, "cfg.json"),
            (ClientKind::GeminiCli, "settings.json"),
            (ClientKind::Cline, "cline_mcp_settings.json"),
            (ClientKind::Antigravity, "mcp_config.json"),
            (ClientKind::Codex, "config.toml"),
            (ClientKind::GrokCli, "user-settings.json"),
            (ClientKind::Goose, "config.yaml"),
        ];
        for (kind, fname) in kinds {
            let client = test_client(kind.clone(), PathBuf::from("/tmp").join(fname));
            let s = preview_mcp_config(&client, "<KEY>", "https://core.erebyx.com", "studio-ai");
            assert!(s.contains("erebyx-os"), "{kind:?} preview names erebyx-os");
            assert!(
                s.contains("studio-ai"),
                "{kind:?} preview includes selected instance id"
            );
            assert!(
                s.contains("mcp-serve"),
                "{kind:?} preview has mcp-serve arg"
            );
            match kind {
                ClientKind::Codex => {
                    assert!(s.contains("[mcp_servers.\"erebyx-os\"]"));
                    assert!(s.contains("[mcp_servers.\"erebyx-os\".env]"));
                }
                ClientKind::Goose => {
                    assert!(s.contains("extensions:") && s.contains("cmd: "));
                    assert!(!s.contains("command: "));
                }
                ClientKind::GrokCli => {
                    let v: Value = serde_json::from_str(&s).unwrap();
                    assert!(v["mcp"]["servers"].is_array());
                    assert_eq!(v["mcp"]["servers"][0]["transport"], "stdio");
                }
                _ => {
                    let v: Value = serde_json::from_str(&s).unwrap();
                    assert!(v["mcpServers"]["erebyx-os"].is_object());
                }
            }
        }
    }
}
