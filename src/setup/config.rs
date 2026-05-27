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
        .insert(
            "erebyx-os".to_string(),
            erebyx_server_entry(api_key, api_url),
        );

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
        .insert(
            "erebyx-os".to_string(),
            erebyx_server_entry(api_key, api_url),
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
fn write_continue_config(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    let is_yaml = client
        .config_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
        .unwrap_or(false);

    if is_yaml {
        write_continue_yaml(client, api_key, api_url)
    } else {
        write_continue_json_legacy(client, api_key, api_url)
    }
}

/// Continue YAML — current format. Top-level `mcpServers`, no nesting.
fn write_continue_yaml(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
    // P0 (brutal-review wave-2 2026-05-27 CLI lane): apply the SAME
    // git-tree credential guard the JSON path enforces. Continue users
    // frequently sync `~/.continue/` to public dotfiles repos; without
    // this guard, the YAML writer would silently leak the API key on
    // the next commit. Same allowlisted override (`EREBYX_ALLOW_GIT_TREE_CONFIG`).
    if is_within_git_tree(&client.config_path)
        && !env_flag_truthy("EREBYX_ALLOW_GIT_TREE_CONFIG")
    {
        anyhow::bail!(
            "Refusing to write API key to {} — path is inside a git working \
             tree. Set EREBYX_ALLOW_GIT_TREE_CONFIG=1 to override, or move \
             your Continue config outside the tree.",
            client.config_path.display()
        );
    }

    // P0 (brutal-review wave-2): YAML-escape api_key + api_url before
    // raw interpolation into the double-quoted YAML string. Real Erebyx
    // keys are alphanumeric (`erebyx_<48 hex>`), but the contract isn't
    // validated upstream — a key with `"` or `\` would silently produce
    // invalid YAML. Use the YAML double-quoted-flow-scalar escape rules:
    // backslash and double-quote each get a backslash prefix.
    let api_key_escaped = api_key.replace('\\', "\\\\").replace('"', "\\\"");
    let api_url_escaped = api_url.replace('\\', "\\\\").replace('"', "\\\"");

    // Read existing YAML if any. Continue's YAML is straightforward; we
    // do a string-level merge rather than depend on a yaml crate, to keep
    // the CLI dep footprint small and avoid reformatting user content.
    let existing = std::fs::read_to_string(&client.config_path).unwrap_or_default();
    let cleaned = strip_existing_erebyx_yaml_block(&existing);

    let entry = format!(
        "  erebyx-os:\n    command: {cmd}\n    args:\n      - mcp-serve\n    env:\n      EREBYX_API_KEY: \"{key}\"\n      EREBYX_API_URL: \"{url}\"\n      EREBYX_INSTANCE_ID: \"default\"\n",
        cmd = erebyx_command(),
        key = api_key_escaped,
        url = api_url_escaped,
    );

    // If a top-level `mcpServers:` block exists, append our entry under
    // it. Otherwise, add the block at the end.
    //
    // P1 (brutal-review wave-2): the `\nmcpServers:` heuristic matches
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
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    std::fs::write(&client.config_path, new_content)
        .with_context(|| format!("Failed to write {}", client.config_path.display()))?;

    // P0 (brutal-review wave-2): apply 0600 perms on Unix — the JSON
    // writer does this via `write_json`; YAML writer was silently
    // shipping world-readable. Real credential-leak surface for users
    // on shared hosts.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&client.config_path, perms).with_context(|| {
            format!(
                "Failed to set permissions on {}",
                client.config_path.display()
            )
        })?;
    }

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
fn write_continue_json_legacy(client: &AiClient, api_key: &str, api_url: &str) -> Result<PathBuf> {
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
            erebyx_server_entry(api_key, api_url),
        );

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
    servers.as_object_mut().expect("validated above").insert(
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

/// True if `path` lives inside a git working tree we should refuse to
/// write a credential into.
///
/// **Brutal-review POSTFIX_CLI P0-A (2026-05-27):** the prior
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
fn is_within_git_tree(path: &std::path::Path) -> bool {
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
/// **Brutal-review POSTFIX_CLI P1-A:** the prior `is_err()`-only check
/// treated `EREBYX_ALLOW_GIT_TREE_CONFIG=0` as a bypass — opposite of
/// what the user expects. This helper enforces the strict allowlist
/// across all CLI env-var toggles.
fn env_flag_truthy(name: &str) -> bool {
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
fn write_json(path: &PathBuf, value: &Value) -> Result<()> {
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
    // tighten permissions out-of-band. The file lands at %USERPROFILE%
    // by default, which is already user-profile-scoped on a single-user
    // box — the risk is multi-user Windows hosts and roaming profiles.
    // A future v0.1.2 will wire `windows-acl` to close this.
    //
    // P1-B (brutal-review POSTFIX_CLI): warn once per process, not once
    // per write_json call. `erebyx setup` typically writes one config
    // per detected client (3-6 files on a developer box); a single
    // warning per `setup` invocation is enough signal.
    #[cfg(windows)]
    {
        use std::sync::Once;
        static WINDOWS_ACL_WARN: Once = Once::new();
        WINDOWS_ACL_WARN.call_once(|| {
            eprintln!(
                "  ⚠ Windows: file permissions cannot be auto-restricted on this platform.\n\
                 \n\
                 Confirm %USERPROFILE% is not world-readable. To tighten ACLs on each\n\
                 written config, run:\n\
                 \n\
                     icacls \"<path>\" /inheritance:r ^\n\
                         /grant:r \"%USERNAME%:F\" \"SYSTEM:F\" \"Administrators:F\"\n\
                 \n\
                 See SECURITY.md → \"API-key file handling\" for the full guidance."
            );
        });
    }

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
}
