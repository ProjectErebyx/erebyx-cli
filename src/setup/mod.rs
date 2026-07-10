// SPDX-License-Identifier: MIT OR Apache-2.0
//! `erebyx setup` — one-command memory installation for all AI coding clients.
//!
//! Detects installed clients (Claude Code, Cursor, Windsurf, Continue, Zed,
//! Copilot, Codex, Gemini CLI, Claude Desktop, Cline, Antigravity, Grok Build
//! CLI, Goose), writes MCP server config for each, injects rules files, and
//! installs hooks. Also prints remote-connector guidance (ChatGPT, Grok chat
//! app, JetBrains AI) for clients with no local config file.

pub mod config;
pub mod detect;
pub mod hooks;
pub mod rules;

use anyhow::Result;
use colored::Colorize;
use dialoguer::{Confirm, Password};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::{json, Value};
use std::path::PathBuf;

use detect::{detect_clients, AiClient};

use crate::client::is_safe_url;
use crate::credentials::{self, StoredCredentials};

/// Shared error message for an unsafe `EREBYX_API_URL` — matches the
/// `ErebyxClient::new` guard wording so the operator sees one consistent
/// contract no matter which entry point rejects (P1-4).
const UNSAFE_URL_MSG: &str = "EREBYX_API_URL must be https:// (got {URL}). \
     Plain http:// is only allowed for localhost/127.0.0.1.";

/// Validate an `api_url` with the canonical `is_safe_url` guard, returning a
/// uniform `anyhow` error on rejection. Centralizes the HTTPS-or-localhost
/// enforcement across `run_setup` / `run_setup_dry_run` so the bearer token
/// is never POSTed over plain http:// to an arbitrary host.
fn ensure_safe_api_url(api_url: &str) -> Result<()> {
    if !is_safe_url(api_url) {
        anyhow::bail!(UNSAFE_URL_MSG.replace("{URL}", api_url));
    }
    Ok(())
}

/// Fetch the substrate's `restore_identity` + `load_context` summary
/// payloads and render a compact pre-injection text suitable for the
/// dynamic block of every detected client's rules file.
///
/// Mirrors `render_session_start_injection` in `main.rs` so the
/// always-loaded rules-file content (model-agnostic) matches what the
/// native SessionStart hook (Claude-Code-only fast path) emits.
///
/// **Fail-open everywhere**: returns an empty string on any error.
/// Setup never refuses to complete because a single API call failed.
async fn fetch_dynamic_context(credentials: &config::SetupCredentials) -> String {
    // P1-4: never POST the bearer token over an unsafe URL. `fetch_dynamic_context`
    // is fail-open by contract (returns "" on any error), so a bad URL yields an
    // empty dynamic block rather than leaking credentials to an arbitrary host.
    // In normal flow `run_setup` already rejected an unsafe URL before reaching
    // here; this is the in-function fence the finding calls for.
    if !is_safe_url(&credentials.api_url) {
        return String::new();
    }
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let base = credentials.api_url.trim_end_matches('/');
    // Setup-time session id — informational only; setup isn't a long-
    // lived session, so a simple per-install token is fine. Format
    // matches what the substrate expects (opaque string).
    let session = format!(
        "setup-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    // Cap response bodies at 10 MiB
    // before deserialization. The sibling `run_hook_inject` already does
    // this; the dynamic-context path was added later and shipped without.
    // A malicious or buggy substrate response would OOM a small setup
    // machine — and setup runs unattended on user workstations.
    const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

    async fn capped_json(resp: reqwest::Response) -> Option<Value> {
        if !resp.status().is_success() {
            return None;
        }
        if let Some(len) = resp.content_length() {
            if len > MAX_RESPONSE_BYTES {
                return None;
            }
        }
        resp.json().await.ok()
    }

    // identity
    let mut identity_req = http
        .post(format!("{}/v0/identity/restore", base))
        .bearer_auth(&credentials.api_key)
        .header("Content-Type", "application/json")
        .header("X-Instance-ID", &credentials.instance_id)
        .header("X-Erebyx-Session-Id", &session)
        .json(&json!({"detail_level": "summary", "limit": 5}));
    if let Some(passphrase) = credentials.passphrase.as_deref() {
        identity_req = identity_req.header("X-Passphrase", passphrase);
    }
    let identity: Option<Value> = match identity_req.send().await {
        Ok(r) => capped_json(r).await,
        Err(_) => None,
    };

    // context
    let mut context_req = http
        .post(format!("{}/v0/session/load", base))
        .bearer_auth(&credentials.api_key)
        .header("Content-Type", "application/json")
        .header("X-Instance-ID", &credentials.instance_id)
        .header("X-Erebyx-Session-Id", &session)
        .json(&json!({"anchors": [], "detail_level": "summary"}));
    if let Some(passphrase) = credentials.passphrase.as_deref() {
        context_req = context_req.header("X-Passphrase", passphrase);
    }
    let context: Option<Value> = match context_req.send().await {
        Ok(r) => capped_json(r).await,
        Err(_) => None,
    };

    render_dynamic_block(identity.as_ref(), context.as_ref())
}

/// Render the dynamic-block content from substrate identity + context
/// payloads. Declarative phrasing throughout — matches
/// `rules_content_uses_declarative_not_imperative_framing` doctrine.
fn render_dynamic_block(identity: Option<&Value>, context: Option<&Value>) -> String {
    let mut lines = Vec::new();
    lines.push("# Current substrate state (refreshed at install time)".to_string());
    let mut chars = lines[0].len();
    const MAX_CHARS: usize = 3200;

    if let Some(id) = identity {
        let name = id
            .get("identity")
            .and_then(|i| i.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if !name.is_empty() {
            let line = format!("\nStored identity: {}", name);
            chars += line.len();
            lines.push(line);
        }
        let ethos = id
            .get("ethos")
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        for statement in ethos.iter().take(3) {
            let trimmed: String = statement.chars().take(180).collect();
            let line = format!("- ethos: {}", trimmed);
            if chars + line.len() < MAX_CHARS {
                chars += line.len();
                lines.push(line);
            }
        }
    }

    if let Some(ctx) = context {
        let handoff = ctx.get("handoff").or_else(|| ctx.get("continuity"));
        if let Some(h) = handoff {
            let what = h
                .get("what_we_built")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let next = h.get("whats_next").and_then(|v| v.as_str()).unwrap_or("");
            if !what.is_empty() || !next.is_empty() {
                lines.push("\nLast session handoff:".to_string());
                if !what.is_empty() {
                    let snippet: String = what.chars().take(600).collect();
                    let line = format!("- built: {}", snippet);
                    if chars + line.len() < MAX_CHARS {
                        chars += line.len();
                        lines.push(line);
                    }
                }
                if !next.is_empty() {
                    let snippet: String = next.chars().take(600).collect();
                    let line = format!("- next:  {}", snippet);
                    if chars + line.len() < MAX_CHARS {
                        chars += line.len();
                        lines.push(line);
                    }
                }
            }
        }
        let anchors = ctx
            .get("anchors")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        if !anchors.is_empty() {
            let joined = anchors
                .iter()
                .take(5)
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            let line = format!("Recent anchors: {}", joined);
            if chars + line.len() < MAX_CHARS {
                lines.push(line);
            }
        }
    }

    // Only the header? Nothing useful — return empty.
    if lines.len() <= 1 {
        return String::new();
    }
    lines.join("\n")
}

/// Run the interactive setup flow.
/// Preview `erebyx setup` without writing anything to disk.
///
/// Stripe CLI / Vercel CLI / Fly Launch convention: let the operator
/// audit what setup will do before it does it. Especially valuable for
/// users running the CLI against a corporate workstation, a shared
/// machine, or any environment where they need to know exactly which
/// files an installer will touch.
///
/// Output sections:
///   1. Detected clients — same as the real setup
///   2. Files that WOULD be written — full list of (label, path) pairs
///   3. Configs that WOULD be merged — per-client JSON snippet preview
///   4. Hooks that WOULD be installed — Claude Code SessionStart +
///      UserPromptSubmit
///   5. What WON'T change — anything detected but not touched
///
/// **No HTTP calls.** Dry-run intentionally skips the dynamic-block
/// fetch + auth probe so it can run offline against any env. The
/// `EREBYX_API_KEY` prompt is also skipped — paths are shown using
/// a placeholder. Run the real setup for credentials + dynamic context.
pub async fn run_setup_dry_run(_api_key: Option<String>, api_url: Option<String>) -> Result<()> {
    let api_url = api_url.unwrap_or_else(|| "https://core.erebyx.com".to_string());

    // P1-4: reject an unsafe URL up-front. Dry-run makes no HTTP calls, but it
    // PREVIEWS the hook script + config that the real run would write with this
    // URL — so it must enforce the same HTTPS-or-localhost contract or it would
    // greenlight a setup the real run rejects.
    ensure_safe_api_url(&api_url)?;

    let placeholder_key = "<YOUR_EREBYX_API_KEY>";
    let placeholder_credentials = config::SetupCredentials {
        api_key: placeholder_key.to_string(),
        api_url: api_url.clone(),
        instance_id: "<YOUR_EREBYX_INSTANCE_ID>".to_string(),
        passphrase: Some("<YOUR_EREBYX_PASSPHRASE>".to_string()),
    };

    println!();
    println!(
        "  {}",
        "EREBYX setup — DRY RUN (no files will be written)"
            .bold()
            .yellow()
    );
    println!();

    // Detect clients (same code path as real setup)
    let clients = detect_clients();
    if clients.is_empty() {
        println!("  ✗ No AI clients detected.");
        println!(
            "    Install one of: Claude Code, Cursor, Windsurf, Continue, Zed, VS Code/Copilot,"
        );
        println!(
            "    Codex, Gemini CLI, Claude Desktop, Cline, Antigravity, Grok Build CLI, Goose."
        );
        println!();
        return Ok(());
    }

    println!("  {} Detected {} client(s):", "✓".green(), clients.len());
    for client in &clients {
        let status = if client.config_exists {
            " (already has erebyx config — would be re-merged)"
                .dimmed()
                .to_string()
        } else {
            String::new()
        };
        println!("    • {}{}", client.name.bold(), status);
    }
    println!();

    // Enumerate files that would be written
    println!("  {}", "Files that would be written:".bold());
    for client in &clients {
        // MCP config file
        println!(
            "    • {} config — {}",
            client.name.dimmed(),
            client.config_path.display()
        );
        // Rules file
        println!(
            "    • {} rules  — {}",
            client.name.dimmed(),
            client.rules_path.display()
        );
        // Claude Code: hook script
        if client.kind == detect::ClientKind::ClaudeCode {
            println!(
                "    • {} hook   — {}",
                client.name.dimmed(),
                client
                    .home_dir
                    .join("hooks")
                    .join("erebyx-memory-injector.sh")
                    .display()
            );
        }
    }
    println!();

    // Per-client config snippet preview — the EXACT MCP entry that would be
    // merged into each client's config file (with the placeholder key). Lets a
    // reviewer eyeball the schema per serializer (JSON object, JSON array,
    // TOML, YAML) before running the real setup.
    println!(
        "  {}",
        "Config that would be merged (placeholder key):".bold()
    );
    for client in &clients {
        let snippet = config::preview_mcp_config_with_credentials(client, &placeholder_credentials);
        println!();
        println!(
            "  ── {} → {}",
            client.name.bold(),
            client.config_path.display()
        );
        for line in snippet.lines() {
            println!("      {line}");
        }
    }
    println!();

    // Hooks summary (Claude Code specifically)
    let has_claude_code = clients
        .iter()
        .any(|c| c.kind == detect::ClientKind::ClaudeCode);
    if has_claude_code {
        println!(
            "  {}",
            "Hooks that would be installed (Claude Code):".bold()
        );
        println!("    • UserPromptSubmit — runs `erebyx hook-inject` on every prompt");
        println!("    • SessionStart     — runs `erebyx hook-session-start` once per session");
        println!("    • Both are marked `_erebyx_managed: true` so re-running setup replaces");
        println!("      them precisely; user-authored hooks alongside are preserved.");
        println!();
    }

    // Idempotency note
    let any_existing = clients.iter().any(|c| c.config_exists);
    if any_existing {
        println!("  {}", "Idempotency:".bold());
        println!("    Existing erebyx-managed entries would be replaced cleanly. Other");
        println!("    MCP servers and hooks in the same config files would be untouched.");
        println!();
    }

    println!("  {}", "Network calls (real setup only):".bold());
    println!(
        "    • POST {}/v0/identity/restore (1.5s timeout)",
        api_url.trim_end_matches('/').dimmed()
    );
    println!(
        "    • POST {}/v0/session/load     (1.5s timeout)",
        api_url.trim_end_matches('/').dimmed()
    );
    println!("    Output: rendered as `<!-- EREBYX:DYNAMIC -->` block in each rules file.");
    println!("    Skipped in this dry run.");
    println!();

    println!(
        "  {} placeholder used wherever credential material would appear.",
        placeholder_key.dimmed()
    );

    // Remote connectors are part of what `erebyx setup` surfaces — show them
    // in the dry-run too so the preview is faithful.
    print_remote_connectors();
    println!();
    println!("  Re-run without `--dry-run` to perform setup.");
    println!();
    Ok(())
}

fn resolve_setup_credentials(
    api_key: Option<String>,
    api_url: Option<String>,
    instance_id: Option<String>,
    passphrase: Option<String>,
) -> Result<config::SetupCredentials> {
    let stored = credentials::load_credentials()?.unwrap_or_else(|| StoredCredentials {
        api_key: String::new(),
        api_url: "https://core.erebyx.com".to_string(),
        instance_id: String::new(),
        passphrase: None,
    });

    let api_key = api_key
        .or_else(|| std::env::var("EREBYX_API_KEY").ok())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| (!stored.api_key.trim().is_empty()).then_some(stored.api_key.clone()))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            println!(
                "  No stored API key found. `erebyx login` will save it for future setup runs."
            );
            Password::new()
                .with_prompt("  Enter your EREBYX API key")
                .interact()
                .unwrap_or_default()
        });
    if api_key.trim().is_empty() {
        anyhow::bail!("EREBYX_API_KEY is required. Run `erebyx login` or pass --api-key.");
    }

    let api_url = api_url
        .or_else(|| std::env::var("EREBYX_API_URL").ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(stored.api_url.clone())
        .trim()
        .trim_end_matches('/')
        .to_string();

    let instance_id = instance_id
        .or_else(|| std::env::var("EREBYX_INSTANCE_ID").ok())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| (!stored.instance_id.trim().is_empty()).then_some(stored.instance_id.clone()))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| prompt_line("  Existing counterpart instance id").unwrap_or_default());
    if instance_id.is_empty() || instance_id == "default" {
        anyhow::bail!(
            "A real counterpart instance id is required. Run `erebyx login --instance-id <id>` with the id from dashboard/API enrollment; setup will not write silently-broken `default` configs."
        );
    }

    let passphrase = passphrase
        .or_else(|| std::env::var("EREBYX_PASSPHRASE").ok())
        .filter(|s| !s.trim().is_empty())
        .or(stored.passphrase.clone())
        .or_else(|| {
            println!("  No passphrase found. Leave blank only for a tenant that does not use passphrase-bound encryption.");
            Password::new()
                .with_prompt("  Counterpart passphrase")
                .allow_empty_password(true)
                .interact()
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    if passphrase.is_none() {
        println!(
            "  {} No EREBYX_PASSPHRASE will be written; encrypted tenants will authenticate but fail to decrypt until you run `erebyx login` with a passphrase.",
            "⚠".yellow()
        );
    }

    Ok(config::SetupCredentials {
        api_key,
        api_url,
        instance_id,
        passphrase,
    })
}

fn prompt_line(prompt: &str) -> Result<String> {
    use std::io::Write;
    print!("{prompt}: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub async fn run_setup(
    api_key: Option<String>,
    api_url: Option<String>,
    instance_id: Option<String>,
    passphrase: Option<String>,
    assume_yes: bool,
) -> Result<()> {
    println!();
    println!("{}", "  EREBYX setup — universal AI memory".bold().cyan());
    println!(
        "{}",
        "  Install persistent memory across all your AI tools.".dimmed()
    );
    println!();

    // Step 1: Detect installed clients
    let spinner = make_spinner("Detecting AI clients...");
    let clients = detect_clients();
    spinner.finish_and_clear();

    if clients.is_empty() {
        println!("{}", "  No supported AI clients detected.".yellow());
        println!("  Supported: Claude Code, Cursor, Windsurf, Continue, Zed, VS Code/Copilot,");
        println!("             Codex, Gemini CLI, Claude Desktop, Cline, Antigravity, Grok Build CLI, Goose");
        println!("  Install one of these and run `erebyx setup` again.");
        return Ok(());
    }

    println!(
        "  {} Detected {} client(s):",
        "✓".green().bold(),
        clients.len()
    );
    for client in &clients {
        let status = if client.config_exists {
            "(already configured)".dimmed().to_string()
        } else {
            String::new()
        };
        println!("    {} {} {}", "•".cyan(), client.name.bold(), status);
    }
    println!();

    let credentials = resolve_setup_credentials(api_key, api_url, instance_id, passphrase)?;
    match credentials::save_credentials(&StoredCredentials {
        api_key: credentials.api_key.clone(),
        api_url: credentials.api_url.clone(),
        instance_id: credentials.instance_id.clone(),
        passphrase: credentials.passphrase.clone(),
    }) {
        Ok(path) => println!(
            "  {} Saved counterpart credentials for future CLI/setup runs ({})",
            "✓".green().bold(),
            path.display()
        ),
        Err(e) => println!(
            "  {} Could not save local credential store; setup will continue ({})",
            "⚠".yellow(),
            e
        ),
    }

    // P1-4: enforce HTTPS-or-localhost on the resolved URL BEFORE writing any
    // config, rules, or hook script with it — and before `fetch_dynamic_context`
    // would POST the bearer token. One guard, all downstream consumers covered.
    ensure_safe_api_url(&credentials.api_url)?;

    // Step 3: Choose which clients to configure
    let unconfigured: Vec<&AiClient> = clients.iter().filter(|c| !c.config_exists).collect();
    let already_configured: Vec<&AiClient> = clients.iter().filter(|c| c.config_exists).collect();

    let to_configure: Vec<&AiClient> = if unconfigured.is_empty() {
        // All already configured — ask if they want to reconfigure.
        // CLI v0.1.2 (--yes fix): `Confirm` needs a TTY; under CI /
        // non-interactive re-provisioning it errors with `not a terminal`
        // and exits 1. `--yes`/`--force` skips the prompt (auto-yes) so a
        // re-run reconfigures every client without a terminal.
        let reconfigure = if assume_yes {
            println!(
                "  {} --yes: reconfiguring all already-configured clients.",
                "✓".green().bold()
            );
            true
        } else {
            Confirm::new()
                .with_prompt("  All clients already configured. Reconfigure?")
                .default(false)
                .interact()?
        };

        if reconfigure {
            clients.iter().collect()
        } else {
            println!("  {} Nothing to do.", "✓".green().bold());
            return Ok(());
        }
    } else if !already_configured.is_empty() {
        // Mix of configured and unconfigured
        println!(
            "  {} client(s) already configured, {} new.",
            already_configured.len(),
            unconfigured.len()
        );
        let include_existing = if assume_yes {
            println!(
                "  {} --yes: also reconfiguring already-configured clients.",
                "✓".green().bold()
            );
            true
        } else {
            Confirm::new()
                .with_prompt("  Also reconfigure already-configured clients?")
                .default(false)
                .interact()?
        };

        if include_existing {
            clients.iter().collect()
        } else {
            unconfigured
        }
    } else {
        unconfigured
    };

    if to_configure.is_empty() {
        println!("  {} Nothing to configure.", "✓".green().bold());
        return Ok(());
    }

    println!();

    // Step 4: Configure each client
    let pb = ProgressBar::new(to_configure.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} [{bar:20.cyan/dim}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let mut success_count = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut has_claude_code = false;
    // Trust-by-showing-work: collect every path setup touches so the
    // final screen can list them. Fly Launch pattern per
    // ADOPTION_MECHANICS_RESEARCH.md §1.
    let mut paths_touched: Vec<(String, PathBuf)> = Vec::new();

    for client in &to_configure {
        pb.set_message(format!("Configuring {}...", client.name));

        // Write MCP server config
        match config::write_mcp_config_with_credentials(client, &credentials) {
            Ok(path) => {
                paths_touched.push((format!("{} config", client.name), path.clone()));
                // Write rules file
                match rules::write_rules_file(client) {
                    Ok(rules_path) => {
                        paths_touched.push((format!("{} rules", client.name), rules_path));
                        // Install hooks (Claude Code only)
                        if client.kind == detect::ClientKind::ClaudeCode {
                            has_claude_code = true;
                            match hooks::install_hooks(client, &credentials) {
                                Ok(_) => {
                                    paths_touched.push((
                                        "Claude Code hook script".to_string(),
                                        client
                                            .home_dir
                                            .join("hooks")
                                            .join("erebyx-memory-injector.sh"),
                                    ));
                                }
                                Err(e) => {
                                    errors.push(format!(
                                        "{}: hooks failed ({}), config OK",
                                        client.name, e
                                    ));
                                }
                            }
                        }
                        success_count += 1;
                    }
                    Err(e) => {
                        errors.push(format!(
                            "{}: rules failed ({}), config written to {}",
                            client.name,
                            e,
                            path.display()
                        ));
                        success_count += 1; // Config still worked
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", client.name, e));
            }
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    // Step 4.5: Fetch + write the dynamic context block into each
    // configured client's rules file. This is the model-agnostic
    // pre-injection mechanism: clients without a native SessionStart
    // hook (Windsurf, Continue, Zed, VS Code/Copilot) get fresh
    // identity + handoff context via the always-loaded rules file.
    // Fail-open: if the substrate call fails, dynamic_content is
    // empty and `write_dynamic_block` becomes a no-op per-file.
    if !to_configure.is_empty() {
        let dynamic_content = fetch_dynamic_context(&credentials).await;
        if !dynamic_content.is_empty() {
            for client in &to_configure {
                if let Err(e) = rules::write_dynamic_block(client, &dynamic_content) {
                    errors.push(format!(
                        "{}: dynamic-block write skipped ({})",
                        client.name, e
                    ));
                }
            }
        }
        match credentials::portable_harness_dir().and_then(|dir| {
            rules::write_portable_harness_files(&dir, &credentials, &dynamic_content)
        }) {
            Ok(paths) => {
                for path in paths {
                    let label = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| format!("Portable {}", n))
                        .unwrap_or_else(|| "Portable harness file".to_string());
                    paths_touched.push((label, path));
                }
            }
            Err(e) => errors.push(format!("portable harness files skipped ({})", e)),
        }
    }

    // Step 5: Summary
    println!();
    if success_count > 0 {
        println!(
            "  {} Configured {} client(s) with EREBYX memory.",
            "✓".green().bold(),
            success_count
        );
    }

    if !errors.is_empty() {
        println!();
        for err in &errors {
            println!("  {} {}", "⚠".yellow(), err);
        }
    }

    // Files written — show every path setup touched so the user knows
    // what's on their disk + can audit / revert by hand. Fly Launch +
    // Vercel CLI pattern: trust by showing the work.
    if !paths_touched.is_empty() {
        println!();
        println!("  {}", "Files written:".bold());
        for (label, path) in &paths_touched {
            println!(
                "  • {} — {}",
                label.dimmed(),
                path.display().to_string().dimmed()
            );
        }
    }

    println!();
    println!("  {}", "What happens now:".bold());
    println!("  • Your AI tools will have access to EREBYX memory tools");
    println!("  • Rules files guide your AI to use memory proactively");
    println!("  • Memory persists across every configured client");

    // Claude Code hooks need EREBYX_API_KEY in the shell environment at runtime.
    // We deliberately do NOT echo the key — it would land in shell history.
    if has_claude_code {
        println!();
        println!("  {}", "Required for Claude Code hooks:".bold().yellow());
        println!("  Setup wrote install-time credential defaults into the owner-only hook script.");
        println!("  Env vars still override those defaults. To rotate manually, add these to your shell profile:");
        println!();
        println!("    export EREBYX_API_KEY=\"<your-api-key>\"");
        println!(
            "    export EREBYX_INSTANCE_ID=\"{}\"",
            credentials.instance_id
        );
        println!("    export EREBYX_PASSPHRASE=\"<your-passphrase>\"");
        println!();
        println!(
            "  {}",
            "Use the same credential set saved by `erebyx login`.".dimmed()
        );
    }

    // Try it out — Stripe CLI pattern. Give the user a concrete next
    // command + an example prompt so the time-to-first-value moment is
    // unmistakable.
    println!();
    println!("  {}", "Try it out:".bold());
    println!("  1. Restart your AI client(s) so the new MCP server loads.");
    println!("  2. In your AI, paste a prompt like:");
    println!();
    println!(
        "       {}",
        "\"Save that I'm setting up EREBYX, category: identity\"".cyan()
    );
    println!();
    println!("  3. Then ask:");
    println!();
    println!("       {}", "\"What did I last save?\"".cyan());

    println!();
    println!(
        "  {}",
        "Run `erebyx doctor` to verify all connections.".dimmed()
    );

    // Remote MCP connectors — clients with NO local config file. They point
    // at the hosted substrate over HTTP with a Bearer token, added by hand
    // in each app's UI.
    print_remote_connectors();
    println!();

    // CLI v0.1.2 (exit-code fix): if we attempted to configure clients but
    // EVERY one failed, setup must exit non-zero. Pre-fix it returned Ok(())
    // even on total failure, so `erebyx setup && <next>` chained past a
    // completely broken install (e.g. unwritable config dirs in CI). A
    // partial success (some clients configured) stays exit 0 — the errors
    // are surfaced above and the working clients are usable.
    if success_count == 0 {
        anyhow::bail!(
            "setup failed for all {} client(s) — no MCP config was written. See the ⚠ lines above.",
            to_configure.len()
        );
    }

    Ok(())
}

/// Print the "Remote connectors (add manually)" section.
///
/// These clients have NO local config file an installer can write — they
/// register an MCP server through their own UI, pointing at the hosted EREBYX
/// substrate over HTTP with a Bearer token. Listed at the end of `erebyx setup`
/// (and the dry-run) so the user knows the manual step for each.
fn print_remote_connectors() {
    println!();
    println!("  {}", "Remote connectors (add manually):".bold());
    println!(
        "  {}",
        "These have no local config file — add them in each app's UI:".dimmed()
    );
    println!();
    println!(
        "    {} https://core.erebyx.com/mcp  with  Authorization: Bearer <EREBYX_API_KEY>",
        "URL:".dimmed()
    );
    println!();
    println!(
        "    • {} — Settings → Connectors → Developer Mode → add custom connector",
        "ChatGPT".bold()
    );
    println!(
        "    • {} — add a custom MCP server / integration with the URL + Bearer header",
        "Grok chat app".bold()
    );
    println!(
        "    • {} — add an MCP server with the URL + Bearer header",
        "JetBrains AI".bold()
    );
}

fn make_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

#[cfg(test)]
mod dynamic_block_tests {
    use super::*;
    use serde_json::json;

    /// Guard against imperative command framing in the dynamic-block
    /// content, mirroring the static-rules test in `rules.rs`.
    ///
    /// The dynamic block is rendered into the SAME rules file as the
    /// static block — claude-code#17804's prompt-injection defense
    /// applies to BOTH surfaces. If a future schema adds a field that
    /// renders with imperative phrasing (e.g. a "next_action_hint"
    /// field containing "Call wrap_up immediately"), it could trip the
    /// defense and surface to the user as text instead of context.
    #[test]
    fn render_dynamic_block_uses_declarative_not_imperative_framing() {
        let id = json!({
            "identity": {"name": "Ada"},
            "ethos": [
                "Clarity over cleverness",
                "Tests before code",
            ],
        });
        let ctx = json!({
            "handoff": {
                "what_we_built": "ship session-start pre-injection",
                "whats_next": "follow-up review + fix forward",
            },
            "anchors": ["launch-prep", "cli"],
        });
        let out = render_dynamic_block(Some(&id), Some(&ctx));
        let lower = out.to_lowercase();
        for banned in &[
            "you must",
            "you should",
            "always call",
            "do not ask permission",
            "bias toward firing",
            "fire proactively",
        ] {
            assert!(
                !lower.contains(banned),
                "render_dynamic_block contains imperative phrase '{}' — would risk \
                 tripping claude-code#17804 prompt-injection defense. Output: {}",
                banned,
                out
            );
        }
    }

    /// Verify `render_dynamic_block` returns empty when given no inputs.
    /// Without this, an empty header-only block would land in every
    /// rules file even when the substrate is unreachable.
    #[test]
    fn render_dynamic_block_empty_when_no_inputs() {
        let out = render_dynamic_block(None, None);
        assert!(
            out.is_empty(),
            "expected empty output for nil inputs, got: {}",
            out
        );
    }

    /// Verify the renderer respects the MAX_CHARS cap. Without this,
    /// a substrate that returns a 1MB narrative could bloat the dynamic
    /// block past any rules-file budget.
    #[test]
    fn render_dynamic_block_respects_char_budget() {
        let big = "x".repeat(10_000);
        let ctx = json!({
            "handoff": {
                "what_we_built": big.clone(),
                "whats_next": big,
            },
        });
        let out = render_dynamic_block(None, Some(&ctx));
        // MAX_CHARS = 3200; allow ~200 chars of structural overhead.
        assert!(
            out.len() <= 3400,
            "expected bounded output ≤3400 chars, got {} chars",
            out.len()
        );
    }
}

#[cfg(test)]
mod api_url_guard_tests {
    use super::ensure_safe_api_url;

    /// REGRESSION (P1-4): setup must enforce HTTPS-or-localhost on
    /// `api_url`. Against the original `mod.rs` there was NO such guard —
    /// these unsafe URLs flowed straight into `fetch_dynamic_context`
    /// (POSTing the bearer token) and into the hook/config writers.
    #[test]
    fn ensure_safe_api_url_rejects_plain_http_to_internet() {
        let err = ensure_safe_api_url("http://evil.example.com").unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("https://"),
            "error must explain HTTPS requirement: {msg}"
        );
        assert!(
            msg.contains("evil.example.com"),
            "error must echo the offending URL: {msg}"
        );
    }

    #[test]
    fn ensure_safe_api_url_rejects_wrong_scheme() {
        for bad in &[
            "ftp://evil.example.com",
            "file:///etc/passwd",
            "http://evil.example.com",
            "ws://evil.example.com",
        ] {
            assert!(
                ensure_safe_api_url(bad).is_err(),
                "setup must reject wrong-scheme api_url {bad:?}"
            );
        }
    }

    /// `ensure_safe_api_url` is the scheme guard, so a `https://`-prefixed
    /// injection payload PASSES it — documenting why hooks.rs additionally
    /// single-quotes the value (defense-in-depth). The setup HTTP paths
    /// (`fetch_dynamic_context`) only ever use the URL as a reqwest base,
    /// never as shell, so scheme-validation is the correct guard there; the
    /// shell-breakout surface lives solely in the generated hook script.
    #[test]
    fn ensure_safe_api_url_scheme_only_documents_defense_in_depth() {
        assert!(
            ensure_safe_api_url(r#"https://evil}$(touch /tmp/pwn)"#).is_ok(),
            "scheme guard accepts https:// payloads by design; hook script \
             single-quoting is the breakout defense"
        );
    }

    #[test]
    fn ensure_safe_api_url_accepts_https_and_localhost() {
        for ok in &[
            "https://core.erebyx.com",
            "http://localhost:8080",
            "http://127.0.0.1:9000/mcp",
        ] {
            assert!(
                ensure_safe_api_url(ok).is_ok(),
                "valid api_url {ok:?} must pass"
            );
        }
    }

    /// `run_setup_dry_run` validates the URL before touching anything.
    /// Pre-fix it would print a preview for an unsafe URL the real run
    /// rejects. Async-but-no-I/O, so it runs without a substrate.
    #[tokio::test]
    async fn run_setup_dry_run_rejects_unsafe_url() {
        let res = super::run_setup_dry_run(None, Some("http://evil.example.com".to_string())).await;
        assert!(
            res.is_err(),
            "dry-run must reject a plain-http internet URL"
        );
    }
}
