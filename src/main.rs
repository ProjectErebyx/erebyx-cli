// SPDX-License-Identifier: Apache-2.0
mod cli;
mod client;
mod output;
mod setup;

use anyhow::Result;
use clap::Parser;
use serde_json::{json, Value};
use std::env;
use std::io::{IsTerminal, Read};

use cli::{Cli, Commands};
use client::{session_id, ErebyxClient};
use output::{print_error, print_response, print_response_with_hints};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        print_error(&format!("{:#}", e));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let json_mode = cli.json;

    match cli.command {
        Commands::Health => {
            // P1-1 (2026-05-27): the first cold-touch command must work
            // BEFORE the customer has run `erebyx setup` or set
            // EREBYX_API_KEY. If no key is configured, hit the substrate's
            // unauthenticated /health route directly so the customer can
            // confirm reachability without paying for an API key first.
            if env::var("EREBYX_API_KEY").is_err() {
                let result = ErebyxClient::health_anonymous(None).await?;
                if json_mode {
                    print_response(&result, false, json_mode);
                } else {
                    print_response(&result, false, json_mode);
                    println!();
                    println!(
                        "  {} no EREBYX_API_KEY configured — run `erebyx setup` to authenticate.",
                        "•"
                    );
                }
            } else {
                let client = ErebyxClient::new()?;
                let result = client.health().await?;
                print_response(&result, false, json_mode);
            }
        }

        Commands::Setup { api_key, api_url } => {
            setup::run_setup(api_key, api_url).await?;
        }

        Commands::HookInject => {
            hook_inject().await;
        }

        Commands::Doctor => {
            // Quick health check + client detection
            println!();
            println!("  {} Checking Erebyx...", "•".to_string());

            // Check server
            match ErebyxClient::new() {
                Ok(client) => match client.health().await {
                    Ok(_) => println!("  {} Server: connected", "✓"),
                    Err(e) => println!("  {} Server: {}", "✗", e),
                },
                Err(e) => println!("  {} Server: {} (set EREBYX_API_KEY)", "✗", e),
            }

            // Check clients
            let clients = setup::detect::detect_clients();
            if clients.is_empty() {
                println!("  {} No AI clients detected", "✗");
            } else {
                for client in &clients {
                    let status = if client.config_exists {
                        format!("{} configured", "✓")
                    } else {
                        format!("{} not configured (run `erebyx setup`)", "✗")
                    };
                    println!("  {} {}: {}", "•", client.name, status);
                }
            }
            println!();
        }

        Commands::RestoreIdentity {
            limit,
            include_guide,
            detail,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({});

            if let Some(limit) = limit {
                args["limit"] = json!(limit);
            }
            if include_guide {
                args["include_guide"] = json!(true);
            }
            if let Some(detail) = detail {
                args["detail"] = json!(detail.to_string());
            }

            let resp = client.call_tool("restore_identity", args).await?;
            print_response_with_hints(&resp.content, resp.is_error, json_mode, &resp.hints, &resp.auto_fired);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::LoadContext { anchors, mode } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({});

            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(mode) = mode {
                args["mode"] = json!(mode.to_string());
            }

            let resp = client.call_tool("load_context", args).await?;
            print_response_with_hints(&resp.content, resp.is_error, json_mode, &resp.hints, &resp.auto_fired);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Save {
            content,
            category,
            title,
            anchors,
            importance,
            memory_type,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "content": content,
                "category": category,
            });

            if let Some(title) = title {
                args["title"] = json!(title);
            }
            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(importance) = importance {
                args["importance"] = json!(importance);
            }
            if let Some(memory_type) = memory_type {
                args["type"] = json!(memory_type.to_string());
            }

            let resp = client.call_tool("save", args).await?;
            print_response_with_hints(&resp.content, resp.is_error, json_mode, &resp.hints, &resp.auto_fired);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::Remember {
            query,
            anchors,
            limit,
            time_range,
            ids,
            generative,
            types,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "query": query,
                "limit": limit,
            });

            if let Some(anchors) = anchors {
                args["hint_anchors"] = json!(anchors);
            }
            if let Some(time_range) = time_range {
                args["time_range"] = json!(time_range.to_string());
            }
            if let Some(ids) = ids {
                args["ids"] = json!(ids);
            }
            if let Some(generative) = generative {
                args["generative"] = json!(generative.to_string());
            }
            if let Some(types) = types {
                args["types"] = json!(types);
            }

            let resp = client.call_tool("remember", args).await?;
            print_response_with_hints(&resp.content, resp.is_error, json_mode, &resp.hints, &resp.auto_fired);
            if resp.is_error {
                std::process::exit(1);
            }
        }

        Commands::McpServe => {
            mcp_serve().await?;
        }

        Commands::WrapUp {
            what_we_built,
            whats_next,
            anchors,
            energy,
            diary,
        } => {
            let client = ErebyxClient::new()?;
            let mut args = json!({
                "what_we_built": what_we_built,
                "whats_next": whats_next,
            });

            if let Some(anchors) = anchors {
                args["anchors"] = json!(anchors);
            }
            if let Some(energy) = energy {
                args["energy"] = json!(energy);
            }
            if let Some(diary) = diary {
                args["diary"] = json!(diary);
            }

            let resp = client.call_tool("wrap_up", args).await?;
            print_response_with_hints(&resp.content, resp.is_error, json_mode, &resp.hints, &resp.auto_fired);
            if resp.is_error {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// MCP stdio server bridge.
///
/// Reads JSON-RPC requests line-by-line from stdin, forwards them to the
/// substrate's `/mcp/` HTTP endpoint with the configured API key, and writes
/// each response back to stdout per MCP's stdio transport
/// (one JSON-RPC message per line, framed by newline).
///
/// Authoritative protocol behavior — including tool surface, schemas, and
/// initialization — lives on the substrate. This bridge stays intentionally
/// thin so it never drifts out of sync with the server.
///
/// Errors on a single message are surfaced as JSON-RPC error responses so the
/// client never sees a hung pipe. Fatal errors (no API key, unreachable host)
/// exit non-zero so the parent harness can report a launch failure.
async fn mcp_serve() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let client = ErebyxClient::new()?;
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    loop {
        // P1-3 (2026-05-27): ride out transient I/O errors instead of
        // killing the bridge. macOS Spaces switches / backgrounding can
        // surface as Interrupted; only Ok(None) (clean EOF) or a hard
        // error tears the loop down.
        let line = match reader.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break, // clean EOF
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // P1-2 (2026-05-27): JSON-RPC 2.0 §4.1 — a Notification is a
        // Request without an `id`; the server MUST NOT respond. MCP uses
        // notifications for `notifications/cancelled` and
        // `notifications/initialized` etc. Claude Code's MCP client
        // treats spurious responses to notifications as protocol
        // violations and disconnects. Detect + fire-and-forget.
        let parsed: Option<Value> = serde_json::from_str(trimmed).ok();
        let is_notification = parsed
            .as_ref()
            .map(|v| v.get("id").is_none())
            .unwrap_or(false);
        if is_notification {
            // Proxy upstream so the substrate sees the notification
            // (e.g. `notifications/initialized` finishes the MCP handshake)
            // but discard whatever the substrate sends back — spec says
            // we MUST NOT echo a response.
            let _ = client.proxy_jsonrpc(trimmed).await;
            continue;
        }

        let response = match client.proxy_jsonrpc(trimmed).await {
            Ok(v) => v,
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": parsed
                    .as_ref()
                    .and_then(|v| v.get("id").cloned())
                    .unwrap_or(Value::Null),
                "error": {
                    "code": -32603,
                    "message": format!("erebyx mcp-serve bridge error: {}", e)
                }
            }),
        };

        let serialized = serde_json::to_string(&response)?;
        stdout.write_all(serialized.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

/// Native hook-inject handler for Claude Code UserPromptSubmit hook.
///
/// Reads JSON from stdin, smart-gates short/greeting messages, calls the
/// Erebyx remember endpoint with a 500ms hard timeout, and emits an
/// `additionalContext` JSON to stdout. Fail-open: any error path emits `{}`
/// so Claude Code never blocks on a memory hiccup.
///
/// Replaces a 100-line bash + python3 pipe-chain. Single Rust binary, no
/// runtime dependencies, shared connection pool, predictable latency.
async fn hook_inject() {
    // Detect direct/interactive invocation — this command is for Claude Code hooks,
    // not direct CLI use. Bail with a helpful message instead of hanging on stdin.
    if std::io::stdin().is_terminal() {
        eprintln!("erebyx hook-inject is an internal command for Claude Code hooks.");
        eprintln!("It reads JSON from stdin. You probably want `erebyx remember <query>` instead.");
        std::process::exit(2);
    }

    // Always emit valid JSON to stdout — never panic, never error out.
    let result = run_hook_inject().await;
    println!("{}", result);
}

async fn run_hook_inject() -> String {
    let empty = "{}".to_string();

    // Read hook input from stdin.
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return empty;
    }

    // Parse the user_message field.
    let parsed: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return empty,
    };
    let user_message = parsed
        .get("user_message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .trim();

    // Smart gate: skip short messages and common greetings.
    // Pattern unified with `erebyx_sdk::middleware::is_greeting` so the CLI
    // hook and the SDK middleware skip the same set of messages.
    if user_message.len() < 15 {
        return empty;
    }
    let lower = user_message.to_lowercase();
    let greetings = [
        "hey", "hi", "hello", "thanks", "thank you", "bye", "ok", "yes", "no",
        "sure", "cool", "nice", "got it", "sounds good", "okay", "yep", "nope", "alright",
    ];
    if lower.len() < 30 && greetings.iter().any(|g| lower.starts_with(g)) {
        return empty;
    }

    // Truncate query to 200 chars (UTF-8 safe).
    let query: String = user_message.chars().take(200).collect();

    // Read API key + URL from env. Fail-open if missing.
    let api_key = match std::env::var("EREBYX_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return empty,
    };
    let api_url = std::env::var("EREBYX_API_URL")
        .unwrap_or_else(|_| "https://core.erebyx.com".to_string());

    // Build a quick HTTP client with 500ms hard timeout.
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return empty,
    };

    let url = format!("{}/v0/memory/remember", api_url.trim_end_matches('/'));
    let body = json!({ "query": query, "limit": 5 });

    let response = match http
        .post(&url)
        .header("Content-Type", "application/json")
        .bearer_auth(&api_key)
        .header("X-Instance-ID", "default")
        .header("X-Erebyx-Session-Id", session_id())
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return empty,
    };

    // Cap response body to 10 MiB — defensive against runaway server payloads.
    if let Some(len) = response.content_length() {
        if len > 10 * 1024 * 1024 {
            return empty;
        }
    }

    let data: Value = match response.json().await {
        Ok(v) => v,
        Err(_) => return empty,
    };

    // Memories live under either `memories` or `results` depending on response shape.
    let memories = data
        .get("memories")
        .or_else(|| data.get("results"))
        .and_then(|m| m.as_array())
        .filter(|arr| !arr.is_empty());

    let memories = match memories {
        Some(m) => m,
        None => return empty,
    };

    // Format injection context (cap total at ~1500 chars to keep prompt small).
    let mut lines: Vec<String> = vec!["[Erebyx Memory Context]".to_string()];
    let mut total = 0usize;
    for m in memories.iter().take(5) {
        let content = m
            .get("content")
            .or_else(|| m.get("text"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let snippet: String = content.chars().take(300).collect();
        if total + snippet.len() > 1500 {
            break;
        }
        if !snippet.trim().is_empty() {
            lines.push(format!("- {}", snippet));
            total += snippet.len();
        }
    }

    if lines.len() < 2 {
        return empty;
    }

    json!({
        "additionalContext": [{
            "type": "text",
            "text": lines.join("\n")
        }]
    })
    .to_string()
}
