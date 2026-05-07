# Changelog

All notable changes to `erebyx-cli` are documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) and [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

The substrate-side release notes live at [erebyx-os `CHANGELOG_v0_1_1.md`](https://github.com/ProjectErebyx/erebyx-os/blob/main/CHANGELOG_v0_1_1.md).

---

## [Unreleased]

### Changed

- **License: Apache-2.0 → MIT-OR-Apache-2.0 dual** per locked canon 2026-05-07 (crates.io ecosystem convention; Rust itself is dual-licensed). `LICENSE` renamed to `LICENSE-APACHE-2.0`; new `LICENSE-MIT` added. `Cargo.toml` license field updated. README + CONTRIBUTING + NOTICE updated. Source-of-truth: `erebyx-monorepo/docs/distribution/license-canon/README.md` §4.1.
- **Cargo.toml `repository` URL**: lowercase `erebyx-cli` → canonical ALL-CAPS `EREBYX-CLI` per ADR-0003 + Mikey "EREBYX always capitalized" lock 2026-05-07.

### Added

- `.github/pull_request_template.md` — carve-out PR template with zero-substrate-logic checklist (load-bearing patent defense per Lock 12+28+42).
- `.github/workflows/dco-check.yml` — DCO sign-off enforcement workflow.

---

## [0.1.1] — 2026-04-27 — Genesis Arche

First public release. The CLI surfaces the EREBYX v0.1.1 cognitive verbs as native commands.

### Added

- **5 cognitive verbs**: `restore-identity`, `load-context`, `save`, `remember`, `wrap-up`
- **One-shot installer**: `npx @erebyx/install-mcp@latest` auto-detects every MCP-capable AI client on the machine and writes per-client MCP configs in a single pass. Idempotent on re-run.
- **Multi-client support**: Claude Desktop, Claude Code, Cursor, Windsurf, VS Code (Continue / Cline), Aider, LM Studio, Zed
- **`erebyx setup`** — interactive API-key + per-client MCP config wizard
- **`erebyx doctor`** — full client config audit (reports per-AI-client status)
- **`erebyx health`** — server reachability + version probe
- **JSON mode** — every command supports `--json` for machine-readable output (agents, scripts, CI)
- **`X-Erebyx-Hint` protocol support** — lifecycle hints surfaced in `--json` output. Hint values: `wrap_up_recommended`, `restore_identity_recommended`, `load_context_recommended`, `compact_imminent`. Honoring hints is optional.
- **Cold-session auto-fire transparency** — first call against a fresh session transparently triggers `restore_identity` + `load_context` substrate-side. The CLI surfaces `X-Erebyx-Auto-Fired` headers so you can observe what happened.

### Configuration

- `EREBYX_API_KEY` (required)
- `EREBYX_API_URL` (default: `https://core.erebyx.com`)
- `EREBYX_INSTANCE_ID` (default: `cli`)
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

- `evolve` — memory reconsolidation (substrate-internal in v0.1.1; CLI verb in v0.2)
- `learn` — explicit relationship formation
- `import` — bulk import from ChatGPT / Claude / Markdown exports
- `pin` / `release` — explicit memory tier control

See the [v0.2 roadmap](https://erebyx.com/docs/roadmap) for cadence.

---

## How to upgrade

```bash
# Cargo install
cargo install erebyx --force

# Or via the npx wrapper (always pulls latest)
npx @erebyx/install-mcp@latest
```

Confirm: `erebyx --version`

---

[0.1.1]: https://github.com/ProjectErebyx/erebyx-cli/releases/tag/v0.1.1
