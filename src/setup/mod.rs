// SPDX-License-Identifier: Apache-2.0
//! `erebyx setup` — one-command memory installation for all AI coding clients.
//!
//! Detects installed clients (Claude Code, Cursor, Windsurf, Continue, Zed, Copilot),
//! writes MCP server config for each, injects rules files, and installs hooks.

pub mod config;
pub mod detect;
pub mod hooks;
pub mod rules;

use anyhow::Result;
use colored::Colorize;
use dialoguer::{Confirm, Password};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use detect::{detect_clients, AiClient};

/// Run the interactive setup flow.
pub async fn run_setup(api_key: Option<String>, api_url: Option<String>) -> Result<()> {
    println!();
    println!("{}", "  Erebyx setup — universal AI memory".bold().cyan());
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
        println!("  Supported: Claude Code, Cursor, Windsurf, Continue, Zed, VS Code/Copilot");
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

    // Step 2: Get API key
    let api_key = match api_key {
        Some(key) => key,
        None => {
            let env_key = std::env::var("EREBYX_API_KEY").ok();
            if let Some(key) = env_key {
                println!(
                    "  {} Using EREBYX_API_KEY from environment",
                    "✓".green().bold()
                );
                key
            } else {
                // Mask input — API keys must never land in terminal scrollback or shell history.
                Password::new()
                    .with_prompt("  Enter your Erebyx API key")
                    .interact()?
            }
        }
    };

    let api_url = api_url
        .or_else(|| std::env::var("EREBYX_API_URL").ok())
        .unwrap_or_else(|| "https://core.erebyx.com".to_string());

    // Step 3: Choose which clients to configure
    let unconfigured: Vec<&AiClient> = clients.iter().filter(|c| !c.config_exists).collect();
    let already_configured: Vec<&AiClient> = clients.iter().filter(|c| c.config_exists).collect();

    let to_configure: Vec<&AiClient> = if unconfigured.is_empty() {
        // All already configured — ask if they want to reconfigure
        let reconfigure = Confirm::new()
            .with_prompt("  All clients already configured. Reconfigure?")
            .default(false)
            .interact()?;

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
        let include_existing = Confirm::new()
            .with_prompt("  Also reconfigure already-configured clients?")
            .default(false)
            .interact()?;

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
        match config::write_mcp_config(client, &api_key, &api_url) {
            Ok(path) => {
                paths_touched.push((format!("{} config", client.name), path.clone()));
                // Write rules file
                match rules::write_rules_file(client) {
                    Ok(rules_path) => {
                        paths_touched.push((format!("{} rules", client.name), rules_path));
                        // Install hooks (Claude Code only)
                        if client.kind == detect::ClientKind::ClaudeCode {
                            has_claude_code = true;
                            match hooks::install_hooks(client, &api_key, &api_url) {
                                Ok(_) => {
                                    paths_touched.push((
                                        "Claude Code hook script".to_string(),
                                        client.home_dir.join("hooks").join("erebyx-memory-injector.sh"),
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

    // Step 5: Summary
    println!();
    if success_count > 0 {
        println!(
            "  {} Configured {} client(s) with Erebyx memory.",
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
    println!("  • Your AI tools will have access to Erebyx memory tools");
    println!("  • Rules files guide your AI to use memory proactively");
    println!("  • Memory persists across every configured client");

    // Claude Code hooks need EREBYX_API_KEY in the shell environment at runtime.
    // We deliberately do NOT echo the key — it would land in shell history.
    if has_claude_code {
        println!();
        println!("  {}", "Required for Claude Code hooks:".bold().yellow());
        println!("  Add this to your shell profile (~/.zshrc or ~/.bashrc):");
        println!();
        println!("    export EREBYX_API_KEY=\"<your-api-key>\"");
        println!();
        println!(
            "  {}",
            "Use the same key you pasted above. The MCP server reads it from config; hooks need it in your shell."
                .dimmed()
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
        "\"Save that I'm setting up Erebyx, category: identity\"".cyan()
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
    println!();

    Ok(())
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
