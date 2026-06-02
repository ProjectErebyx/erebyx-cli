# Security Policy

## Reporting a vulnerability

If you believe you've found a security vulnerability in `erebyx-cli`, report it privately so we can fix it before it harms anyone.

- **Email:** `legal@erebyx.com`
- **Alternate:** `privacy@erebyx.com` (data-handling concerns)
- **PGP key:** Available on request to `legal@erebyx.com`
- **Response SLA:** acknowledgment within 24 hours; status update within 72 hours

Please include:
- Clear description of the vulnerability + impact assessment
- Steps to reproduce (proof-of-concept code, requests, payloads)
- The affected component and `erebyx --version`
- Whether you intend to disclose publicly, and on what timeline

We follow **coordinated disclosure**: we'll work with you on a fix and a public-disclosure timeline. We do not currently run a paid bug-bounty program but will publicly credit responsible disclosures with your permission.

**Do NOT open a public GitHub issue for a security report.** That defeats the private-disclosure protection.

---

## Supported versions

| Version | Supported            | Notes |
|---------|----------------------|---|
| 0.1.x   | :white_check_mark:   | Active development; security fixes within 72h |
| < 0.1   | :x:                  | Pre-release, unsupported |

When v0.2 ships, v0.1.x receives security fixes for 90 days.

---

## Scope

In scope:
- The `erebyx` CLI binary distributed via `crates.io` or `cargo install`
- The MCP config writers that touch local AI client config files
- API-key handling and storage
- Network behavior (TLS, request signing, circuit breaker)

Out of scope:
- Issues in the substrate engine `erebyx-os` (closed-source — same email, separate triage)
- Third-party dependencies (`reqwest`, `clap`, `tokio`, etc.) — please report upstream
- Local-only attacks requiring physical machine access
- Self-XSS where the only victim is the reporter

---

## API-key file handling (`erebyx setup`)

`erebyx setup` writes your `EREBYX_API_KEY` in plaintext into each
detected AI client's MCP config file (e.g. `~/.claude/settings.json`,
`~/.cursor/mcp.json`). Specifics:

- **Unix (macOS / Linux):** the CLI sets each written file's
  permissions to `0600` (owner read/write only). Other local users
  cannot read the key.
- **Windows:** the CLI cannot set per-file ACLs without an extra
  dependency in v0.1.1. Files inherit the parent directory's ACL —
  typically user-profile-scoped on a single-user box, but **multi-user
  hosts and roaming profiles are not protected**. The CLI emits a
  one-line warning when it writes on Windows. Tighten the ACL
  out-of-band — Claude Code on Windows lands settings at
  `%USERPROFILE%\.claude\settings.json`:

  ```cmd
  icacls "%USERPROFILE%\.claude\settings.json" /inheritance:r ^
      /grant:r "%USERNAME%:F" "SYSTEM:F" "Administrators:F"
  ```

  Granting SYSTEM and Administrators alongside your user prevents
  backup tools and AV scanners running as SYSTEM from losing read
  access (the bare `%USERNAME%:F` grant strips both inherited
  rights). v0.1.2 will land the in-process fix via `windows-acl`.
- **Git working trees:** the CLI refuses to write a config file whose
  ancestor contains a `.git` directory unless you set
  `EREBYX_ALLOW_GIT_TREE_CONFIG=1` explicitly (truthy: `1`, `true`,
  `yes` — `0` does NOT bypass). This prevents the common footgun where
  users sync `~/.claude/` (or similar) to a public dotfiles repo and
  unknowingly commit a credential. **`$HOME` as a git-managed
  dotfiles repo (yadm, chezmoi --bare) is recognized and allowed by
  default** — refuse it explicitly via `EREBYX_REFUSE_HOME_DOTFILES=1`
  if your home repo IS the synced-public-repo case. Symlinked config
  dirs into a dotfiles tree are caught by the canonical-path
  resolution.
- **Rotation:** if a config file is ever committed to a public repo
  or shared inadvertently, rotate your key immediately at
  [app.erebyx.com/keys](https://app.erebyx.com/keys). The substrate
  treats every key as a bearer credential.

## Known limitations + roadmap

| Area | Current limitation | Target fix |
|---|---|---|
| Client-side encryption | Memory is encrypted in transit (TLS 1.3) and at rest using XChaCha20-Poly1305 envelope encryption (AES-256-GCM legacy supported on existing rows) with per-tenant Key Encryption Keys wrapped under a server-held master KEK. At v0.1.1 EREBYX operationally holds the master KEK; per-user zero-knowledge encryption (passphrase-derived keys, EREBYX cannot decrypt) ships in v0.2. The browser extension already implements client-side AES-256-GCM today. | v0.2+ |
| Windows ACL hardening | v0.1.1 emits a warning instead of setting a user-only DACL on written configs. v0.1.2 will wire `windows-acl` or equivalent to close the multi-user-host gap. | v0.1.2 |
| API-key rotation | Manual rotation via `app.erebyx.com/keys`; CLI does not yet auto-rotate | v0.2 |
| Sandbox for `setup` writers | Config writers touch real client-config files; no dry-run mode | v0.1.x |
| Keyring storage path | `setup` writes the API key directly into each client's MCP config. A future `EREBYX_API_KEY_FILE` + OS-keyring path will keep the key out of the config files entirely. | v0.1.2 |

---

**Built by EREBYX, LLC** — `https://erebyx.com`
