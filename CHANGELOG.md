# Changelog

All notable changes to `erebyx-cli` are documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) and [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

Substrate-side release notes are published at [erebyx.com/changelog](https://erebyx.com/changelog).

---

## [0.1.1] — 2026-04-27 — Genesis Arche

First public release. The CLI surfaces the EREBYX v0.1.1 cognitive verbs as native commands.

### Added

- **5 cognitive verbs**: `restore-identity`, `load-context`, `save`, `remember`, `wrap-up`
- **`erebyx setup`** — interactive API-key + per-client MCP config wizard. Auto-detects every supported AI client on the machine and writes per-client MCP configs in a single pass. Idempotent on re-run.
- **Multi-client support**: Claude Code, Cursor, Windsurf, Continue, Zed, VS Code (Copilot)
- **`erebyx doctor`** — 5-section client config audit (Environment / Auth / MCP / Clients / Hook)
- **`erebyx health`** — substrate reachability + version probe
- **`erebyx hook-inject`** — Claude Code `UserPromptSubmit` hook payload writer
- **`erebyx hook-session-start`** — Claude Code `SessionStart` hook (identity + handoff context cold-load)
- **`erebyx mcp-serve`** — stdio MCP bridge invoked by AI clients
- **JSON mode** — every command supports `--json` for machine-readable output (agents, scripts, CI)
- **`X-Erebyx-Hint` protocol support** — lifecycle hints surfaced in `--json` output. Hint values: `wrap_up_recommended`, `restore_identity_recommended`, `load_context_recommended`, `compact_imminent`. Honoring hints is optional.
- **Cold-session auto-fire transparency** — first call against a fresh session transparently triggers `restore_identity` + `load_context` substrate-side. The CLI surfaces `X-Erebyx-Auto-Fired` headers so you can observe what happened.
- **Dual-licensed** under MIT OR Apache-2.0 (crates.io ecosystem convention; Rust itself is dual-licensed).
- **PR template** + **DCO check workflow** in `.github/`.

### Configuration

- `EREBYX_API_KEY` (required)
- `EREBYX_PASSPHRASE` (required for tenants registered at v0.1.1+ — Argon2id-default-on)
- `EREBYX_API_URL` (default: `https://core.erebyx.com`)
- `EREBYX_INSTANCE_ID` (default: `default` — same canonical tenant slice across CLI / SDK / extension; override for per-surface attribution)
- `EREBYX_HINTS_DISABLED=1` — opt out of `X-Erebyx-Hint` parsing
- `EREBYX_DISABLE_AUTO_FIRE=1` — opt out of substrate-side cold-fire (rare; usually you want it)

### Compatibility

- **MSRV**: Rust 1.75
- **Targets**: macOS (arm64, x64), Linux (arm64, x64), Windows (x64)
- **Substrate**: requires `erebyx-os` v0.1.1+
- **Backward compat**: hard guarantee within v0.1.x. The MCP wire protocol and CLI flags are stable.

### Breaking changes

None. First public release.

### Deferred to v0.2

- `evolve` — update a memory with new context (substrate-internal in v0.1.1; CLI verb in v0.2)
- `learn` — explicit relationship formation
- `import` — bulk import from ChatGPT / Claude / Markdown exports
- `pin` / `release` — explicit memory tier control

See the [v0.2 roadmap](https://erebyx.com/docs/roadmap) for cadence.

---

## How to upgrade

```bash
cargo install erebyx --force
```

Confirm: `erebyx --version`

---

[0.1.1]: https://github.com/ProjectErebyx/erebyx-cli/releases/tag/v0.1.1
