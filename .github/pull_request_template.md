<!-- Canonical PR template for the Erebyx public client surface. -->

## Summary

<!-- 1-3 sentences. What changed, why. Reference the issue or feature request. -->

## Scope

- [ ] Thin-client surface only (HTTP + auth + serialization)
- [ ] No imports from non-public Erebyx modules
- [ ] No direct database access (all data flows through the documented HTTP API)

> **Why this matters**: this repo is a public client carve-out for the Erebyx
> substrate. The substrate itself is closed-source. Keep client code focused on
> wire protocol + ergonomics; substrate behavior lives behind the API and is
> not implemented here.

## DCO sign-off

- [ ] Every commit signed off (`git commit -s`) per [Developer Certificate of Origin](https://developercertificate.org/)
- [ ] DCO check workflow passes

## Verification

- [ ] CI green (tests + lint + format + license check)
- [ ] If touching public API surface: ergonomics verified against the documented
      auth + memory contract (`Authorization: Bearer <api_key>`).
- [ ] If touching auth: no logging of token material; constant-time comparisons
      where applicable.

## Co-authoring

- [ ] Commits include `Co-Authored-By: <Your Name> <email>` for any pair-work.

---

Built with [Erebyx](https://erebyx.com).
