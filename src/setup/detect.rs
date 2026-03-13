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
            config_exists: has_erebyx_mcp_config(&config_path),
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
            config_exists: has_erebyx_mcp_config(&config_path),
            config_path,
            rules_path: cursor_dir.join("rules").join("erebyx-memory.mdc"),
            home_dir: cursor_dir,
        });
    }

    // Windsurf: ~/.codeium/ directory or Windsurf.app
    let codeium_dir = home.join(".codeium");
    let windsurf_app = PathBuf::from("/Applications/Windsurf.app");
    if codeium_dir.is_dir() || windsurf_app.exists() {
        let config_path = codeium_dir.join("windsurf").join("mcp_config.json");
        clients.push(AiClient {
            kind: ClientKind::Windsurf,
            name: "Windsurf",
            config_exists: has_erebyx_mcp_config(&config_path),
            config_path,
            rules_path: home.join(".windsurfrules"),
            home_dir: codeium_dir,
        });
    }

    // Continue: ~/.continue/ directory
    let continue_dir = home.join(".continue");
    if continue_dir.is_dir() {
        let config_path = continue_dir.join("config.json");
        clients.push(AiClient {
            kind: ClientKind::Continue,
            name: "Continue",
            config_exists: has_erebyx_mcp_config(&config_path),
            config_path,
            rules_path: continue_dir.join("rules").join("erebyx-memory.md"),
            home_dir: continue_dir,
        });
    }

    // Zed: ~/.config/zed/ directory
    let zed_dir = home.join(".config").join("zed");
    if zed_dir.is_dir() {
        let config_path = zed_dir.join("settings.json");
        clients.push(AiClient {
            kind: ClientKind::Zed,
            name: "Zed",
            config_exists: has_erebyx_mcp_config(&config_path),
            config_path,
            rules_path: zed_dir.join("rules").join("erebyx-memory.md"),
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
            config_exists: has_erebyx_mcp_config(&config_path),
            config_path,
            // Use absolute path under $HOME for copilot instructions
            rules_path: home.join(".github").join("copilot-instructions.md"),
            home_dir: vscode_dir,
        });
    }

    clients
}

/// Check if a config file already contains erebyx MCP configuration.
fn has_erebyx_mcp_config(path: &PathBuf) -> bool {
    if !path.exists() {
        return false;
    }

    match std::fs::read_to_string(path) {
        Ok(content) => content.contains("erebyx"),
        Err(_) => false,
    }
}
