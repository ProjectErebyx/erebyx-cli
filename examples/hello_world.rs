// SPDX-License-Identifier: Apache-2.0
//! Quick-start example: save a memory, then retrieve it.
//!
//! Run:
//!
//! ```bash
//! EREBYX_API_KEY=erebyx_... cargo run --example hello_world
//! ```
//!
//! This example talks to the substrate's HTTP API directly using `reqwest`
//! so you can see the full request shape without going through the CLI.
//! For a typed Rust client, use the [`erebyx-sdk`] crate instead.
//!
//! [`erebyx-sdk`]: https://github.com/ProjectErebyx/erebyx-sdk

use serde_json::{json, Value};
use std::env;

const DEFAULT_API_URL: &str = "https://core.erebyx.com";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("EREBYX_API_KEY")
        .expect("EREBYX_API_KEY env var required (get one at https://app.erebyx.com/keys)");
    let api_url = env::var("EREBYX_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());
    let instance_id = env::var("EREBYX_INSTANCE_ID").unwrap_or_else(|_| "default".to_string());

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let mcp_url = format!("{}/mcp/", api_url.trim_end_matches('/'));

    println!("→ Saving first memory…");
    let save_resp = call_mcp_tool(
        &http,
        &mcp_url,
        &api_key,
        &instance_id,
        "save",
        json!({
            "content": "Hello from the erebyx-cli quick-start example.",
            "category": "insight",
            "title": "Quick-start hello",
            "anchors": ["quickstart", "example"],
            "importance": 0.5,
        }),
    )
    .await?;
    println!("  saved: {}", short(&save_resp));

    println!("→ Retrieving by query…");
    let remember_resp = call_mcp_tool(
        &http,
        &mcp_url,
        &api_key,
        &instance_id,
        "remember",
        json!({ "query": "quick-start hello", "limit": 3 }),
    )
    .await?;
    println!("  found: {}", short(&remember_resp));

    println!("✓ Memory saved + retrieved successfully.");
    Ok(())
}

/// Minimal JSON-RPC tool call against the substrate `/mcp/` endpoint.
async fn call_mcp_tool(
    http: &reqwest::Client,
    mcp_url: &str,
    api_key: &str,
    instance_id: &str,
    tool: &str,
    args: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": args }
    });

    let response = http
        .post(mcp_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("X-API-Key", api_key)
        .header("X-Instance-ID", instance_id)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<Value>().await?)
}

/// Truncate a JSON value's string form for terminal-friendly preview.
fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() <= 240 {
        s
    } else {
        format!("{}…", &s[..240])
    }
}
