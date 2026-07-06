// SPDX-License-Identifier: MIT OR Apache-2.0
//! AI client detection -- finds installed coding AI tools on the system.

use std::path::PathBuf;

/// Supported AI client types.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientKind {
    ClaudeCode,
    Cursor,
    Windsurf,
    Continue,
    Zed,
    VsCodeCopilot,
    // v0.1.3 — additional MCP clients.
    /// OpenAI Codex CLI — TOML config at `~/.codex/config.toml`.
    Codex,
    /// Gemini CLI — JSON `mcpServers` object at `~/.gemini/settings.json`.
    GeminiCli,
    /// Claude Desktop — JSON `mcpServers` object, platform-specific path.
    ClaudeDesktop,
    /// Cline (VS Code ext / CLI) — JSON `mcpServers` object in globalStorage.
    Cline,
    /// Google Antigravity IDE/CLI — JSON `mcpServers` at `~/.gemini/config/mcp_config.json`.
    Antigravity,
    /// Grok Build CLI (`@vibe-kit/grok-cli`) — JSON `mcp.servers` ARRAY at `~/.grok/user-settings.json`.
    GrokCli,
    /// Goose (Block) — YAML `extensions` map at `~/.config/goose/config.yaml`.
    Goose,
}

/// A detected AI client with its configuration paths.
#[derive(Debug, Clone)]
pub struct AiClient {
    pub kind: ClientKind,
    pub name: &'static str,
    /// Path to the MCP configuration file
    pub config_path: PathBuf,
    /// Path to the rules/instructions file
    pub rules_path: PathBuf,
    /// Whether MCP config already exists
    pub config_exists: bool,
    /// Home directory for this client
    pub home_dir: PathBuf,
}

/// Detect all installed AI clients on the system.
pub fn detect_clients() -> Vec<AiClient> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let mut clients = Vec::new();

    // Claude Code: ~/.claude/ directory
    let claude_dir = home.join(".claude");
    if claude_dir.is_dir() {
        let config_path = claude_dir.join("settings.json");
        clients.push(AiClient {
            kind: ClientKind::ClaudeCode,
            name: "Claude Code",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::ClaudeCode),
            config_path,
            rules_path: claude_dir.join("rules").join("erebyx-memory.md"),
            home_dir: claude_dir,
        });
    }

    // Cursor: ~/.cursor/ directory
    let cursor_dir = home.join(".cursor");
    if cursor_dir.is_dir() {
        let config_path = cursor_dir.join("mcp.json");
        clients.push(AiClient {
            kind: ClientKind::Cursor,
            name: "Cursor",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Cursor),
            config_path,
            rules_path: cursor_dir.join("rules").join("erebyx-memory.mdc"),
            home_dir: cursor_dir,
        });
    }

    // Windsurf: ~/.codeium/ directory or (macOS only) the installed app.
    //
    // CLI v0.1.2 fix [10]: the macOS app probe is now gated behind
    // `cfg!(target_os = "macos")` — `/Applications/Windsurf.app` can never
    // exist on Linux/Windows, so probing it there was a wasted stat that
    // could also false-detect on an exotic mount.
    let codeium_dir = home.join(".codeium");
    let windsurf_app_present =
        cfg!(target_os = "macos") && PathBuf::from("/Applications/Windsurf.app").exists();
    if codeium_dir.is_dir() || windsurf_app_present {
        let config_path = codeium_dir.join("windsurf").join("mcp_config.json");
        clients.push(AiClient {
            kind: ClientKind::Windsurf,
            // CLI v0.1.2 fix [10]: Windsurf's GLOBAL rules live at
            // ~/.codeium/windsurf/memories/global_rules.md. The previous
            // path (~/.windsurfrules) is a PROJECT-ROOT file, not a $HOME
            // global — writing there did nothing for the user's global
            // memory, a silent no-op. VERIFY against live Windsurf docs
            // (https://docs.windsurf.com/windsurf/cascade/memories) before
            // each publish — Codeium has relocated this path before.
            rules_path: codeium_dir
                .join("windsurf")
                .join("memories")
                .join("global_rules.md"),
            name: "Windsurf",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Windsurf),
            config_path,
            home_dir: codeium_dir,
        });
    }

    // Continue: ~/.continue/ directory.
    // Continue migrated from config.json (legacy) to config.yaml (current,
    // ~2026-Q1). The legacy file uses `experimental.mcpServers`; the new
    // YAML file uses top-level `mcpServers`. We prefer config.yaml if both
    // exist, write to whichever the user already has, and default to YAML
    // for fresh installs.
    let continue_dir = home.join(".continue");
    if continue_dir.is_dir() {
        let yaml_path = continue_dir.join("config.yaml");
        let json_path = continue_dir.join("config.json");
        let config_path = if yaml_path.exists() {
            yaml_path
        } else if json_path.exists() {
            json_path
        } else {
            // Fresh install — bias to the current YAML format.
            yaml_path
        };
        clients.push(AiClient {
            kind: ClientKind::Continue,
            name: "Continue",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Continue),
            config_path,
            rules_path: continue_dir.join("rules").join("erebyx-memory.md"),
            home_dir: continue_dir,
        });
    }

    // Zed: ~/.config/zed/ directory.
    // Rules path corrected 2026-05-27: Zed's prompt library lives at
    // ~/.config/zed/prompts/ (per https://zed.dev/docs/ai/configuration and
    // https://zed.dev/docs/extensions/context-servers). The earlier path
    // ~/.config/zed/rules/ was wishful-thinking — Zed wrote files there
    // and silently never read them. Affected users: any Zed install that
    // ran `erebyx setup` before this fix shipped.
    let zed_dir = home.join(".config").join("zed");
    if zed_dir.is_dir() {
        let config_path = zed_dir.join("settings.json");
        clients.push(AiClient {
            kind: ClientKind::Zed,
            name: "Zed",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Zed),
            config_path,
            rules_path: zed_dir.join("prompts").join("erebyx-memory.md"),
            home_dir: zed_dir,
        });
    }

    // VS Code / Copilot: platform-specific settings directory
    let vscode_dir = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
    } else if cfg!(target_os = "linux") {
        home.join(".config").join("Code").join("User")
    } else {
        // Windows
        home.join("AppData")
            .join("Roaming")
            .join("Code")
            .join("User")
    };
    if vscode_dir.is_dir() {
        let config_path = vscode_dir.join("settings.json");
        clients.push(AiClient {
            kind: ClientKind::VsCodeCopilot,
            name: "VS Code / Copilot",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::VsCodeCopilot),
            config_path,
            // Use absolute path under $HOME for copilot instructions
            rules_path: home.join(".github").join("copilot-instructions.md"),
            home_dir: vscode_dir,
        });
    }

    // ---------------------------------------------------------------
    // v0.1.3 — additional MCP clients
    // ---------------------------------------------------------------

    // Codex (OpenAI Codex CLI): ~/.codex/ directory, TOML config.
    // ~/.codex is hardcoded by Codex on every OS (XDG-agnostic).
    let codex_dir = home.join(".codex");
    if codex_dir.is_dir() {
        let config_path = codex_dir.join("config.toml");
        clients.push(AiClient {
            kind: ClientKind::Codex,
            name: "Codex (OpenAI Codex CLI)",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Codex),
            config_path,
            // Codex reads project AGENTS.md / ~/.codex/AGENTS.md for guidance.
            rules_path: codex_dir.join("AGENTS.md"),
            home_dir: codex_dir,
        });
    }

    // Gemini CLI: ~/.gemini/ directory, JSON settings.
    // NOTE: shared with Antigravity (also under ~/.gemini). Gemini CLI's
    // file is ~/.gemini/settings.json; Antigravity's is
    // ~/.gemini/config/mcp_config.json — distinct files, both merge-safe.
    let gemini_dir = home.join(".gemini");
    if gemini_dir.is_dir() {
        let config_path = gemini_dir.join("settings.json");
        clients.push(AiClient {
            kind: ClientKind::GeminiCli,
            name: "Gemini CLI",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::GeminiCli),
            config_path,
            rules_path: gemini_dir.join("GEMINI.md"),
            home_dir: gemini_dir.clone(),
        });

        // Antigravity (Google Antigravity IDE/CLI) shares ~/.gemini but uses
        // the dedicated shared config at ~/.gemini/config/mcp_config.json.
        // The per-app dirs (antigravity-ide/ antigravity-cli/) are auto-
        // generated from it, so we ONLY write the shared file. We probe the
        // shared config file OR either Antigravity app dir as the presence
        // signal so a fresh Antigravity install (config dir not yet created)
        // is still detected.
        let antigravity_config = gemini_dir.join("config").join("mcp_config.json");
        let antigravity_present = antigravity_config.exists()
            || gemini_dir.join("antigravity").is_dir()
            || gemini_dir.join("antigravity-ide").is_dir()
            || gemini_dir.join("antigravity-cli").is_dir()
            || (cfg!(target_os = "macos")
                && PathBuf::from("/Applications/Antigravity.app").exists());
        if antigravity_present {
            clients.push(AiClient {
                kind: ClientKind::Antigravity,
                name: "Antigravity",
                config_exists: has_erebyx_mcp_config(&antigravity_config, &ClientKind::Antigravity),
                config_path: antigravity_config,
                rules_path: gemini_dir.join("GEMINI.md"),
                home_dir: gemini_dir.clone(),
            });
        }
    }

    // Claude Desktop: platform-specific app-support directory.
    // macOS: ~/Library/Application Support/Claude/
    // Linux (community build): ~/.config/Claude/
    // Windows: %APPDATA%/Claude/
    let claude_desktop_dir = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Claude")
    } else if cfg!(target_os = "linux") {
        home.join(".config").join("Claude")
    } else {
        // Windows
        home.join("AppData").join("Roaming").join("Claude")
    };
    if claude_desktop_dir.is_dir() {
        let config_path = claude_desktop_dir.join("claude_desktop_config.json");
        clients.push(AiClient {
            kind: ClientKind::ClaudeDesktop,
            name: "Claude Desktop",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::ClaudeDesktop),
            config_path,
            // Claude Desktop has no per-tool rules file; co-locate a reference
            // doc next to its config so re-runs are idempotent and the user
            // can read what was installed.
            rules_path: claude_desktop_dir.join("erebyx-memory.md"),
            home_dir: claude_desktop_dir,
        });
    }

    // Cline (saoudrizwan.claude-dev VS Code extension): lives in the VS Code
    // globalStorage tree. The IDE-fork segment ("Code") is the stable build;
    // the extension-id folder is ALWAYS literally "saoudrizwan.claude-dev".
    let cline_base = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
    } else if cfg!(target_os = "linux") {
        home.join(".config").join("Code").join("User")
    } else {
        // Windows
        home.join("AppData")
            .join("Roaming")
            .join("Code")
            .join("User")
    };
    let cline_dir = cline_base
        .join("globalStorage")
        .join("saoudrizwan.claude-dev")
        .join("settings");
    if cline_dir.is_dir() {
        let config_path = cline_dir.join("cline_mcp_settings.json");
        clients.push(AiClient {
            kind: ClientKind::Cline,
            name: "Cline",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Cline),
            config_path,
            rules_path: cline_dir.join("erebyx-memory.md"),
            home_dir: cline_dir,
        });
    }

    // Grok Build CLI (@vibe-kit/grok-cli): ~/.grok/ directory, JSON
    // `mcp.servers` array.
    let grok_dir = home.join(".grok");
    if grok_dir.is_dir() {
        let config_path = grok_dir.join("user-settings.json");
        clients.push(AiClient {
            kind: ClientKind::GrokCli,
            name: "Grok Build CLI",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::GrokCli),
            config_path,
            rules_path: grok_dir.join("erebyx-memory.md"),
            home_dir: grok_dir,
        });
    }

    // Goose (Block "codename goose"): ~/.config/goose/ directory, YAML config.
    let goose_dir = home.join(".config").join("goose");
    if goose_dir.is_dir() {
        let config_path = goose_dir.join("config.yaml");
        clients.push(AiClient {
            kind: ClientKind::Goose,
            name: "Goose",
            config_exists: has_erebyx_mcp_config(&config_path, &ClientKind::Goose),
            config_path,
            rules_path: goose_dir.join("erebyx-memory.md"),
            home_dir: goose_dir,
        });
    }

    clients
}

/// Check if a config file already contains the erebyx MCP server entry.
///
/// CLI v0.1.2 fix [9]: the previous implementation did a bare
/// `content.contains("erebyx")` substring scan. ANY incidental mention of
/// "erebyx" anywhere in the file — a comment, an unrelated path, a stale
/// rules reference — made `config_exists` true, so `erebyx setup` skipped the
/// real install and `erebyx doctor` falsely reported "configured". This now
/// PARSES the file and checks the exact key path that THIS crate writes for
/// each client kind (see `src/setup/config.rs`):
///
/// - Claude Code / Cursor / Windsurf / Gemini CLI / Claude Desktop / Cline /
///   Antigravity: JSON `mcpServers["erebyx-os"]`
/// - Zed: JSON `context_servers["erebyx-os"]`
/// - VS Code / Copilot: JSON `mcp.servers["erebyx-os"]`
/// - Codex (`config.toml`): the `# EREBYX-START` marker, OR a real
///   `[mcp_servers."erebyx-os"]` table header
/// - Grok Build CLI (`user-settings.json`): a `mcp.servers[]` array element
///   whose `id`/`name`/`label` is `erebyx-os`
/// - Goose (`config.yaml`): the `# EREBYX-START` marker, OR an `erebyx-os:`
///   entry under a real top-level `extensions:` block
/// - Continue (`config.yaml`): the `# EREBYX-START` marker, OR an `erebyx-os:`
///   entry under a real top-level `mcpServers:` block
/// - Continue (`config.json`, legacy): JSON
///   `experimental.mcpServers["erebyx-os"]`
fn has_erebyx_mcp_config(path: &PathBuf, kind: &ClientKind) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };

    const SERVER_KEY: &str = "erebyx-os";

    match kind {
        // JSON-object `mcpServers` family — Claude Code, Cursor, Windsurf, and
        // the v0.1.3 additions (Gemini CLI, Claude Desktop, Cline, Antigravity).
        ClientKind::ClaudeCode
        | ClientKind::Cursor
        | ClientKind::Windsurf
        | ClientKind::GeminiCli
        | ClientKind::ClaudeDesktop
        | ClientKind::Cline
        | ClientKind::Antigravity => json_has_nested_key(&content, &["mcpServers", SERVER_KEY]),
        ClientKind::Zed => json_has_nested_key(&content, &["context_servers", SERVER_KEY]),
        ClientKind::VsCodeCopilot => json_has_nested_key(&content, &["mcp", "servers", SERVER_KEY]),
        // Codex (TOML): a `[mcp_servers.erebyx-os]` table header — the writer
        // wraps its block in `# EREBYX-START` markers too, so check either.
        ClientKind::Codex => codex_toml_has_erebyx(&content, SERVER_KEY),
        // Grok (JSON `mcp.servers` ARRAY): an element whose `id`/`name`/`label`
        // is the erebyx server key.
        ClientKind::GrokCli => grok_json_has_erebyx(&content, SERVER_KEY),
        // Goose (YAML `extensions` map): an `erebyx-os:` key under a real
        // top-level `extensions:` block, OR the writer's `# EREBYX-START` fence.
        ClientKind::Goose => goose_yaml_has_erebyx(&content, SERVER_KEY),
        ClientKind::Continue => {
            // Continue has two on-disk formats. Branch on extension so we
            // check the actual key path the writer uses for each.
            let is_yaml = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
                .unwrap_or(false);
            if is_yaml {
                continue_yaml_has_erebyx(&content, SERVER_KEY)
            } else {
                // Legacy JSON config: experimental.mcpServers["erebyx-os"].
                json_has_nested_key(&content, &["experimental", "mcpServers", SERVER_KEY])
            }
        }
    }
}

/// Parse `content` as JSON and return true iff the nested object path
/// `keys` resolves to a present value (objects descended key-by-key).
/// Non-JSON or a missing key → false (never a substring guess).
fn json_has_nested_key(content: &str, keys: &[&str]) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let mut cursor = &value;
    for key in keys {
        match cursor.get(key) {
            Some(next) => cursor = next,
            None => return false,
        }
    }
    true
}

/// Detect the erebyx entry inside a Continue `config.yaml`.
///
/// We avoid pulling in a YAML parser (no `serde_yaml` dependency in this
/// crate) and instead check the two structural signals the writer in
/// `config.rs` actually produces:
///   1. The `# EREBYX-START` fence the writer wraps its block in.
///   2. An `erebyx-os:` map key indented under a real (non-commented)
///      top-level `mcpServers:` block.
fn continue_yaml_has_erebyx(content: &str, server_key: &str) -> bool {
    if content.contains("# EREBYX-START") {
        return true;
    }
    // Find a real top-level `mcpServers:` line (column 0, not commented),
    // then look for an `erebyx-os:` map key in the following indented block.
    let mut in_mcp_servers = false;
    for line in content.lines() {
        let trimmed_start = line.trim_start();
        let indent = line.len() - trimmed_start.len();
        if indent == 0 {
            // A new top-level key ends any previous block.
            in_mcp_servers = trimmed_start.starts_with("mcpServers:");
            continue;
        }
        if in_mcp_servers {
            let key = trimmed_start.trim_end();
            if key == format!("{server_key}:") || key.starts_with(&format!("{server_key}:")) {
                return true;
            }
        }
    }
    false
}

/// Detect the erebyx entry inside a Codex `config.toml`.
///
/// No TOML parser is pulled into this crate (the writer hand-serializes a
/// `[mcp_servers.erebyx-os]` table fenced in `# EREBYX-START`/`# EREBYX-END`
/// markers). We check the two structural signals the writer produces:
///   1. The `# EREBYX-START` fence.
///   2. A real (column-0, non-commented) `[mcp_servers.erebyx-os]` table
///      header. TOML allows bare or quoted keys; the writer always emits the
///      quoted form `"erebyx-os"` because the key contains a hyphen.
fn codex_toml_has_erebyx(content: &str, server_key: &str) -> bool {
    if content.contains("# EREBYX-START") {
        return true;
    }
    // Accept both quoted and bare table-header spellings, tolerant of inner
    // whitespace: `[mcp_servers."erebyx-os"]` and `[mcp_servers.erebyx-os]`.
    for line in content.lines() {
        let trimmed = line.trim_start();
        // Skip commented lines.
        if trimmed.starts_with('#') {
            continue;
        }
        // Only consider real table headers at column 0.
        if line.len() == trimmed.len() && trimmed.starts_with("[mcp_servers.") {
            let inner: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
            let quoted = format!("[mcp_servers.\"{server_key}\"]");
            let quoted_env = format!("[mcp_servers.\"{server_key}\".env]");
            let bare = format!("[mcp_servers.{server_key}]");
            let bare_env = format!("[mcp_servers.{server_key}.env]");
            if inner == quoted || inner == quoted_env || inner == bare || inner == bare_env {
                return true;
            }
        }
    }
    false
}

/// Detect the erebyx entry inside a Grok CLI `user-settings.json`.
///
/// Grok stores MCP servers as a JSON ARRAY at `mcp.servers`. Each element is
/// an object with `id` / `label` / `transport` fields. We parse and look for
/// an element whose `id`, `name`, or `label` equals the server key.
fn grok_json_has_erebyx(content: &str, server_key: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let Some(servers) = value
        .get("mcp")
        .and_then(|m| m.get("servers"))
        .and_then(|s| s.as_array())
    else {
        return false;
    };
    servers.iter().any(|el| {
        ["id", "name", "label"].iter().any(|k| {
            el.get(*k)
                .and_then(|v| v.as_str())
                .map(|s| s == server_key)
                .unwrap_or(false)
        })
    })
}

/// Detect the erebyx entry inside a Goose `config.yaml`.
///
/// Goose stores MCP servers under a top-level `extensions:` map (NOT
/// `mcpServers`). We avoid a YAML dep and check the writer's structural
/// signals: the `# EREBYX-START` fence, OR an `erebyx-os:` key indented under
/// a real (column-0, non-commented) `extensions:` block.
fn goose_yaml_has_erebyx(content: &str, server_key: &str) -> bool {
    if content.contains("# EREBYX-START") {
        return true;
    }
    let mut in_extensions = false;
    for line in content.lines() {
        let trimmed_start = line.trim_start();
        let indent = line.len() - trimmed_start.len();
        if indent == 0 {
            in_extensions = trimmed_start.starts_with("extensions:");
            continue;
        }
        if in_extensions && !trimmed_start.starts_with('#') {
            let key = trimmed_start.trim_end();
            if key == format!("{server_key}:") || key.starts_with(&format!("{server_key}:")) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod detect_tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DETECT_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Write `content` to a temp file with the given extension and run
    /// `has_erebyx_mcp_config` against it for `kind`.
    fn detect_in(content: &str, ext: &str, kind: &ClientKind) -> bool {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "erebyx-detect-test-{}-{}-{}.{}",
            std::process::id(),
            // cheap unique-ish suffix
            content.len(),
            DETECT_TEST_COUNTER.fetch_add(1, Ordering::Relaxed),
            ext
        ));
        {
            let mut f = std::fs::File::create(&path).expect("create temp config");
            f.write_all(content.as_bytes()).expect("write temp config");
        }
        let result = has_erebyx_mcp_config(&path, kind);
        let _ = std::fs::remove_file(&path);
        result
    }

    /// CLI v0.1.2 fix [9]: a bare mention of "erebyx" (a comment, a stale
    /// path) must NOT count as a configured client — the prior substring
    /// scan did, which made setup skip the real install.
    #[test]
    fn substring_mention_is_not_a_real_config() {
        let json = r#"{
            "mcpServers": {
                "some-other-server": { "command": "/usr/bin/erebyx-helper" }
            },
            "_comment": "remember to wire up erebyx later"
        }"#;
        assert!(!detect_in(json, "json", &ClientKind::ClaudeCode));
        assert!(!detect_in(json, "json", &ClientKind::Cursor));
    }

    #[test]
    fn detects_real_mcp_servers_entry() {
        let json = r#"{ "mcpServers": { "erebyx-os": { "command": "erebyx" } } }"#;
        assert!(detect_in(json, "json", &ClientKind::ClaudeCode));
        assert!(detect_in(json, "json", &ClientKind::Cursor));
        assert!(detect_in(json, "json", &ClientKind::Windsurf));
    }

    #[test]
    fn zed_uses_context_servers_key_not_mcp_servers() {
        let zed = r#"{ "context_servers": { "erebyx-os": { "command": {} } } }"#;
        assert!(detect_in(zed, "json", &ClientKind::Zed));
        // The same file under the mcpServers key must NOT count for Zed.
        let wrong = r#"{ "mcpServers": { "erebyx-os": {} } }"#;
        assert!(!detect_in(wrong, "json", &ClientKind::Zed));
    }

    #[test]
    fn vscode_uses_mcp_servers_nested_key() {
        let vscode = r#"{ "mcp": { "servers": { "erebyx-os": { "type": "stdio" } } } }"#;
        assert!(detect_in(vscode, "json", &ClientKind::VsCodeCopilot));
        let wrong = r#"{ "mcpServers": { "erebyx-os": {} } }"#;
        assert!(!detect_in(wrong, "json", &ClientKind::VsCodeCopilot));
    }

    #[test]
    fn continue_yaml_marker_detected() {
        let yaml =
            "mcpServers:\n  # EREBYX-START\n  erebyx-os:\n    command: erebyx\n  # EREBYX-END\n";
        assert!(detect_in(yaml, "yaml", &ClientKind::Continue));
    }

    #[test]
    fn continue_yaml_real_block_without_marker() {
        let yaml = "mcpServers:\n  erebyx-os:\n    command: erebyx\n";
        assert!(detect_in(yaml, "yaml", &ClientKind::Continue));
    }

    #[test]
    fn continue_yaml_commented_block_is_not_real() {
        // A commented-out mcpServers block must not register as configured.
        let yaml = "# mcpServers:\n#   erebyx-os:\n#     command: erebyx\n";
        assert!(!detect_in(yaml, "yaml", &ClientKind::Continue));
    }

    #[test]
    fn continue_legacy_json_experimental_key() {
        let json = r#"{ "experimental": { "mcpServers": { "erebyx-os": {} } } }"#;
        assert!(detect_in(json, "json", &ClientKind::Continue));
        // Top-level mcpServers in the JSON form is the wrong key for legacy.
        let wrong = r#"{ "mcpServers": { "erebyx-os": {} } }"#;
        assert!(!detect_in(wrong, "json", &ClientKind::Continue));
    }

    #[test]
    fn malformed_json_is_not_a_false_positive() {
        // Garbage that merely contains "erebyx-os" but isn't valid JSON.
        let junk = "this is not json but mentions erebyx-os somewhere";
        assert!(!detect_in(junk, "json", &ClientKind::ClaudeCode));
    }

    // -------------------------------------------------------------
    // v0.1.3 — additional MCP clients
    // -------------------------------------------------------------

    #[test]
    fn json_object_family_detected_for_new_clients() {
        // Gemini CLI / Claude Desktop / Cline / Antigravity all share the
        // top-level `mcpServers` object key.
        let json = r#"{ "mcpServers": { "erebyx-os": { "command": "erebyx" } } }"#;
        for kind in &[
            ClientKind::GeminiCli,
            ClientKind::ClaudeDesktop,
            ClientKind::Cline,
            ClientKind::Antigravity,
        ] {
            assert!(detect_in(json, "json", kind), "{kind:?} should detect");
        }
        // The Zed/VsCode/Grok key shapes must NOT count for the object family.
        let wrong = r#"{ "context_servers": { "erebyx-os": {} } }"#;
        assert!(!detect_in(wrong, "json", &ClientKind::GeminiCli));
    }

    #[test]
    fn codex_toml_marker_detected() {
        let toml = "[mcp_servers.other]\ncommand = \"x\"\n\n# EREBYX-START\n[mcp_servers.\"erebyx-os\"]\ncommand = \"erebyx\"\n# EREBYX-END\n";
        assert!(detect_in(toml, "toml", &ClientKind::Codex));
    }

    #[test]
    fn codex_toml_real_table_without_marker() {
        // Quoted form (writer emits this — hyphen in key).
        let toml = "[mcp_servers.\"erebyx-os\"]\ncommand = \"erebyx\"\n";
        assert!(detect_in(toml, "toml", &ClientKind::Codex));
        // Bare form should also be recognized.
        let bare = "[mcp_servers.erebyx-os]\ncommand = \"erebyx\"\n";
        assert!(detect_in(bare, "toml", &ClientKind::Codex));
    }

    #[test]
    fn codex_toml_commented_table_is_not_real() {
        let toml = "# [mcp_servers.\"erebyx-os\"]\n# command = \"erebyx\"\n";
        assert!(!detect_in(toml, "toml", &ClientKind::Codex));
        // An unrelated server must not false-positive.
        let other = "[mcp_servers.\"something-else\"]\ncommand = \"x\"\n";
        assert!(!detect_in(other, "toml", &ClientKind::Codex));
    }

    #[test]
    fn grok_json_array_detected() {
        let json = r#"{ "mcp": { "servers": [ { "id": "other", "transport": "stdio" }, { "id": "erebyx-os", "label": "erebyx", "transport": "stdio" } ] } }"#;
        assert!(detect_in(json, "json", &ClientKind::GrokCli));
    }

    #[test]
    fn grok_json_top_level_mcpservers_is_wrong_key() {
        // Grok uses `mcp.servers`, NOT a top-level `mcpServers` object. A
        // top-level object must not register for Grok.
        let wrong = r#"{ "mcpServers": { "erebyx-os": {} } }"#;
        assert!(!detect_in(wrong, "json", &ClientKind::GrokCli));
        // An array with no matching element must not register.
        let no_match = r#"{ "mcp": { "servers": [ { "id": "other" } ] } }"#;
        assert!(!detect_in(no_match, "json", &ClientKind::GrokCli));
    }

    #[test]
    fn goose_yaml_marker_detected() {
        let yaml =
            "extensions:\n  # EREBYX-START\n  erebyx-os:\n    type: stdio\n    cmd: erebyx\n  # EREBYX-END\n";
        assert!(detect_in(yaml, "yaml", &ClientKind::Goose));
    }

    #[test]
    fn goose_yaml_real_block_without_marker() {
        let yaml = "extensions:\n  erebyx-os:\n    type: stdio\n    cmd: erebyx\n";
        assert!(detect_in(yaml, "yaml", &ClientKind::Goose));
    }

    #[test]
    fn goose_yaml_commented_block_is_not_real() {
        let yaml = "# extensions:\n#   erebyx-os:\n#     cmd: erebyx\n";
        assert!(!detect_in(yaml, "yaml", &ClientKind::Goose));
        // Goose uses `extensions:`, not `mcpServers:` — wrong block doesn't count.
        let wrong = "mcpServers:\n  erebyx-os:\n    command: erebyx\n";
        assert!(!detect_in(wrong, "yaml", &ClientKind::Goose));
    }
}
