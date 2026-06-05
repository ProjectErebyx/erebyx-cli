# Changelog

All notable changes to `erebyx-cli` are documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) and [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.

Substrate-side release notes are summarized at [erebyx.com/core](https://erebyx.com/core).

---

## [0.1.2] — 2026-06-05

Surface-hardening release. No wire-protocol or CLI-flag breaking changes.

### Fixed

- **Claude Code hooks were wired at the wrong `settings.json` nesting** and never
  fired in 0.1.0 / 0.1.1 — the memory-injection + session-start hooks silently
  did nothing. **Re-run `erebyx setup`** to install the corrected hook wiring.
- **`erebyx doctor` could panic** on a malformed `EREBYX_API_KEY` (an emoji or
  other multibyte character pasted into the first 10 chars). The key preview is
  now built on char boundaries and can never panic.
- **`erebyx doctor` over-claimed key validity.** The Environment check now
  validates the full advertised key shape (`erebyx_` + 48 hex chars) instead of
  a loose length check, so a garbage key no longer renders a false `✓ set`.
- **`erebyx doctor` reported a revoked/garbage key as "accepted".** The auth
  check now probes an *authenticated* route (`tools/list` on `/mcp/`) instead of
  the unauthenticated `/health` route, so a 401/403 surfaces honestly. The probe
  lists tool schemas only — it never creates a memory.
- **Client-config detection false-positives.** Detection of an existing erebyx
  config no longer relies on a bare `"erebyx"` substring scan (which matched any
  incidental mention in a comment or path). It now parses the config and checks
  the exact key path this CLI writes per client.
- **Windsurf global-rules path was wrong** (`~/.windsurfrules` is a project-root
  file, not a `$HOME` global) — writing there was a silent no-op. Global rules
  now go to `~/.codeium/windsurf/memories/global_rules.md`. The macOS app probe
  is also gated to macOS.
- **`erebyx setup` and `erebyx doctor` always exited 0**, even when every client
  failed / the substrate was unreachable — so `erebyx setup && <next>` chained
  past a broken install. Setup now exits non-zero when *all* clients fail (a
  partial success still exits 0); doctor exits non-zero (code 2) on any failed
  check.
- **Server 5xx bodies were echoed verbatim** into the customer-facing error
  message (noise, no credential). They are now dropped from the default message
  and shown only under `RUST_LOG` / `EREBYX_VERBOSE`.
- **`examples/hello_world.rs` could panic** truncating a multibyte preview;
  fixed to truncate on char boundaries, and its SPDX header now matches the
  crate's `MIT OR Apache-2.0` license.

### Added

- **`erebyx setup --yes`** (alias `-y` / `--force`) — skip the interactive
  "Reconfigure?" confirmation so re-provisioning works under CI / non-TTY
  environments without erroring with `not a terminal`.

### Changed

- **`erebyx doctor` now exits 2 on a failed check** (was always 0). Warnings
  remain exit 0.
- **Dependency trim**: dropped the unused `dialoguer` `fuzzy-select` feature (and
  its `fuzzy-matcher` transitive dependency) — only `Password` + `Confirm` are
  used.
- **Crate packaging** switched from an `exclude` list to an explicit `include`
  allowlist so only intended files ever ship.
- **MSRV raised to Rust 1.85** (was 1.75). The current `reqwest` / `hyper`
  dependency stack pulls `hashbrown` 0.17 and `indexmap` 2.14, which declare
  `rust-version = 1.85`, and `Cargo.lock` is now format v4 (parseable only by
  Cargo ≥ 1.78). A new CI job builds on exactly 1.85 with `--locked` so the
  declared MSRV stays honest against future dependency bumps.

### CI / Release

- CI now runs `clippy` + `test` on a `ubuntu` / `windows` / `macos` matrix so the
  platform-specific path / hook code is compiled and linted on every PR.
- Added a `cargo audit` + `cargo deny check` advisory/license/bans gate; the
  crates.io publish job now `needs:` it, so a known-vuln dependency blocks a
  release.

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
- `EREBYX_INSTANCE_ID` (default: `default` — same canonical tenant slice across CLI / SDK; override for per-surface attribution)
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

See the [v0.2 roadmap](https://erebyx.com/genesis) for cadence.

---

## How to upgrade

```bash
cargo install erebyx --force
```

Confirm: `erebyx --version`

---

[0.1.2]: https://github.com/ProjectErebyx/erebyx-cli/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/ProjectErebyx/erebyx-cli/releases/tag/v0.1.1
