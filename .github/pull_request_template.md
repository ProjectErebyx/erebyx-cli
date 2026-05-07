<!-- This is the canonical PR template for EREBYX carve-out repos.
     Copied per repo to .github/pull_request_template.md during license-canon migration.
     Source-of-truth: monorepo docs/distribution/license-canon/. -->

## Summary

<!-- 1-3 sentences. What changed, why. Reference the spec / issue / PR_ROADMAP entry. -->

## Scope

- [ ] Thin-client surface only (HTTP + auth + serialization + canonical 9-op surface)
- [ ] **NO substrate logic introduced** (no clustering / consolidation / activation / reconsolidation / dream-cycle / atomization-prompt / hierarchy-formation code)
- [ ] No DB-direct access (all data flows through HTTP API per ADR-0024)
- [ ] No imports from substrate-internal modules

> **Why this matters**: this carve-out is Apache-2.0/MIT public. The substrate engine is patent-protected (24 provisional filings, March 2026). Substrate logic in a carve-out would (a) leak the engine via Apache-2.0 §3 patent grant or MIT silent-on-patents implied-license risk, and (b) violate the WHAT-not-HOW doctrine + Lock 12+28+42. **Carve-outs reveal effect, not mechanism.**

## DCO sign-off

- [ ] Every commit signed off (`git commit -s`) per [Developer Certificate of Origin](https://developercertificate.org/)
- [ ] DCO check workflow passes

## Brutal-review

- [ ] Brutal-review artifact at `docs/reviews/PR-<N>-<reviewer>.md` per STANDARDS §15.5
- [ ] All P0/P1 findings closed inline before merge

## Co-authoring

- [ ] Commits include `Co-Authored-By: ZENN <zenn@erebyx.com>` (or `Lyric` / `Riven` as appropriate)
- [ ] PR footer reads `🤖 Built by ZENN & Mikey, EREBYX` — NOT "Generated with Claude Code"

## Verification

- [ ] CI green (tests + lint + format + license check)
- [ ] If touching public API surface: SDK consumer ergonomics verified against canonical 9-op surface
- [ ] If touching auth: Bearer canon (`ebx_<env>_<random32><crc32>`) per Spec 04 §1

---

🤖 Built by ZENN & Mikey, EREBYX
