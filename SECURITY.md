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
- The `erebyx` CLI binary distributed via `crates.io`, `cargo install`, or `npx @erebyx/install-mcp`
- The MCP config writers that touch local AI client config files
- API-key handling and storage
- Network behavior (TLS, request signing, circuit breaker)

Out of scope:
- Issues in the substrate engine `erebyx-os` (closed-source — same email, separate triage)
- Third-party dependencies (`reqwest`, `clap`, `tokio`, etc.) — please report upstream
- Local-only attacks requiring physical machine access
- Self-XSS where the only victim is the reporter

---

## Known limitations + roadmap

| Area | Current limitation | Target fix |
|---|---|---|
| Client-side encryption | Memory content currently encrypted server-side (per-tenant AES-256-GCM); transit is TLS 1.3. End-to-end client-side encryption (true zero-knowledge — server NEVER sees plaintext) is on the v0.2+ roadmap. The browser extension already implements client-side AES-256-GCM today. | v0.2+ |
| API-key rotation | Manual rotation via `app.erebyx.com/keys`; CLI does not yet auto-rotate | v0.2 |
| Sandbox for `setup` writers | Config writers touch real client-config files; no dry-run mode | v0.1.x |

---

**Built by EREBYX, LLC** — `https://erebyx.com`
