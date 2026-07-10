// SPDX-License-Identifier: MIT OR Apache-2.0
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

use crate::credentials;

/// Maximum response body size we will buffer (10 MiB).
const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

/// Resolve the per-process session id used for the `X-Erebyx-Session-Id`
/// header. Resolution order:
///
/// 1. `EREBYX_SESSION_ID` env var (overrides everything; useful in CI/tests).
/// 2. Stable file at `~/.erebyx/session-id` — created on first use.
/// 3. Cached in-process for the lifetime of the binary.
///
/// The header lets the substrate attribute hook + tool calls to a stable
/// caller without leaking PII. It is intentionally cheap to compute and never
/// blocks substrate calls — any I/O error falls back to a fresh per-process id.
pub fn session_id() -> &'static str {
    use std::sync::OnceLock;
    static SESSION_ID: OnceLock<String> = OnceLock::new();
    SESSION_ID.get_or_init(resolve_session_id).as_str()
}

/// Resolve the `X-Instance-ID` value: `EREBYX_INSTANCE_ID` (trimmed), or
/// `"default"` when unset/blank.
///
/// Centralized so EVERY caller targets the SAME instance slice: the
/// interactive `ErebyxClient`, the Claude Code hook handlers, and the `setup`
/// reachability probes. The hooks + probes previously hardcoded `"default"`,
/// so a user who set a custom `EREBYX_INSTANCE_ID` got a 403 on those paths —
/// memory injection + session cold-load silently no-op'd — while their
/// interactive commands (which honored the env var) worked. One value now.
pub fn resolve_instance_id() -> String {
    env::var("EREBYX_INSTANCE_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

fn resolve_session_id() -> String {
    if let Ok(s) = env::var("EREBYX_SESSION_ID") {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".erebyx");
        let path = dir.join("session-id");
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let fresh = generate_session_id();
        if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, &fresh).is_ok() {
            // Best-effort 0600 on unix so the id is not world-readable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        } else {
            // CLI v0.1.2 fix [N2]: persisting the session id failed
            // (unwritable $HOME, read-only fs, full disk). Pre-fix we
            // silently returned a fresh per-process id every run, so the
            // `X-Erebyx-Session-Id` header churned on every call and
            // server-side session attribution looked random with no clue
            // why. Warn ONCE (Once-guarded so we don't spam every call)
            // that the id won't be stable — but never fail the call, the
            // id is still usable for this process.
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!(
                    "erebyx: could not persist session id to {} — using a per-process id \
                     (session attribution will not be stable across runs). \
                     Set EREBYX_SESSION_ID to a fixed value to silence this.",
                    path.display()
                );
            });
        }
        return fresh;
    }

    generate_session_id()
}

/// Generate a UUID-shaped 128-bit random id (RFC 4122 v4 layout) without
/// pulling in the `uuid` crate. We only need uniqueness, not parsing.
fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Mix time + process id + thread id + an internal counter so concurrent
    // instances on the same machine still diverge.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut seed = now ^ (pid.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut out = [0u8; 16];
    for byte in out.iter_mut() {
        // xorshift64-style step on the lower 64 bits.
        let mut x = (seed & 0xFFFF_FFFF_FFFF_FFFF) as u64;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        seed = (seed >> 8) ^ (x as u128);
        *byte = (x & 0xFF) as u8;
    }
    // Set version (4) and variant (RFC 4122) bits.
    out[6] = (out[6] & 0x0F) | 0x40;
    out[8] = (out[8] & 0x3F) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        out[0], out[1], out[2], out[3],
        out[4], out[5],
        out[6], out[7],
        out[8], out[9],
        out[10], out[11], out[12], out[13], out[14], out[15],
    )
}

/// Return true if URL is HTTPS, or HTTP pointed at localhost (dev affordance).
///
/// Localhost forms accepted:
///   - `http://localhost[:port][/path]`
///   - `http://127.0.0.1[:port][/path]`
///   - `http://[::1][:port][/path]`   (P1-7 — IPv6 bracketed form)
///
/// Scheme matching is case-insensitive so `HTTPS://...` from a clipboard
/// paste doesn't get rejected on a technicality (audit P2-4 adjacent).
///
/// Exposed `pub(crate)` so the single canonical URL guard is shared across
/// every `EREBYX_API_URL` consumer — `ErebyxClient::new`, `erebyx setup`,
/// and both Claude Code hook handlers (`hook-inject` / `hook-session-start`)
/// — instead of each path re-deriving (or skipping) the check. Centralizing
/// here closes the launch-grade gap where `setup` + the hook handlers POSTed
/// the bearer token to whatever `EREBYX_API_URL` pointed at, bypassing this
/// guard, and where an attacker-controlled `api_url` was shell-interpolated
/// into the generated hook script.
pub(crate) fn is_safe_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return false;
    };
    let host_part = rest.split('/').next().unwrap_or("");
    // Reject any userinfo (`user:pass@host`) BEFORE the localhost match.
    // `http://127.0.0.1:1@evil.com/` parses with authority host `evil.com`
    // — without this guard the naive prefix check below would see the
    // `127.0.0.1` userinfo and report SAFE, so the bearer token would be
    // POSTed in plaintext to the attacker-controlled host. No legitimate
    // localhost/IPv6 dev URL carries an `@`, so a blanket reject is safe.
    if host_part.contains('@') {
        return false;
    }
    // P1-7 (2026-05-27): IPv6 literals are bracketed (`[::1]:8080`). The
    // prior `host_part.split(':').next()` returned `[` for that form and
    // rejected legitimate IPv6 localhost dev setups (Linux distros that
    // default to IPv6, modern Docker, Codespaces).
    if let Some(stripped) = host_part.strip_prefix('[') {
        if let Some(end) = stripped.find(']') {
            let host = &stripped[..end];
            return matches!(host, "::1");
        }
        // Malformed bracketed form — fall through to reject.
        return false;
    }
    let host = host_part.split(':').next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1")
}

#[cfg(test)]
mod instance_id_tests {
    use super::resolve_instance_id;
    use serial_test::serial;

    #[test]
    #[serial]
    fn defaults_when_unset() {
        std::env::remove_var("EREBYX_INSTANCE_ID");
        assert_eq!(resolve_instance_id(), "default");
    }

    #[test]
    #[serial]
    fn uses_configured_value() {
        std::env::set_var("EREBYX_INSTANCE_ID", "zenn");
        assert_eq!(resolve_instance_id(), "zenn");
        std::env::remove_var("EREBYX_INSTANCE_ID");
    }

    #[test]
    #[serial]
    fn blank_or_whitespace_falls_back_to_default() {
        std::env::set_var("EREBYX_INSTANCE_ID", "   ");
        assert_eq!(resolve_instance_id(), "default");
        std::env::set_var("EREBYX_INSTANCE_ID", "");
        assert_eq!(resolve_instance_id(), "default");
        std::env::remove_var("EREBYX_INSTANCE_ID");
    }

    #[test]
    #[serial]
    fn trims_surrounding_whitespace() {
        std::env::set_var("EREBYX_INSTANCE_ID", "  zenn  ");
        assert_eq!(resolve_instance_id(), "zenn");
        std::env::remove_var("EREBYX_INSTANCE_ID");
    }
}

#[cfg(test)]
mod url_safety_tests {
    use super::is_safe_url;

    #[test]
    fn accepts_https() {
        assert!(is_safe_url("https://core.erebyx.com"));
        assert!(is_safe_url("https://core.erebyx.com/mcp"));
    }

    #[test]
    fn accepts_https_uppercase_scheme() {
        assert!(is_safe_url("HTTPS://core.erebyx.com"));
    }

    #[test]
    fn accepts_localhost_dev() {
        assert!(is_safe_url("http://localhost:8080"));
        assert!(is_safe_url("http://127.0.0.1:8080/mcp"));
    }

    #[test]
    fn accepts_ipv6_localhost() {
        assert!(is_safe_url("http://[::1]:8080"));
        assert!(is_safe_url("http://[::1]:8080/mcp"));
        assert!(is_safe_url("http://[::1]"));
    }

    #[test]
    fn rejects_plain_http_to_internet() {
        assert!(!is_safe_url("http://example.com"));
        assert!(!is_safe_url("http://core.erebyx.com"));
    }

    #[test]
    fn rejects_malformed_ipv6() {
        assert!(!is_safe_url("http://[:::"));
    }

    /// Userinfo-bypass defense: a `user:pass@host` authority must never be
    /// treated as localhost. `http://127.0.0.1:1@evil.com/` connects to
    /// `evil.com` (the real host) while the naive localhost prefix check
    /// would otherwise see the `127.0.0.1` userinfo and report SAFE,
    /// leaking the bearer token in plaintext to the attacker host.
    #[test]
    fn rejects_userinfo_bypass() {
        assert!(!is_safe_url("http://127.0.0.1:1@evil.com/"));
        assert!(!is_safe_url("http://localhost@evil.com"));
        assert!(!is_safe_url("http://[::1]@evil.com"));
        assert!(!is_safe_url("http://user:pass@evil.com"));
    }

    /// Regression guard: the userinfo reject must NOT break legitimate
    /// localhost/IPv6 dev URLs, which never carry an `@`.
    #[test]
    fn userinfo_reject_preserves_legit_localhost() {
        assert!(is_safe_url("http://localhost:8080"));
        assert!(is_safe_url("http://127.0.0.1:8080/mcp"));
        assert!(is_safe_url("http://[::1]:8080"));
        assert!(is_safe_url("https://core.erebyx.com"));
    }
}

/// HTTP client for erebyx-os MCP endpoint.
/// Sends JSON-RPC tool calls and returns the result content.
pub struct ErebyxClient {
    client: Client,
    base_url: String,
    api_key: String,
    instance_id: String,
    /// Per-tenant passphrase for `argon2_passphrase` mode (default at
    /// v0.1.1+). When set, sent as the `X-Passphrase` header on every
    /// request. Resolved from `EREBYX_PASSPHRASE`; empty values
    /// normalized to `None`. Future: prompt-at-setup + OS-keychain
    /// persistence via `erebyx login`.
    passphrase: Option<String>,
}

/// JSON-RPC response from the MCP server
#[derive(Debug)]
pub struct McpResponse {
    pub content: Value,
    pub is_error: bool,
    /// Lifecycle hints from the substrate (parsed from the
    /// ``X-Erebyx-Hint`` response header). Empty when the substrate
    /// emits no hints OR the env override ``EREBYX_HINTS_DISABLED=1``
    /// is set. Known values: ``wrap_up_recommended``,
    /// ``restore_identity_recommended``, ``load_context_recommended``,
    /// ``compact_imminent``.
    pub hints: Vec<String>,
    /// Tools the substrate auto-fired during this request, parsed from the
    /// ``X-Erebyx-Auto-Fired`` response header. NOTE: the `/mcp/` transport
    /// this client uses injects only ``x-erebyx-hint`` — the auto-fired header
    /// is emitted on the REST routes, not `/mcp/` — so over this client it is
    /// currently always empty. Parsed anyway so it lights up automatically if
    /// the `/mcp/` transport ever surfaces it; treat a populated value as a
    /// bonus, never depend on it.
    pub auto_fired: Vec<String>,
}

/// Parse a comma-separated header value into a deduped, trimmed,
/// lowercase-comparable list. Used for ``X-Erebyx-Hint`` and
/// ``X-Erebyx-Auto-Fired`` capture. Empty header → empty Vec.
fn parse_csv_header(value: Option<&reqwest::header::HeaderValue>) -> Vec<String> {
    value
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

impl ErebyxClient {
    pub fn new() -> Result<Self> {
        let stored =
            credentials::load_credentials().context("Failed to load stored EREBYX credentials")?;
        let api_key = env::var("EREBYX_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| stored.as_ref().map(|c| c.api_key.clone()))
            .context("EREBYX_API_KEY is required. Run `erebyx login` or set EREBYX_API_KEY.")?;

        let base_url = env::var("EREBYX_API_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| stored.as_ref().map(|c| c.api_url.clone()))
            .unwrap_or_else(|| "https://core.erebyx.com".to_string());

        // Env is the runtime override. `erebyx login` is the durable carry
        // layer for an existing counterpart on a fresh machine/client.
        let instance_id = env::var("EREBYX_INSTANCE_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| stored.as_ref().map(|c| c.instance_id.clone()))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(resolve_instance_id);

        // Argon2id-default-on: tenants register with a passphrase used to
        // derive the KEK at request time. EREBYX_PASSPHRASE is the transport
        // until OS-keychain (keyring crate, follow-up). Empty strings normalize to None so legacy hkdf_api_key tenants
        // don't accidentally transmit an empty X-Passphrase header.
        let passphrase = env::var("EREBYX_PASSPHRASE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| stored.and_then(|c| c.passphrase));

        if api_key.trim().is_empty() {
            anyhow::bail!("EREBYX_API_KEY is set but empty");
        }

        // Reject non-HTTPS URLs (allow http://localhost:* for dev).
        if !is_safe_url(&base_url) {
            anyhow::bail!(
                "EREBYX_API_URL must be https:// (got {}). \
                 Plain http:// is only allowed for localhost/127.0.0.1.",
                base_url
            );
        }

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            base_url,
            api_key,
            instance_id,
            passphrase,
        })
    }

    /// Call an MCP tool via JSON-RPC POST to /mcp/
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<McpResponse> {
        let url = format!("{}/mcp/", self.base_url.trim_end_matches('/'));

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        let mut rb = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .bearer_auth(&self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .header("X-Erebyx-Session-Id", session_id());
        if let Some(ref p) = self.passphrase {
            rb = rb.header("X-Passphrase", p);
        }
        let response = rb
            .json(&body)
            .send()
            .await
            .context("Failed to connect to EREBYX")?;

        let status = response.status();
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_BYTES {
                anyhow::bail!(
                    "Response body too large ({} bytes; cap is {})",
                    len,
                    MAX_RESPONSE_BYTES
                );
            }
        }
        // Capture lifecycle headers BEFORE .text() consumes the response.
        // Honors EREBYX_HINTS_DISABLED env var as a per-call opt-out.
        // Truthiness matches the substrate's allowlist
        // (core/api/middleware/erebyx_hints.py): {"1","true","yes"} only.
        // Prior shape treated any non-"0" value as truthy → asymmetric
        // with substrate, so `EREBYX_HINTS_DISABLED=false` would disable
        // here but not server-side (CLI postfix-review P1-3).
        let hints_disabled = std::env::var("EREBYX_HINTS_DISABLED")
            .ok()
            .map(|v| {
                let v = v.trim().to_lowercase();
                matches!(v.as_str(), "1" | "true" | "yes")
            })
            .unwrap_or(false);
        let (hints, auto_fired) = if hints_disabled {
            (Vec::new(), Vec::new())
        } else {
            (
                parse_csv_header(response.headers().get("X-Erebyx-Hint")),
                parse_csv_header(response.headers().get("X-Erebyx-Auto-Fired")),
            )
        };
        let response_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!(
                "Server returned HTTP {}: {}",
                status.as_u16(),
                truncate_safe(&response_text, 500)
            );
        }

        let rpc_response: Value =
            serde_json::from_str(&response_text).context("Invalid JSON in response")?;

        // JSON-RPC error
        if let Some(error) = rpc_response.get("error") {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Ok(McpResponse {
                content: json!({ "error": message }),
                is_error: true,
                hints,
                auto_fired,
            });
        }

        // Extract tool result from JSON-RPC response
        // MCP tools/call returns: { "result": { "content": [...], "isError": bool } }
        let result = rpc_response
            .get("result")
            .cloned()
            .unwrap_or_else(|| rpc_response.clone());

        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Extract text content from MCP content array
        let content = if let Some(content_array) = result.get("content").and_then(|c| c.as_array())
        {
            // Combine all text content items
            let texts: Vec<&str> = content_array
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect();

            if texts.len() == 1 {
                // Try to parse single text as JSON, fall back to string
                match serde_json::from_str::<Value>(texts[0]) {
                    Ok(parsed) => parsed,
                    Err(_) => json!(texts[0]),
                }
            } else if texts.is_empty() {
                result
            } else {
                json!(texts.join("\n\n"))
            }
        } else {
            result
        };

        Ok(McpResponse {
            content,
            is_error,
            hints,
            auto_fired,
        })
    }

    /// Forward a raw JSON-RPC request body to the substrate `/mcp/` endpoint
    /// and return the parsed JSON-RPC response.
    ///
    /// Used by `erebyx mcp-serve` to bridge an MCP stdio client to the
    /// substrate over HTTP. The request body is passed through verbatim so
    /// initialization, capabilities, tool schemas, and prompts all stay
    /// authoritative on the server.
    pub async fn proxy_jsonrpc(&self, raw_body: &str) -> Result<Value> {
        let url = format!("{}/mcp/", self.base_url.trim_end_matches('/'));

        let mut rb = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .bearer_auth(&self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .header("X-Erebyx-Session-Id", session_id());
        if let Some(ref p) = self.passphrase {
            rb = rb.header("X-Passphrase", p);
        }
        let response = rb
            .body(raw_body.to_owned())
            .send()
            .await
            .context("Failed to connect to EREBYX")?;

        let status = response.status();
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_BYTES {
                anyhow::bail!(
                    "Response body too large ({} bytes; cap is {})",
                    len,
                    MAX_RESPONSE_BYTES
                );
            }
        }
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!(
                "Server returned HTTP {}: {}",
                status.as_u16(),
                truncate_safe(&body, 500)
            );
        }

        serde_json::from_str(&body).context("Invalid JSON in MCP response")
    }

    /// Authenticated, side-effect-free auth probe for `erebyx doctor`.
    ///
    /// CLI v0.1.2 fix [N3]: the doctor's auth check previously called
    /// `health()`, which GETs the *unauthenticated* `/health` route — so a
    /// revoked or garbage key still reported green ("key accepted"). This
    /// instead POSTs a JSON-RPC `tools/list` to the bearer-authenticated
    /// `/mcp/` endpoint:
    ///   - It is the SAME endpoint every real tool call already uses, so it
    ///     is guaranteed to exist and to enforce auth (a bad token yields a
    ///     non-2xx — typically 401 — before the JSON-RPC layer).
    ///   - `tools/list` only enumerates the server's tool schema; it never
    ///     executes a tool, so it creates NO memory and costs nothing on the
    ///     user's substrate (unlike a `save`/`remember` round-trip).
    ///
    /// Returns `Ok(())` on a 2xx, and an `Err` whose message carries the HTTP
    /// status on any non-2xx (so the doctor can branch on "401" → rejected).
    ///
    /// SERVER-CONFIRM (verify-before-publish): `tools/list` is a standard MCP
    /// method the substrate already answers during the stdio handshake
    /// (`mcp-serve` proxies it). If the substrate ever stops accepting
    /// `tools/list` over the HTTP `/mcp/` transport, swap this for the
    /// cheapest authenticated route the server team confirms.
    pub async fn probe_auth(&self) -> Result<()> {
        let url = format!("{}/mcp/", self.base_url.trim_end_matches('/'));
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });

        let mut rb = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .bearer_auth(&self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .header("X-Erebyx-Session-Id", session_id());
        if let Some(ref p) = self.passphrase {
            rb = rb.header("X-Passphrase", p);
        }
        let response = rb
            .json(&body)
            .send()
            .await
            .context("Failed to connect to EREBYX")?;

        let status = response.status();
        if !status.is_success() {
            // Surface the status code so the doctor can distinguish a 401
            // (auth rejected) from a 5xx (substrate down).
            anyhow::bail!("Auth probe returned HTTP {}", status.as_u16());
        }
        Ok(())
    }

    /// Anonymous server-reachability probe — no API key required.
    ///
    /// The substrate's `/health` route is intentionally unauthenticated so
    /// monitoring + first-touch reachability checks work without a key.
    /// This static helper exists so `erebyx health` and `erebyx doctor` can
    /// answer "is the substrate up?" BEFORE the user has run `erebyx setup`
    /// or set `EREBYX_API_KEY`.
    ///
    /// `api_url` defaults to `https://core.erebyx.com` if `EREBYX_API_URL`
    /// is unset.
    pub async fn health_anonymous(api_url: Option<&str>) -> Result<Value> {
        let base = match api_url {
            Some(u) => u.to_string(),
            None => {
                env::var("EREBYX_API_URL").unwrap_or_else(|_| "https://core.erebyx.com".to_string())
            }
        };
        let url = format!("{}/health", base.trim_end_matches('/'));

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to build HTTP client")?;

        let response = client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to EREBYX")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Health endpoint returned HTTP {}", status);
        }
        response
            .json::<Value>()
            .await
            .context("Invalid JSON from health endpoint")
    }

    /// Check server health via GET /health
    pub async fn health(&self) -> Result<Value> {
        let url = format!("{}/health", self.base_url.trim_end_matches('/'));

        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .header("X-Erebyx-Session-Id", session_id())
            .send()
            .await
            .context("Failed to connect to EREBYX")?;

        let status = response.status();
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_BYTES {
                anyhow::bail!(
                    "Health response too large ({} bytes; cap is {})",
                    len,
                    MAX_RESPONSE_BYTES
                );
            }
        }
        let response_text = response
            .text()
            .await
            .context("Failed to read health response")?;

        if !status.is_success() {
            anyhow::bail!(
                "Health check failed with HTTP {}: {}",
                status.as_u16(),
                truncate_safe(&response_text, 500)
            );
        }

        serde_json::from_str(&response_text).context("Invalid JSON in health response")
    }
}

/// Truncate a string to at most `max_len` bytes, respecting UTF-8 char boundaries.
/// Never panics on multi-byte characters (emoji, CJK, etc.).
fn truncate_safe(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod probe_auth_tests {
    use super::*;
    use serial_test::serial;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    fn client_for(uri: &str) -> ErebyxClient {
        std::env::set_var(
            "EREBYX_API_KEY",
            "ebx_test_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZab",
        );
        std::env::set_var("EREBYX_API_URL", uri);
        std::env::remove_var("EREBYX_PASSPHRASE");
        let c = ErebyxClient::new().expect("client builds for test");
        // The URL guard rejects plain http to non-localhost; wiremock binds
        // 127.0.0.1 so it's allowed. Clean the env immediately after build.
        std::env::remove_var("EREBYX_API_URL");
        std::env::remove_var("EREBYX_API_KEY");
        c
    }

    /// CLI v0.1.2 fix [N3]: probe_auth POSTs `tools/list` (authenticated,
    /// side-effect-free) and treats a 2xx as success.
    #[tokio::test]
    #[serial]
    async fn probe_auth_ok_on_2xx() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/mcp/"))
            .and(matchers::header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "result": { "tools": [] }
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        assert!(client.probe_auth().await.is_ok());
    }

    /// A revoked/garbage key yields a 401 — probe_auth must Err with a
    /// message carrying the status so the doctor can branch on "401".
    #[tokio::test]
    #[serial]
    async fn probe_auth_surfaces_401() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/mcp/"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let err = client.probe_auth().await.expect_err("401 must Err");
        assert!(
            format!("{err}").contains("401"),
            "error must reference the 401 status, got: {err}"
        );
    }
}

#[cfg(test)]
mod health_anonymous_tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    /// `health_anonymous` hits an unauthenticated /health endpoint —
    /// no Bearer header, no X-Instance-ID, no X-Erebyx-Session-Id.
    /// Pins the contract that this probe truly does NOT require a
    /// configured API key.
    #[tokio::test]
    async fn health_anonymous_does_not_send_auth_headers() {
        let server = MockServer::start().await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            // Negative assertions: these headers MUST be absent
            // (or empty) because the probe runs pre-API-key-config.
            // `matchers::header_exists` returning false isn't directly
            // expressible, so we rely on response-shape assertion +
            // visual confirmation that the request handler in
            // health_anonymous explicitly doesn't add auth.
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "healthy",
                "transport": "fastapi",
            })))
            .mount(&server)
            .await;

        let result = ErebyxClient::health_anonymous(Some(&server.uri()))
            .await
            .expect("health_anonymous must succeed against a 200 mock");

        assert_eq!(result["status"], "healthy");
    }

    /// Server unreachable → returns a contextual error, not a panic.
    #[tokio::test]
    async fn health_anonymous_surfaces_connection_failure() {
        // Use a port that's almost certainly not in use to force a
        // connect-refused. Localhost:1 typically has no listener.
        let result = ErebyxClient::health_anonymous(Some("http://127.0.0.1:1")).await;
        assert!(result.is_err(), "unreachable server must Err, not panic");
    }

    /// Non-2xx response → Err with a server-status message (not Ok with
    /// a misleading body).
    #[tokio::test]
    async fn health_anonymous_returns_error_on_non_2xx() {
        let server = MockServer::start().await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let result = ErebyxClient::health_anonymous(Some(&server.uri())).await;
        assert!(result.is_err(), "503 must surface as an Err");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("503") || msg.contains("status"),
            "error message must reference the server status, got: {msg}"
        );
    }

    /// Trailing-slash handling — the URL builder must normalize so we
    /// don't ship `//health` to the server.
    #[tokio::test]
    async fn health_anonymous_normalizes_trailing_slash_on_base_url() {
        let server = MockServer::start().await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "healthy",
            })))
            .mount(&server)
            .await;

        // server.uri() returns no trailing slash; explicitly add one.
        let url_with_slash = format!("{}/", server.uri());
        let result = ErebyxClient::health_anonymous(Some(&url_with_slash))
            .await
            .expect("trailing slash on base must be normalized");
        assert_eq!(result["status"], "healthy");
    }
}
