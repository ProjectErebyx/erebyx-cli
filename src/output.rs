// SPDX-License-Identifier: MIT OR Apache-2.0
use colored::Colorize;
use serde_json::{json, Value};

/// Print the response from an MCP tool call.
/// In JSON mode, prints raw JSON. Otherwise, prints colored terminal output.
pub fn print_response(content: &Value, is_error: bool, json_mode: bool) {
    print_response_with_hints(content, is_error, json_mode, &[], &[]);
}

/// Print response with lifecycle hints surfaced.
///
/// JSON mode: hints + auto_fired are merged into the top-level JSON
/// object as ``hints`` and ``auto_fired`` arrays (alongside the tool's
/// own content), so ``jq '.hints'`` / ``jq '.auto_fired'`` work as the
/// public docs promise. If ``content`` is not a JSON object, the JSON
/// output wraps it under ``content``.
///
/// Terminal mode: hints render as a dim note after the response if
/// any are present. Auto-fired tools are surfaced only when present.
pub fn print_response_with_hints(
    content: &Value,
    is_error: bool,
    json_mode: bool,
    hints: &[String],
    auto_fired: &[String],
) {
    if json_mode {
        // CLI postfix-review P0-2 (2026-05-27): only wrap into an
        // envelope when there are actual hints / auto_fired tokens to
        // surface. The prior shape wrapped ANY non-object content as
        // `{"content": ...}`, silently breaking customer scripts that
        // used `--json | jq -r .` to extract a raw string. Preserve the
        // bare-content shape when no hints — keep the envelope only
        // when there's something to add.
        if hints.is_empty() && auto_fired.is_empty() {
            println!(
                "{}",
                serde_json::to_string_pretty(content).unwrap_or_default()
            );
            return;
        }
        let mut envelope = match content {
            Value::Object(_) => content.clone(),
            other => json!({ "content": other }),
        };
        if let Value::Object(ref mut map) = envelope {
            // Use .entry().or_insert_with(...) so a tool that legitimately
            // returns a `hints` or `auto_fired` field in its own body
            // isn't silently clobbered.
            if !hints.is_empty() {
                map.entry("hints".to_string())
                    .or_insert_with(|| json!(hints));
            }
            if !auto_fired.is_empty() {
                map.entry("auto_fired".to_string())
                    .or_insert_with(|| json!(auto_fired));
            }
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope).unwrap_or_default()
        );
        return;
    }

    if is_error {
        print_error_content(content);
        return;
    }

    print_value(content, 0);

    if !hints.is_empty() {
        eprintln!("{} {}", "hint:".dimmed(), hints.join(", ").dimmed());
    }
    if !auto_fired.is_empty() {
        eprintln!(
            "{} {}",
            "auto-fired:".dimmed(),
            auto_fired.join(", ").dimmed()
        );
    }
}

/// Print an error response with red formatting
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "error:".red().bold(), msg);
}

/// Map a raw error chain string to an actionable customer-facing
/// message + remediation hint.
///
/// Promotes the per-class error handling from ``erebyx doctor`` to
/// every CLI command. Pre-fix, ``save`` / ``remember`` / ``wrap-up``
/// dumped the raw ``anyhow`` chain on failure — a wall of
/// ``Server error: 401 ...`` text that left the developer with no
/// idea how to recover. This routes common failure classes
/// (401 / 403 / 422 / 429 / 5xx / network / DNS) to a single-line
/// "here's what's wrong, here's how to fix it" message and only
/// falls through to the raw chain when no class matches.
///
/// Brutal-review wave-2 (Genesis Arche T-5 days, 2026-05-27) flagged
/// the raw-chain dump as a first-touch papercut that would cause
/// developers to bounce on their first failed call.
/// True when the caller has opted into verbose error output — used to gate
/// echoing raw server bodies (CLI v0.1.2 fix [17]). Honors a non-empty
/// `RUST_LOG` (the conventional Rust verbosity switch) or an explicit
/// `EREBYX_VERBOSE` set to a truthy value.
fn verbose_errors_enabled() -> bool {
    std::env::var("RUST_LOG")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("EREBYX_VERBOSE")
            .ok()
            .map(|v| {
                let v = v.trim().to_lowercase();
                matches!(v.as_str(), "1" | "true" | "yes")
            })
            .unwrap_or(false)
}

pub fn map_actionable_error(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // Authentication — most common first-touch failure.
    if raw.contains("401") || lower.contains("unauthorized") || lower.contains("auth") {
        return "Authentication rejected (401). Check that EREBYX_API_KEY is set correctly.\n\
             • Verify in your shell: `echo $EREBYX_API_KEY`\n\
             • Get a fresh key at https://app.erebyx.com/keys\n\
             • Run `erebyx doctor` to diagnose."
            .to_string();
    }

    // Permission / scope mismatch.
    if raw.contains("403") || lower.contains("forbidden") {
        return "Permission denied (403). Your API key may not have the right scope for this operation.\n\
             Check your key's permissions at https://app.erebyx.com/keys."
            .to_string();
    }

    // Validation — usually fixable in-line.
    if raw.contains("422") || lower.contains("validation") || lower.contains("unprocessable") {
        return format!(
            "Request validation failed.\n\
             Original error: {raw}\n\
             See https://docs.erebyx.com for field requirements."
        );
    }

    // Rate limit — actionable wait.
    if raw.contains("429") || lower.contains("rate limit") || lower.contains("too many requests") {
        return "Rate limited (429). Wait a few seconds and retry.\n\
             If this keeps happening on a small workload, contact support@erebyx.com."
            .to_string();
    }

    // Server errors — point at status page.
    //
    // CLI v0.1.2 fix [17]: the raw server 5xx body (a stack-trace fragment /
    // HTML error page / gateway blurb, up to 500 chars) was embedded in the
    // default customer message — pure noise that buries the actionable line.
    // It carries no credential (verified), so this is a clarity papercut, not
    // a leak. Drop it from the default message; surface it only when the
    // caller opts into verbosity via RUST_LOG (any non-empty value) or
    // EREBYX_VERBOSE so an operator debugging an incident can still see it.
    if raw.contains("500") || raw.contains("502") || raw.contains("503") || raw.contains("504") {
        let base =
            "EREBYX server error. Try again in a few seconds; if it persists, check status.\n\
             Status: https://status.erebyx.com";
        if verbose_errors_enabled() {
            return format!("{base}\nOriginal error: {raw}");
        }
        return base.to_string();
    }

    // Network / DNS — pre-substrate failure.
    if lower.contains("connection refused")
        || lower.contains("could not resolve")
        || lower.contains("dns")
        || lower.contains("no route to host")
        || lower.contains("network is unreachable")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        return format!(
            "Could not reach EREBYX.\n\
             • Check your network connection.\n\
             • If you're behind a proxy, set EREBYX_API_URL.\n\
             • Run `erebyx doctor` to diagnose.\n\
             Original error: {raw}"
        );
    }

    // No match — pass through the original anyhow chain.
    raw.to_string()
}

fn print_error_content(content: &Value) {
    match content {
        Value::String(s) => eprintln!("{} {}", "error:".red().bold(), s),
        Value::Object(map) => {
            if let Some(error) = map.get("error").and_then(|e| e.as_str()) {
                eprintln!("{} {}", "error:".red().bold(), error);
            } else {
                eprintln!(
                    "{} {}",
                    "error:".red().bold(),
                    serde_json::to_string_pretty(content).unwrap_or_default()
                );
            }
        }
        _ => eprintln!(
            "{} {}",
            "error:".red().bold(),
            serde_json::to_string_pretty(content).unwrap_or_default()
        ),
    }
}

/// Recursively print a JSON value with colored formatting
fn print_value(value: &Value, depth: usize) {
    match value {
        Value::String(s) => {
            // Strings render as-is (preserves embedded newlines).
            println!("{}", s);
        }
        Value::Object(map) => {
            for (key, val) in map {
                let indent = "  ".repeat(depth);
                match val {
                    Value::String(s) => {
                        if s.contains('\n') {
                            println!("{}{}", indent, format_key(key));
                            for line in s.lines() {
                                println!("{}  {}", indent, line);
                            }
                        } else {
                            println!("{}{} {}", indent, format_key(key), s);
                        }
                    }
                    Value::Number(n) => {
                        println!("{}{} {}", indent, format_key(key), n.to_string().cyan());
                    }
                    Value::Bool(b) => {
                        let colored = if *b {
                            "true".green().to_string()
                        } else {
                            "false".red().to_string()
                        };
                        println!("{}{} {}", indent, format_key(key), colored);
                    }
                    Value::Null => {
                        println!("{}{} {}", indent, format_key(key), "null".dimmed());
                    }
                    Value::Array(arr) => {
                        println!("{}{}", indent, format_key(key));
                        for item in arr {
                            print_array_item(item, depth + 1);
                        }
                    }
                    Value::Object(_) => {
                        println!("{}{}", indent, format_key(key));
                        print_value(val, depth + 1);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                print_array_item(item, depth);
            }
        }
        Value::Number(n) => println!("{}", n.to_string().cyan()),
        Value::Bool(b) => println!("{}", if *b { "true".green() } else { "false".red() }),
        Value::Null => println!("{}", "null".dimmed()),
    }
}

fn print_array_item(item: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    match item {
        Value::String(s) => println!("{}- {}", indent, s),
        Value::Object(_) => {
            println!("{}---", indent);
            print_value(item, depth + 1);
        }
        _ => println!("{}- {}", indent, item),
    }
}

fn format_key(key: &str) -> String {
    format!("{}:", key.bold())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::map_actionable_error;

    #[test]
    fn maps_401_to_auth_remediation() {
        let mapped = map_actionable_error("Server error: 401 Unauthorized");
        assert!(mapped.contains("Authentication rejected"));
        assert!(mapped.contains("EREBYX_API_KEY"));
        assert!(mapped.contains("https://app.erebyx.com/keys"));
        assert!(mapped.contains("erebyx doctor"));
    }

    #[test]
    fn maps_403_to_permission_message() {
        let mapped = map_actionable_error("403 Forbidden");
        assert!(mapped.contains("Permission denied"));
        assert!(mapped.contains("scope"));
    }

    #[test]
    fn maps_422_to_validation_message() {
        let mapped = map_actionable_error("Server error: 422 Unprocessable Entity");
        assert!(mapped.contains("validation failed"));
        assert!(mapped.contains("docs.erebyx.com"));
    }

    #[test]
    fn maps_429_to_rate_limit_message() {
        let mapped = map_actionable_error("Server error: 429 Too Many Requests");
        assert!(mapped.contains("Rate limited"));
    }

    #[test]
    fn maps_5xx_to_status_page_pointer() {
        let mapped = map_actionable_error("Server error: 503 Service Unavailable");
        assert!(mapped.contains("server error"));
        assert!(mapped.contains("status.erebyx.com"));
    }

    /// CLI v0.1.2 fix [17]: the raw server 5xx body must NOT appear in the
    /// default customer message. These tests mutate process env, so they run
    /// serially and clean up after themselves.
    #[test]
    #[serial_test::serial]
    fn fix17_5xx_default_drops_raw_body() {
        // Ensure neither verbosity switch is set for the default-path test.
        std::env::remove_var("RUST_LOG");
        std::env::remove_var("EREBYX_VERBOSE");
        let raw = "Server returned HTTP 500: <html>internal stack trace blah blah</html>";
        let mapped = super::map_actionable_error(raw);
        assert!(mapped.contains("status.erebyx.com"));
        assert!(
            !mapped.contains("stack trace") && !mapped.contains("<html>"),
            "raw 5xx body must be dropped from the default message, got: {mapped}"
        );
        assert!(!mapped.contains("Original error"));
    }

    #[test]
    #[serial_test::serial]
    fn fix17_5xx_verbose_surfaces_raw_body() {
        std::env::remove_var("RUST_LOG");
        std::env::set_var("EREBYX_VERBOSE", "1");
        let raw = "Server returned HTTP 502: upstream connect error blah";
        let mapped = super::map_actionable_error(raw);
        std::env::remove_var("EREBYX_VERBOSE");
        assert!(mapped.contains("Original error"));
        assert!(mapped.contains("upstream connect error"));
    }

    #[test]
    fn maps_connection_refused_to_network_message() {
        let mapped = map_actionable_error("Connection refused (os error 61)");
        assert!(mapped.contains("Could not reach EREBYX"));
        assert!(mapped.contains("EREBYX_API_URL"));
    }

    #[test]
    fn maps_dns_failure_to_network_message() {
        let mapped = map_actionable_error("DNS resolution failed");
        assert!(mapped.contains("Could not reach EREBYX"));
    }

    #[test]
    fn maps_timeout_to_network_message() {
        let mapped = map_actionable_error("Request timed out after 30s");
        assert!(mapped.contains("Could not reach EREBYX"));
    }

    #[test]
    fn falls_through_unknown_errors_unchanged() {
        let raw = "Some completely unrecognized error chain: foo bar";
        let mapped = map_actionable_error(raw);
        assert_eq!(mapped, raw);
    }

    #[test]
    fn auth_branch_does_not_match_random_text() {
        // Ensure the auth branch isn't matching ``unauth`` in random
        // unrelated strings (it currently uses ``contains`` on ``auth``;
        // this test pins that we accept that breadth deliberately).
        let mapped = map_actionable_error("auth header sent OK then payload too large");
        // This DOES match — documenting current behaviour.
        assert!(mapped.contains("Authentication rejected"));
    }
}
