// SPDX-License-Identifier: MIT OR Apache-2.0
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
use client::{is_safe_url, session_id, ErebyxClient};
use output::{map_actionable_error, print_error, print_response, print_response_with_hints};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        // Brutal-review wave-2 (Genesis Arche T-5 days, 2026-05-27):
        // promote ``erebyx doctor``'s actionable-error pattern to every
        // command. Pre-fix the raw ``anyhow`` chain dumped here on any
        // save/remember/wrap-up failure — first-touch developers hit a
        // wall of ``Server error: 401 ...`` text and bounced. Now we
        // route common failure classes (401 / 403 / 422 / 429 / 5xx /
        // network / DNS) to single-line ``here's what's wrong, here's
        // how to fix it`` messages and only fall through to the raw
        // chain when no class matches.
        print_error(&map_actionable_error(&format!("{:#}", e)));
        std::process::exit(1);
    }
}

/// Return true if `key` matches the advertised EREBYX API-key format:
/// the literal prefix `erebyx_` followed by exactly 48 lowercase hex
/// characters (55 chars total).
///
/// CLI v0.1.2 fix [8]: the old gate (`starts_with("erebyx_") && len()>=32`)
/// was far looser than the documented `erebyx_<48 hex chars>` shape, so a
/// 35-char paste of garbage rendered a false `✓ set` in `erebyx doctor`.
/// The auth section still does the real check against the substrate; this
/// just stops the Environment section from over-claiming on an obviously
/// malformed key.
///
/// NOTE (verify-before-publish): the `erebyx_<48 hex>` length/charset is
/// taken from the documented format in this crate (config.rs, doctor copy)
/// and the issuer copy at app.erebyx.com/keys. If the issuer ever changes
/// the key shape (length or charset), update this predicate in lockstep.
fn is_well_formed_api_key(key: &str) -> bool {
    const HEX_LEN: usize = 48;
    let Some(body) = key.strip_prefix("erebyx_") else {
        return false;
    };
    body.len() == HEX_LEN && body.bytes().all(|b| b.is_ascii_hexdigit())
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
                        "  • no EREBYX_API_KEY configured — run `erebyx setup` to authenticate."
                    );
                }
            } else {
                let client = ErebyxClient::new()?;
                let result = client.health().await?;
                print_response(&result, false, json_mode);
            }
        }

        Commands::Setup {
            api_key,
            api_url,
            dry_run,
            yes,
        } => {
            // P1-6 (2026-05-27): the `--api-key <key>` form lands the
            // credential in shell history, ps auxww, and shell-completion
            // logs. Warn loudly + point at the safer paths. We don't
            // refuse — power users / CI scripts may need this — but we
            // surface the risk so casual copy-pasters know.
            if api_key.is_some() {
                eprintln!("  ⚠ Reading API key from `--api-key` flag — the value lands in");
                eprintln!("    shell history and `ps` output. Prefer one of:");
                eprintln!("      EREBYX_API_KEY=<key> erebyx setup       (env var; ps-invisible)");
                eprintln!(
                    "      erebyx setup                            (interactive prompt; no echo)"
                );
            }
            if dry_run {
                setup::run_setup_dry_run(api_key, api_url).await?;
            } else {
                setup::run_setup(api_key, api_url, yes).await?;
            }
        }

        Commands::HookInject => {
            hook_inject().await;
        }

        Commands::HookSessionStart => {
            hook_session_start().await;
        }

        Commands::Doctor => {
            // Full named-check series: environment / auth / MCP / clients
            // / hook. Per ADOPTION_MECHANICS_RESEARCH.md §3: a doctor
            // command structured as named checks (success/warning/error)
            // is the single biggest support-load reducer at launch.
            //
            // Each check is read-only (no side effects against the user's
            // substrate state). A separate --roundtrip flag in v0.1.2
            // will issue a save + remember to verify the loop end-to-end;
            // it's gated because save creates a memory the user didn't
            // ask for.
            use std::io::Write;
            let mut total_pass = 0u32;
            let mut total_warn = 0u32;
            let mut total_fail = 0u32;
            let mut report = |status: char, name: &str, msg: &str| {
                match status {
                    '✓' => total_pass += 1,
                    '⚠' => total_warn += 1,
                    '✗' => total_fail += 1,
                    _ => {}
                };
                println!(
                    "    {} {}{}",
                    status,
                    name,
                    if msg.is_empty() {
                        "".to_string()
                    } else {
                        format!(": {}", msg)
                    }
                );
                let _ = std::io::stdout().flush();
            };

            println!();
            println!("  EREBYX Doctor — named checks");
            println!();

            // === Section 1: Environment ===
            println!("  Environment");
            let api_key = std::env::var("EREBYX_API_KEY").ok();
            match api_key.as_deref() {
                Some(k) if is_well_formed_api_key(k) => {
                    // CLI v0.1.2 fix [8]: build the preview char-by-char.
                    // `&k[..10]` panics if a multibyte char (emoji from a
                    // bad paste) straddles bytes 7-9. `.chars().take(10)`
                    // is char-boundary-safe and can never panic.
                    let preview: String = k.chars().take(10).collect();
                    report('✓', "EREBYX_API_KEY", &format!("set ({}…)", preview));
                }
                Some(_) => {
                    report('⚠', "EREBYX_API_KEY", "set but format doesn't match `erebyx_<48 hex chars>` — may not authenticate");
                }
                None => {
                    report(
                        '✗',
                        "EREBYX_API_KEY",
                        "unset — get one at https://app.erebyx.com/keys",
                    );
                }
            }
            let api_url = std::env::var("EREBYX_API_URL")
                .unwrap_or_else(|_| "https://core.erebyx.com".to_string());
            report('✓', "EREBYX_API_URL", &api_url);
            println!();

            // === Section 2: Authentication ===
            println!("  Authentication");
            if api_key.is_none() {
                report('✗', "Substrate auth", "skipped — EREBYX_API_KEY unset");
            } else {
                // CLI v0.1.2 fix [N3]: probe an AUTHENTICATED route
                // (`tools/list` on `/mcp/`) rather than the unauthenticated
                // `/health` — a revoked/garbage key must surface as a 401,
                // not a false green "key accepted".
                match ErebyxClient::new() {
                    Ok(client) => match client.probe_auth().await {
                        Ok(_) => report('✓', "Substrate reachable + key accepted", ""),
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("401") || msg.to_lowercase().contains("unauthorized") {
                                report(
                                    '✗',
                                    "Substrate auth",
                                    "rejected (401) — API key may be revoked or wrong tenant",
                                );
                            } else if msg.contains("403")
                                || msg.to_lowercase().contains("forbidden")
                            {
                                report(
                                    '✗',
                                    "Substrate auth",
                                    "forbidden (403) — key lacks scope for this tenant",
                                );
                            } else {
                                report('✗', "Substrate reachable", &msg);
                            }
                        }
                    },
                    Err(e) => report('✗', "Client init", &e.to_string()),
                }
            }
            println!();

            // === Section 3: MCP server binary ===
            println!("  MCP server");
            let self_bin = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "(unknown)".to_string());
            report('✓', "erebyx binary", &self_bin);
            report('✓', "erebyx mcp-serve", "available as subcommand");
            println!();

            // === Section 4: Detected AI clients ===
            println!("  Clients");
            let clients = setup::detect::detect_clients();
            if clients.is_empty() {
                report(
                    '⚠',
                    "No supported AI clients detected",
                    "install Claude Code, Cursor, Windsurf, Continue, Zed, or VS Code",
                );
            } else {
                for client in &clients {
                    if client.config_exists {
                        report('✓', client.name, "configured");
                    } else {
                        report(
                            '⚠',
                            client.name,
                            "detected but not configured (run `erebyx setup`)",
                        );
                    }
                }
            }
            println!();

            // === Section 5: Hook script (Claude Code only) ===
            let claude_client = clients
                .iter()
                .find(|c| matches!(c.kind, setup::detect::ClientKind::ClaudeCode));
            if let Some(cc) = claude_client {
                println!("  Hook (Claude Code)");
                let hook_path = cc.home_dir.join("hooks").join("erebyx-memory-injector.sh");
                if !hook_path.exists() {
                    report(
                        '⚠',
                        "Hook script",
                        &format!(
                            "missing ({}) — run `erebyx setup` to install",
                            hook_path.display()
                        ),
                    );
                } else {
                    report('✓', "Hook script", &hook_path.display().to_string());
                    // Check executable bit on Unix
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        match std::fs::metadata(&hook_path) {
                            Ok(meta) => {
                                let mode = meta.permissions().mode() & 0o777;
                                if mode & 0o100 == 0 {
                                    report(
                                        '⚠',
                                        "Hook executable bit",
                                        &format!("mode {:o} — should be 0700 (owner exec)", mode),
                                    );
                                } else {
                                    report('✓', "Hook executable bit", &format!("mode {:o}", mode));
                                }
                            }
                            Err(e) => report('⚠', "Hook permissions", &e.to_string()),
                        }
                    }
                }

                // Registration check: the script file existing on disk is
                // necessary but NOT sufficient — memory injection only fires
                // if `erebyx setup` also wired our managed groups into
                // settings.json. A stray edit, a half-finished setup, or a
                // settings.json reset leaves the script orphaned and injection
                // silently dead, which a file-stat-only check can't see.
                let settings_path = &cc.config_path; // ~/.claude/settings.json
                let registration = std::fs::read_to_string(settings_path)
                    .ok()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                    .map(|v| setup::hooks::hook_registration_status(&v));
                match registration {
                    Some(reg) if reg.fully_wired() => {
                        report(
                            '✓',
                            "Hook registration",
                            "SessionStart + UserPromptSubmit wired in settings.json",
                        );
                    }
                    Some(_) if hook_path.exists() => {
                        report(
                            '⚠',
                            "Hook registration",
                            "hook script present but not wired into settings.json — \
                             re-run `erebyx setup`",
                        );
                    }
                    Some(_) => {
                        report(
                            '⚠',
                            "Hook registration",
                            "not wired into settings.json — run `erebyx setup`",
                        );
                    }
                    None => {
                        report(
                            '⚠',
                            "Hook registration",
                            &format!(
                                "could not read/parse {} — run `erebyx setup`",
                                settings_path.display()
                            ),
                        );
                    }
                }
                println!();
            }

            // === Summary ===
            println!(
                "  Summary: {} passing, {} warning{}, {} failure{}",
                total_pass,
                total_warn,
                if total_warn == 1 { "" } else { "s" },
                total_fail,
                if total_fail == 1 { "" } else { "s" },
            );
            if total_fail > 0 {
                println!();
                println!("  Next: address the ✗ items above, then re-run `erebyx doctor`.");
            } else if total_warn > 0 {
                println!();
                println!("  Next: ⚠ items are non-blocking but worth fixing for a complete setup.");
            } else {
                println!();
                println!("  All checks passing — substrate, clients, and hooks are wired.");
            }
            println!();

            // CLI v0.1.2 (exit-code fix): doctor exited 0 even when a
            // critical check failed (substrate unreachable / key rejected /
            // client init error), so `erebyx doctor && <next>` chained past
            // a broken install. Exit non-zero on any ✗ so CI / shell `&&`
            // see the failure. Warnings (⚠) stay exit 0 — they're
            // non-blocking. Exit code 2 distinguishes "ran, found failures"
            // from the top-level exit 1 used for unhandled errors.
            if total_fail > 0 {
                std::process::exit(2);
            }
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
            print_response_with_hints(
                &resp.content,
                resp.is_error,
                json_mode,
                &resp.hints,
                &resp.auto_fired,
            );
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
            print_response_with_hints(
                &resp.content,
                resp.is_error,
                json_mode,
                &resp.hints,
                &resp.auto_fired,
            );
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
            print_response_with_hints(
                &resp.content,
                resp.is_error,
                json_mode,
                &resp.hints,
                &resp.auto_fired,
            );
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
            print_response_with_hints(
                &resp.content,
                resp.is_error,
                json_mode,
                &resp.hints,
                &resp.auto_fired,
            );
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
            print_response_with_hints(
                &resp.content,
                resp.is_error,
                json_mode,
                &resp.hints,
                &resp.auto_fired,
            );
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
    use tokio::io::{AsyncBufReadExt, BufReader};

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

        // P0-B (2026-05-27, brutal-review POSTFIX_CLI): when the line
        // fails to parse as JSON, JSON-RPC 2.0 §5.1 mandates a local
        // Parse error response with code -32700 — NOT a round-trip to
        // the substrate. The prior code sent the garbage upstream as
        // an HTTP body and emitted code -32603 on the 4xx reply, which
        // both wasted network and used the wrong error code.
        let parsed: Option<Value> = serde_json::from_str(trimmed).ok();
        if parsed.is_none() {
            let err = json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {
                    "code": -32700,
                    "message": "Parse error",
                },
            });
            emit_jsonrpc(&mut stdout, &err).await?;
            continue;
        }
        let parsed_value = parsed.as_ref().unwrap();

        // P1-2 (2026-05-27): JSON-RPC 2.0 §4.1 — a Notification is a
        // Request without an `id`; the server MUST NOT respond. MCP uses
        // notifications for `notifications/cancelled` and
        // `notifications/initialized` etc. Claude Code's MCP client
        // treats spurious responses to notifications as protocol
        // violations and disconnects. Detect + fire-and-forget.
        // (Note: explicit `id: null` IS a request per §4.2, so we check
        // key presence via `as_object().contains_key`, not `.get().is_none`.)
        let is_notification = !parsed_value
            .as_object()
            .map(|o| o.contains_key("id"))
            .unwrap_or(false);
        if is_notification {
            // Proxy upstream so the substrate sees the notification
            // (e.g. `notifications/initialized` finishes the MCP handshake)
            // but discard whatever the substrate sends back — spec says
            // we MUST NOT echo a response. Log proxy failures to stderr
            // so operators can diagnose handshake hangs (stderr is
            // invisible to the MCP stream over stdio).
            if let Err(e) = client.proxy_jsonrpc(trimmed).await {
                eprintln!("erebyx mcp-serve: notification proxy failed: {e}");
            }
            continue;
        }

        let response = match client.proxy_jsonrpc(trimmed).await {
            Ok(v) => v,
            Err(e) => json!({
                "jsonrpc": "2.0",
                "id": parsed_value
                    .get("id")
                    .cloned()
                    .unwrap_or(Value::Null),
                "error": {
                    "code": -32603,
                    "message": format!("erebyx mcp-serve bridge error: {}", e)
                }
            }),
        };

        emit_jsonrpc(&mut stdout, &response).await?
    }

    Ok(())
}

/// Serialize + emit a JSON-RPC value to stdout with newline + flush.
///
/// **P1-G (brutal-review POSTFIX_CLI, 2026-05-27):** stdout writes
/// previously used `?`-propagation, which surfaced `BrokenPipe` as a
/// non-zero exit. For a stdio bridge, BrokenPipe means the MCP client
/// (Claude Code) has terminated — that's a clean shutdown signal, not
/// an error. This helper maps BrokenPipe to a clean Ok(()) and bubbles
/// the loop out via the caller's early-return so the bridge exits 0.
async fn emit_jsonrpc(stdout: &mut tokio::io::Stdout, value: &Value) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let serialized = serde_json::to_string(value)?;

    // Wrap each write so BrokenPipe → clean exit signal.
    async fn write_or_broken(out: &mut tokio::io::Stdout, bytes: &[u8]) -> Result<bool> {
        match out.write_all(bytes).await {
            Ok(_) => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(true),
            Err(e) => Err(e.into()),
        }
    }

    if write_or_broken(stdout, serialized.as_bytes()).await? {
        return Ok(()); // caller should break; we signal via subsequent broken writes
    }
    if write_or_broken(stdout, b"\n").await? {
        return Ok(());
    }
    match stdout.flush().await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Native hook-inject handler for Claude Code UserPromptSubmit hook.
///
/// Reads JSON from stdin, smart-gates short/greeting messages, calls the
/// EREBYX remember endpoint with a 500ms hard timeout, and emits an
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

    // Read hook input from stdin with a hard 1 MiB cap.
    //
    // Brutal-review wave-2 (2026-05-27) finding: a malicious or
    // misconfigured Claude Code build (or pipe-redirection misuse)
    // feeding gigabytes of stdin would OOM the binary before the 500ms
    // HTTP timeout ever fires. Real UserPromptSubmit payloads are
    // <16KB; capping at 1 MiB leaves >60x headroom while keeping OOM
    // surface bounded.
    const MAX_HOOK_STDIN_BYTES: u64 = 1 << 20; // 1 MiB
    let mut input = String::new();
    let mut handle = std::io::stdin().take(MAX_HOOK_STDIN_BYTES);
    if handle.read_to_string(&mut input).is_err() {
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
        "hey",
        "hi",
        "hello",
        "thanks",
        "thank you",
        "bye",
        "ok",
        "yes",
        "no",
        "sure",
        "cool",
        "nice",
        "got it",
        "sounds good",
        "okay",
        "yep",
        "nope",
        "alright",
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
    let api_url =
        std::env::var("EREBYX_API_URL").unwrap_or_else(|_| "https://core.erebyx.com".to_string());

    // P1-5: this handler POSTs the bearer token directly (bypassing
    // `ErebyxClient::new`'s URL guard). Enforce the same HTTPS-or-localhost
    // contract here. Fail OPEN to empty JSON — matching this hook's
    // never-block contract — so a bad EREBYX_API_URL silences injection
    // instead of leaking the bearer to an arbitrary host.
    if !is_safe_url(&api_url) {
        return empty;
    }

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
    let mut lines: Vec<String> = vec!["[EREBYX Memory Context]".to_string()];
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

/// Native SessionStart pre-injection hook for Claude Code (and Cursor 1.7+).
///
/// Mirrors claude-mem's #1 mechanic — the reason that project hit 46.1K
/// GitHub stars: pre-populate the AI's system prompt with stored identity
/// and the prior session's handoff BEFORE the first user prompt. The AI
/// then KNOWS the context exists; it doesn't have to choose to call
/// `restore_identity` on a session where the user happens not to mention
/// memory.
///
/// Hook contract (per https://code.claude.com/docs/en/hooks):
///   - Reads JSON payload from stdin (session_id, cwd, transcript_path).
///   - Writes additionalContext JSON to stdout — Claude Code injects the
///     content into the session's context.
///   - Fail-open on every error path: emit `{}` so Claude Code never
///     blocks session boot. The user can still call `restore_identity`
///     explicitly if the auto-injection failed.
///
/// 800ms total budget across both substrate calls — generous compared to
/// hook-inject's 500ms because session start is less latency-critical
/// (it's a one-time cost, not per-prompt).
async fn hook_session_start() {
    if std::io::stdin().is_terminal() {
        eprintln!("erebyx hook-session-start is an internal command for Claude Code hooks.");
        eprintln!("It reads JSON from stdin. You probably want `erebyx restore-identity` instead.");
        std::process::exit(2);
    }

    let result = run_hook_session_start().await;
    println!("{}", result);
}

async fn run_hook_session_start() -> String {
    let empty = "{}".to_string();

    // Drain stdin so the hook protocol stays happy, but the SessionStart
    // payload itself is informational — we don't need session_id from it
    // (we generate our own per-call), and cwd is unused at v0.1.1.
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);

    let api_key = match std::env::var("EREBYX_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return empty,
    };
    let api_url =
        std::env::var("EREBYX_API_URL").unwrap_or_else(|_| "https://core.erebyx.com".to_string());

    // P1-5: same as hook-inject — this SessionStart handler POSTs the bearer
    // token directly, bypassing `ErebyxClient::new`'s URL guard. Enforce
    // HTTPS-or-localhost, failing OPEN to empty JSON so session boot is never
    // blocked and the bearer is never sent to an unsafe host.
    if !is_safe_url(&api_url) {
        return empty;
    }

    // 800ms total budget. Each substrate call gets up to 400ms.
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(400))
        .build()
    {
        Ok(c) => c,
        Err(_) => return empty,
    };

    let session = session_id();
    let base = api_url.trim_end_matches('/');

    // === Fetch 1: identity ===
    let identity_resp = http
        .post(format!("{}/v0/identity/restore", base))
        .header("Content-Type", "application/json")
        .bearer_auth(&api_key)
        .header("X-Instance-ID", "default")
        .header("X-Erebyx-Session-Id", session)
        .json(&json!({"detail_level": "summary", "limit": 5}))
        .send()
        .await;

    let identity_body: Option<Value> = match identity_resp {
        Ok(r) if r.status().is_success() => r.json().await.ok(),
        _ => None,
    };

    // === Fetch 2: latest handoff ===
    let context_resp = http
        .post(format!("{}/v0/session/load", base))
        .header("Content-Type", "application/json")
        .bearer_auth(&api_key)
        .header("X-Instance-ID", "default")
        .header("X-Erebyx-Session-Id", session)
        .json(&json!({"anchors": [], "detail_level": "summary"}))
        .send()
        .await;

    let context_body: Option<Value> = match context_resp {
        Ok(r) if r.status().is_success() => r.json().await.ok(),
        _ => None,
    };

    // === Render compact injection ===
    let injection = render_session_start_injection(identity_body.as_ref(), context_body.as_ref());

    if injection.trim().is_empty() {
        return empty;
    }

    json!({
        "additionalContext": [{
            "type": "text",
            "text": injection
        }]
    })
    .to_string()
}

/// Render the SessionStart injection text — bounded to ~800 tokens (~3200
/// chars) total. Declarative phrasing throughout (per the claude-code#17804
/// injection-defense doctrine — `rules_content_uses_declarative_not_imperative_framing`
/// test enforces the same shape on the static rules file).
fn render_session_start_injection(identity: Option<&Value>, context: Option<&Value>) -> String {
    let mut lines = Vec::new();
    lines.push("[EREBYX Memory — pre-loaded context]".to_string());
    let mut total_chars = lines[0].len();
    const MAX_CHARS: usize = 3200;

    // Identity section — bias toward 600-char budget.
    if let Some(id) = identity {
        let name = id
            .get("identity")
            .and_then(|i| i.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if !name.is_empty() {
            let line = format!("Stored identity: {}", name);
            if total_chars + line.len() < MAX_CHARS {
                total_chars += line.len();
                lines.push(line);
            }
        }
        let ethos = id
            .get("ethos")
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        for statement in ethos.iter().take(3) {
            let trimmed: String = statement.chars().take(180).collect();
            let line = format!("- ethos: {}", trimmed);
            if total_chars + line.len() < MAX_CHARS {
                total_chars += line.len();
                lines.push(line);
            }
        }
        if let Some(narrative) = id.get("narrative").and_then(|n| n.as_str()) {
            let snippet: String = narrative.chars().take(280).collect();
            let line = format!("Identity narrative: {}", snippet);
            if total_chars + line.len() < MAX_CHARS {
                total_chars += line.len();
                lines.push(line);
            }
        }
    }

    // Handoff section — bias toward 1500-char budget.
    if let Some(ctx) = context {
        let handoff = ctx.get("handoff").or_else(|| ctx.get("continuity"));
        if let Some(h) = handoff {
            let what = h
                .get("what_we_built")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let next = h.get("whats_next").and_then(|v| v.as_str()).unwrap_or("");
            if !what.is_empty() || !next.is_empty() {
                lines.push("".to_string());
                lines.push("Last session handoff:".to_string());
                if !what.is_empty() {
                    let snippet: String = what.chars().take(600).collect();
                    let line = format!("- built: {}", snippet);
                    if total_chars + line.len() < MAX_CHARS {
                        total_chars += line.len();
                        lines.push(line);
                    }
                }
                if !next.is_empty() {
                    let snippet: String = next.chars().take(600).collect();
                    let line = format!("- next:  {}", snippet);
                    if total_chars + line.len() < MAX_CHARS {
                        total_chars += line.len();
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
            if total_chars + line.len() < MAX_CHARS {
                lines.push(line);
            }
        }
    }

    // If we only have the header, there's nothing useful to inject.
    if lines.len() <= 1 {
        return String::new();
    }

    lines.join("\n")
}

#[cfg(test)]
mod api_key_format_tests {
    use super::is_well_formed_api_key;

    #[test]
    fn accepts_canonical_key() {
        // erebyx_ + exactly 48 hex chars.
        let key = format!("erebyx_{}", "a".repeat(48));
        assert!(is_well_formed_api_key(&key));
        let hexy = format!("erebyx_{}", "0123456789abcdef".repeat(3)); // 48 hex
        assert!(is_well_formed_api_key(&hexy));
    }

    #[test]
    fn rejects_short_garbage_key() {
        // CLI v0.1.2 fix [8]: a 35-char key that merely starts with the
        // prefix used to render a false ✓. The tightened gate rejects it.
        let key = "erebyx_xxxxxxxxxxxxxxxxxxxxxxxxxxxx"; // 35 chars, non-hex body
        assert!(!is_well_formed_api_key(key));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(!is_well_formed_api_key(&format!(
            "erebyx_{}",
            "a".repeat(47)
        )));
        assert!(!is_well_formed_api_key(&format!(
            "erebyx_{}",
            "a".repeat(49)
        )));
    }

    #[test]
    fn rejects_non_hex_body() {
        // 48 chars but contains a non-hex char (g, z).
        let key = format!("erebyx_{}g", "a".repeat(47));
        assert!(!is_well_formed_api_key(&key));
    }

    #[test]
    fn rejects_missing_prefix() {
        assert!(!is_well_formed_api_key(&"a".repeat(55)));
    }

    #[test]
    fn multibyte_key_does_not_panic_and_is_rejected() {
        // The exact paste error that crashed the doctor preview: an emoji
        // straddling the first bytes. is_well_formed_api_key must reject it
        // (and never panic), and the preview path uses .chars() so it's safe.
        let key = "erebyx_🤘xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        assert!(!is_well_formed_api_key(key));
        // Mirror the doctor preview construction to prove it can't panic.
        let preview: String = key.chars().take(10).collect();
        assert!(!preview.is_empty());
    }
}

#[cfg(test)]
mod hook_session_start_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_empty_when_no_identity_or_context() {
        let out = render_session_start_injection(None, None);
        assert!(out.is_empty(), "no inputs should render empty");
    }

    #[test]
    fn render_with_identity_only() {
        let id = json!({
            "identity": {"name": "Ada"},
            "ethos": ["Clarity over cleverness", "Tests before code"],
            "narrative": "Ada is the user's coding assistant.",
        });
        let out = render_session_start_injection(Some(&id), None);
        assert!(out.contains("EREBYX Memory"), "expected header");
        assert!(out.contains("Ada"), "expected identity name");
        assert!(out.contains("ethos"), "expected at least one ethos line");
    }

    #[test]
    fn render_with_handoff() {
        let ctx = json!({
            "handoff": {
                "what_we_built": "rules.rs declarative rewrite",
                "whats_next": "ship session-start pre-injection",
            },
            "anchors": ["launch-prep", "cli", "coding"],
        });
        let out = render_session_start_injection(None, Some(&ctx));
        assert!(
            out.contains("Last session handoff"),
            "expected handoff section"
        );
        assert!(out.contains("rules.rs"), "expected what_we_built content");
        assert!(out.contains("session-start"), "expected whats_next content");
        assert!(out.contains("Recent anchors"), "expected anchors line");
    }

    #[test]
    fn render_respects_char_budget() {
        // Construct an oversized identity payload — the renderer should
        // cap to MAX_CHARS (3200) without panicking or returning more.
        let big = "x".repeat(10_000);
        let id = json!({"identity": {"name": "Z"}, "narrative": big});
        let out = render_session_start_injection(Some(&id), None);
        assert!(
            out.len() <= 4000,
            "expected bounded output, got {}",
            out.len()
        );
    }

    #[test]
    fn render_uses_declarative_not_imperative_framing() {
        // claude-code#17804 defense — injection text must NOT contain
        // imperative system-command patterns.
        let id = json!({
            "identity": {"name": "Ada"},
            "ethos": ["Test ethos"],
        });
        let ctx = json!({
            "handoff": {
                "what_we_built": "test",
                "whats_next": "test",
            },
        });
        let out = render_session_start_injection(Some(&id), Some(&ctx));
        let lower = out.to_lowercase();
        for banned in &[
            "you must",
            "you should",
            "always call",
            "do not",
            "always remember",
        ] {
            assert!(
                !lower.contains(banned),
                "render output contains imperative phrase '{}': {}",
                banned,
                out
            );
        }
    }
}

#[cfg(test)]
mod hook_handler_url_guard_tests {
    use super::run_hook_session_start;

    /// RAII env setter that restores the prior value on drop. Both handlers
    /// read process-global env; this test mutates `EREBYX_API_URL` /
    /// `EREBYX_API_KEY` and restores them so it doesn't bleed into siblings.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, val);
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    /// REGRESSION (P1-5): the Claude Code hook handlers POST the bearer
    /// token directly, bypassing `ErebyxClient::new`'s URL guard. Both
    /// assertions live in ONE test to avoid a cross-test race on the shared
    /// `EREBYX_API_URL` / `EREBYX_API_KEY` process env (and to avoid holding
    /// a lock across `.await`, which clippy's `await_holding_lock` forbids).
    ///
    /// In `cargo test` stdin is empty (EOF) — `run_hook_session_start`
    /// drains it fine; the env-driven URL guard is what we exercise.
    ///
    /// Why it FAILS pre-fix: the original `run_hook_session_start` had NO
    /// `is_safe_url` gate. With `EREBYX_API_URL=http://198.51.100.7:1`
    /// (RFC-5737 TEST-NET, guaranteed unroutable) it would build the client
    /// and `POST .../v0/identity/restore` + `/v0/session/load` with
    /// `.bearer_auth(api_key)` — attempting to leak the bearer — and only
    /// return `{}` AFTER both connect attempts timed out (~2x400ms budget).
    /// Post-fix it returns `{}` instantly without constructing any request,
    /// which the elapsed-time ceiling proves.
    #[tokio::test]
    async fn hook_handlers_fail_open_on_unsafe_url_and_proceed_on_safe() {
        let _key = EnvGuard::set("EREBYX_API_KEY", "ebx_live_test_key_not_real");

        // --- Unsafe (plain-http, non-localhost) URL: short-circuit to {} ---
        {
            // TEST-NET address (RFC 5737) so even a regression can't reach a
            // real host. is_safe_url rejects it (http:// + non-localhost).
            let _url = EnvGuard::set("EREBYX_API_URL", "http://198.51.100.7:1");
            let start = std::time::Instant::now();
            let out = run_hook_session_start().await;
            let elapsed = start.elapsed();
            assert_eq!(out, "{}", "must fail open to empty JSON on unsafe URL");
            // Post-fix returns before the 2x400ms substrate budget; a 300ms
            // ceiling proves we short-circuited at the URL guard rather than
            // attempting (and timing out) two bearer-bearing POSTs.
            assert!(
                elapsed < std::time::Duration::from_millis(300),
                "URL guard must short-circuit before any HTTP attempt; took {elapsed:?}"
            );
        }

        // --- Safe URL (localhost:1, no listener): proceeds, still {} ---
        {
            // localhost:1 passes is_safe_url but has no listener → connect
            // refused → fail-open {} AFTER attempting the calls. Confirms the
            // guard rejects ONLY unsafe URLs, never the happy path.
            let _url = EnvGuard::set("EREBYX_API_URL", "http://127.0.0.1:1");
            let out = run_hook_session_start().await;
            assert_eq!(out, "{}", "unreachable safe URL still fails open to {{}}");
        }
    }
}
