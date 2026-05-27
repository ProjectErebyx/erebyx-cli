// SPDX-License-Identifier: Apache-2.0
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

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
                let _ = std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
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
fn is_safe_url(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host_part = rest.split('/').next().unwrap_or("");
        let host = host_part.split(':').next().unwrap_or("");
        return matches!(host, "localhost" | "127.0.0.1" | "::1");
    }
    false
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
    /// persistence via the `keyring` crate.
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
    /// Tools the substrate auto-fired during this request (parsed from
    /// the ``X-Erebyx-Auto-Fired`` response header). Typically
    /// ``["restore_identity", "load_context"]`` on the first call
    /// against a fresh ``(instance_id, session_id)`` tuple, empty
    /// thereafter.
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
        let api_key =
            env::var("EREBYX_API_KEY").context("EREBYX_API_KEY environment variable is required")?;

        let base_url =
            env::var("EREBYX_API_URL").unwrap_or_else(|_| "https://core.erebyx.com".to_string());

        // Default to "default" — same canonical tenant slice across CLI / SDK / extension.
        // Override with EREBYX_INSTANCE_ID if you want per-surface attribution.
        let instance_id =
            env::var("EREBYX_INSTANCE_ID").unwrap_or_else(|_| "default".to_string());

        // Argon2id-default-on: tenants register with a passphrase used to
        // derive the KEK at request time. EREBYX_PASSPHRASE is the transport
        // until prompt-at-setup + OS-keychain (keyring crate, follow-up).
        // Empty strings normalize to None so legacy hkdf_api_key tenants
        // don't accidentally transmit an empty X-Passphrase header.
        let passphrase = env::var("EREBYX_PASSPHRASE")
            .ok()
            .filter(|s| !s.trim().is_empty());

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
            .context("Failed to connect to Erebyx")?;

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
            .context("Failed to connect to Erebyx")?;

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
            .context("Failed to connect to Erebyx")?;

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
