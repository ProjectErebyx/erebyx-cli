# Contributing to erebyx-cli

Thanks for your interest. The CLI is a thin client over the EREBYX memory substrate — pull requests for setup-time UX, client compatibility, ergonomics, and reliability are welcome.

By contributing, you agree your contributions are dual-licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE-2.0) at your option.

---

## Sign-off (DCO)

Every commit must be signed off using the [Developer Certificate of Origin](https://developercertificate.org/):

```bash
git commit -s -m "your message"
```

This adds a `Signed-off-by:` line attesting that you have the right to contribute the code under the project's license.

The DCO bot will block PRs without sign-off.

---

## Local dev setup

```bash
git clone https://github.com/ProjectErebyx/erebyx-cli.git
cd erebyx-cli
cargo build
cargo test
```

Required toolchain:
- Rust 1.85 or later (`rustup install stable`)
- `cargo fmt` + `cargo clippy` components

---

## Test commands

```bash
cargo test                                                    # unit + integration
cargo fmt --check                                             # formatting
cargo clippy --all-targets --locked -- -D warnings           # lint (zero warnings)
```

All three must pass before a PR is reviewed.

---

## Commit conventions

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(setup): detect Zed editor MCP config
fix(client): retry on connection-reset within circuit-breaker window
docs(readme): add JSON output examples
chore(deps): bump reqwest to 0.12.5
```

Types we use: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`.

Subject in imperative mood, ≤72 chars. Body explains *why*, not *what*.

---

## Pull request template

When you open a PR, include:

1. **What changed** — one paragraph
2. **Why** — the user-visible problem this fixes
3. **How verified** — `cargo test` output or repro steps
4. **Risk surface** — backward-compat / breaking-change assessment

PRs that touch the wire protocol (`client.rs`) require an extra reviewer.

---

## Scope

The CLI surfaces the v0.1.1 cognitive verbs: `restore-identity`, `load-context`, `save`, `remember`, `wrap-up` plus operational commands (`setup`, `doctor`, `health`).

All EREBYX processing — memory understanding, recall, organization, encryption — lives in the closed-source `erebyx-os` engine. Client-side issues — install UX, MCP config writers, output formatting, error messages, `X-Erebyx-Hint` parsing — are in scope here.

Out of scope: anything that would require a substrate change. File those as issues on `erebyx-os` instead.

---

## Bug reports

Open a [GitHub Issue](https://github.com/ProjectErebyx/erebyx-cli/issues). Include:

- `erebyx --version`
- `erebyx doctor` output
- OS + AI client(s) affected
- Steps to reproduce
- Expected vs actual behavior

---

## Security disclosures

Don't open public issues for security findings. See [SECURITY.md](SECURITY.md).

---

**Built by EREBYX, LLC** — `https://erebyx.com`
