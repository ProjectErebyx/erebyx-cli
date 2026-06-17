# erebyx-cli

> Connect any MCP-capable AI to your EREBYX memory substrate. Persistent memory across every AI you use.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-APACHE-2.0)
[![Version](https://img.shields.io/crates/v/erebyx.svg)](https://crates.io/crates/erebyx)
[![DCO](https://img.shields.io/badge/DCO-required-orange.svg)](https://github.com/ProjectEREBYX/erebyx-cli/blob/main/CONTRIBUTING.md#sign-off-dco)

---

## Install in 5 lines

```bash
cargo install erebyx                  # native CLI binary
export EREBYX_API_KEY="<YOUR_API_KEY>"    # get one at https://app.erebyx.com/keys
erebyx setup                          # auto-detects every MCP-capable AI on your machine
erebyx save "Anchor-based retrieval improves recall by 40%" --category insight
erebyx remember "anchor retrieval"
```

That's the whole loop: install -> setup -> save -> remember. Memory follows you across every MCP client on this machine.

---

## What it is

`erebyx-cli` is the native client for the EREBYX memory substrate. It exposes the v0.1.1 cognitive surface as five verbs you can call from any shell, script, or AI harness:

| Verb | Purpose |
|---|---|
| `restore-identity` | Wake up — load identity at session start |
| `load-context` | Resume — load handoff + recent work |
| `save` | Store a memory worth keeping |
| `remember` | Find what you know by meaning |
| `wrap-up` | Create a session handoff at the end |

All processing — memory understanding, recall, organization, encryption — lives behind the API — you never need to think about it.

---

## One command, every client

```bash
cargo install erebyx   # native CLI binary
erebyx setup           # masked API-key prompt, then writes each detected client's config
```

`erebyx setup` prompts for your API key (masked), then auto-detects and configures **every MCP-capable AI on this machine** in a single pass. It's idempotent — re-run it any time you install a new client, and it merges into existing config without clobbering MCP servers you already have. Preview everything first with `erebyx setup --dry-run` (writes nothing; prints the exact config snippet for each client).

### Auto-configured clients (13)

`erebyx setup` detects and writes a local MCP server config for each of these:

| Client | Config it writes |
|---|---|
| **Claude Code** | `~/.claude/settings.json` MCP entry **+ memory-injection hook** |
| **Cursor** | `~/.cursor/mcp.json` |
| **Windsurf** | `~/.codeium/windsurf/mcp_config.json` |
| **Continue** | `~/.continue/config.yaml` (or legacy `config.json`) |
| **Zed** | `~/.config/zed/settings.json` (`context_servers`) |
| **VS Code / Copilot** | VS Code `settings.json` (`mcp.servers`) |
| **Codex (OpenAI Codex CLI)** | `~/.codex/config.toml` (`[mcp_servers."erebyx-os"]`) |
| **Gemini CLI** | `~/.gemini/settings.json` |
| **Claude Desktop** | `claude_desktop_config.json` (platform-specific path) |
| **Cline** | VS Code globalStorage `cline_mcp_settings.json` |
| **Antigravity** | `~/.gemini/config/mcp_config.json` |
| **Grok Build CLI** | `~/.grok/user-settings.json` (`mcp.servers` array) |
| **Goose** | `~/.config/goose/config.yaml` (`extensions`) |

The first six shipped at v0.1.1; the last seven were added in v0.1.3. Only clients actually installed on your machine are written.

### Remote connectors (manual paste)

Three clients have **no local config file** an installer can write — they register an MCP server through their own UI. `erebyx setup` **prints** the exact values to paste for each:

- **ChatGPT** (Settings → Connectors → Developer Mode → add custom connector)
- **Grok chat app** (add a custom MCP server / integration)
- **JetBrains AI** (add an MCP server)

Point each at `https://core.erebyx.com/mcp` with an `Authorization: Bearer <EREBYX_API_KEY>` header.

### Preview before writing

```bash
erebyx setup --dry-run   # writes nothing; prints the per-client config snippet
```

`--dry-run` makes no HTTP calls and touches no files — it renders the exact JSON object, JSON array, TOML, or YAML that would be merged into each detected client (plus the remote-connector section), so you can audit the schema first.

### What `erebyx setup` writes

For every detected client, the setup writer drops an MCP server entry pointing at the local `erebyx mcp-serve` binary. Example (Claude Code, Cursor, Windsurf):

```jsonc
{
  "mcpServers": {
    "erebyx-os": {
      "command": "/usr/local/bin/erebyx",
      "args": ["mcp-serve"],
      "env": {
        "EREBYX_API_KEY": "<YOUR_API_KEY>",
        "EREBYX_API_URL": "https://core.erebyx.com",
        "EREBYX_INSTANCE_ID": "default"
      }
    }
  }
}
```

`erebyx mcp-serve` is a stdio bridge — it reads JSON-RPC on stdin, forwards to the substrate over HTTPS, and writes responses on stdout. Your AI client speaks pure MCP; the binary does no protocol logic of its own.

---

## Configuration

```bash
# Required
export EREBYX_API_KEY="<YOUR_API_KEY>"

# Required for tenants registered at v0.1.1+ (Argon2id-default-on).
# Find it in your dashboard recovery panel. At v0.1.1, EREBYX holds a
# server-side master KEK and can decrypt for support/backup/recovery —
# this is NOT zero-knowledge. When per-user zero-knowledge ships in v0.2,
# your passphrase + BIP39 recovery seed become the ONLY keys to your
# memory; losing both will be unrecoverable by design. Keep both safe.
export EREBYX_PASSPHRASE="<YOUR_PASSPHRASE>"

# Optional (defaults shown)
export EREBYX_API_URL="https://core.erebyx.com"
export EREBYX_INSTANCE_ID="default"
```

Get your API key at [app.erebyx.com/keys](https://app.erebyx.com/keys).
The dashboard surfaces the matching `EREBYX_PASSPHRASE` value at registration;
both are required for new tenants.

---

## X-Erebyx-Hint — lifecycle signals for free

Every CLI call surfaces the substrate's lifecycle hints over HTTP. When the substrate sees a natural consolidation boundary, you get a hint back:

```bash
erebyx save "..." --json | jq '.hints'
# ["wrap_up_recommended"]
```

Hint values:
- `wrap_up_recommended` — substrate sees a natural consolidation boundary
- `restore_identity_recommended` — voice drift detected (v0.2)
- `load_context_recommended` — retrieval scores trending low
- `compact_imminent` — sustained save volume; consolidate before context fills

Honoring hints is optional. Disable globally with `EREBYX_HINTS_DISABLED=1`. Full hint protocol at [DEV_QUICKSTART.md](https://github.com/ProjectEREBYX/erebyx-cli/blob/main/DEV_QUICKSTART.md#x-erebyx-hint--lifecycle-signals).

---

## Usage cheat-sheet

### Session start
```bash
erebyx restore-identity                          # baseline
erebyx restore-identity --limit 5 --detail full  # rich identity load
erebyx load-context --anchors trading,coding     # filter by domain
```

### During a session
```bash
erebyx save "Discovered anchor-based retrieval improves recall by 40%" \
  --category insight \
  --title "Anchor Retrieval Improvement" \
  --anchors memory,retrieval \
  --importance 0.9

erebyx remember "anchor retrieval performance" --limit 5
erebyx remember "trading patterns" --anchors trading --time-range last-week
erebyx remember "anchor retrieval" --ids mem_abc123,mem_def456  # query is required, even with --ids
```

### Session end
```bash
erebyx wrap-up "Built the erebyx CLI in Rust" \
  --whats-next "Add shell completions" \
  --anchors cli,rust \
  --energy systematic
```

### Health & diagnostics
```bash
erebyx health    # server reachability + version
erebyx doctor    # full client config audit
```

### MCP stdio server
```bash
erebyx mcp-serve   # invoked by AI clients; reads JSON-RPC on stdin, writes on stdout
```
You should not need to run this by hand — `erebyx setup` wires it into each detected client's MCP config. Use it for direct MCP-over-stdio testing if you're building a custom integration.

### JSON output (for agents)
```bash
erebyx remember "query" --json | jq '.memories[0].content'
```

---

## Architecture

```
src/
  main.rs    Clap dispatch + mcp-serve stdio bridge
  cli.rs     Command surface (5 cognitive verbs + setup, doctor, health, mcp-serve)
  client.rs  HTTP client (reqwest) for MCP JSON-RPC
  output.rs  JSON vs pretty formatting
  setup/     MCP config writers (one per AI client)
```

The CLI calls the MCP HTTP endpoint at `${EREBYX_API_URL}/mcp/` using JSON-RPC. Each command maps to one MCP tool call. Health uses `GET /health` directly. `mcp-serve` reads JSON-RPC on stdin and forwards verbatim to the same `/mcp/` endpoint, so AI clients can speak MCP stdio while the substrate stays HTTPS-only.

### Headers on every request
- `Authorization: Bearer <api_key>` — authentication (canonical Bearer form)
- `X-Instance-ID` — multi-tenant routing
- `X-Erebyx-Session-Id` — stable per-install id (override with `EREBYX_SESSION_ID`)
- `Content-Type: application/json`

---

## How to upgrade

Track releases in [CHANGELOG.md](CHANGELOG.md). Backward compatibility is a hard guarantee within v0.1.x — every release lists explicit breaking changes (none expected before v0.2).

```bash
cargo install erebyx --force
```

---

## Build from source

```bash
git clone https://github.com/ProjectEREBYX/erebyx-cli.git
cd erebyx-cli
cargo build --release
# Binary lands at target/release/erebyx
```

---

## See also

- [`erebyx-sdk`](https://github.com/ProjectEREBYX/erebyx-sdk) — Rust SDK (type-safe substrate client)
- [`@erebyx/sdk`](https://github.com/ProjectEREBYX/erebyx-sdk-node) — Node.js / TypeScript SDK
- [EREBYX Core docs](https://erebyx.com/core)
- [Per-harness integration examples](https://erebyx.com/core) — 13 auto-configured clients, copy-paste integration

---

## Contributing

Pull requests welcome. DCO sign-off required (`git commit -s`). See [CONTRIBUTING.md](https://github.com/ProjectEREBYX/erebyx-cli/blob/main/CONTRIBUTING.md).

## Security

Vulnerability reports → `legal@erebyx.com`. See [SECURITY.md](SECURITY.md).

## License

Dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE-2.0) at your option. See [NOTICE](NOTICE) for attribution requirements when used under Apache-2.0.

---

**Built by EREBYX, LLC** — `https://erebyx.com`
