use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

/// HTTP client for erebyx-os MCP endpoint.
/// Sends JSON-RPC tool calls and returns the result content.
pub struct ErebyxClient {
    client: Client,
    base_url: String,
    api_key: String,
    instance_id: String,
}

/// JSON-RPC response from the MCP server
#[derive(Debug)]
pub struct McpResponse {
    pub content: Value,
    pub is_error: bool,
}

impl ErebyxClient {
    pub fn new() -> Result<Self> {
        let api_key =
            env::var("EREBYX_API_KEY").context("EREBYX_API_KEY environment variable is required")?;

        let base_url =
            env::var("EREBYX_API_URL").unwrap_or_else(|_| "https://core.erebyx.com".to_string());

        let instance_id =
            env::var("EREBYX_INSTANCE_ID").unwrap_or_else(|_| "cli".to_string());

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

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("X-API-Key", &self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .json(&body)
            .send()
            .await
            .context("Failed to connect to erebyx-os")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!(
                "Server returned HTTP {}: {}",
                status.as_u16(),
                truncate(&response_text, 500)
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

        Ok(McpResponse { content, is_error })
    }

    /// Check server health via GET /health
    pub async fn health(&self) -> Result<Value> {
        let url = format!("{}/health", self.base_url.trim_end_matches('/'));

        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .header("X-Instance-ID", &self.instance_id)
            .send()
            .await
            .context("Failed to connect to erebyx-os")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read health response")?;

        if !status.is_success() {
            anyhow::bail!(
                "Health check failed with HTTP {}: {}",
                status.as_u16(),
                truncate(&response_text, 500)
            );
        }

        serde_json::from_str(&response_text).context("Invalid JSON in health response")
    }
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
