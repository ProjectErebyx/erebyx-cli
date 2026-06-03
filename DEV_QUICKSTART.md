# Developer Quickstart — erebyx-cli

Five minutes from zero to your first wrap-up.

---

## 0. Prerequisites

- An EREBYX API key — get one at [app.erebyx.com/keys](https://app.erebyx.com/keys)
- `cargo` (Rust 1.75+)

---

## 1. Install (30 seconds)

```bash
cargo install erebyx
```

Confirm:

```bash
erebyx --version    # 0.1.1
erebyx health       # checks server + key
```

---

## 2. Configure (10 seconds)

```bash
export EREBYX_API_KEY="<YOUR_API_KEY>"
```

Optional overrides (rarely needed):

```bash
export EREBYX_API_URL="https://core.erebyx.com"
export EREBYX_INSTANCE_ID="my-laptop"
```

Persist these in your shell rc file so every shell sees them.

---

## 3. First save (10 seconds)

```bash
erebyx save "Substrate URL is core.erebyx.com" \
  --category knowledge \
  --anchors setup \
  --importance 0.6
```

Successful response:

```
✓ Saved memory mem_<id>
  category: knowledge
  anchors: [setup]
  importance: 0.6
```

---

## 4. First remember (10 seconds)

```bash
erebyx remember "substrate URL"
```

You'll see your save reflected back, ranked by semantic match.

---

## 5. First wrap-up (10 seconds)

```bash
erebyx wrap-up "Got the CLI working" \
  --whats-next "Wire it into my agent loop" \
  --anchors setup,cli \
  --energy systematic
```

That handoff is now retrievable next session via `erebyx load-context`.

**You're done.** That's the whole loop.

---

## X-Erebyx-Hint — lifecycle signals

Every CLI call returns lifecycle hints from the substrate. Read them in `--json` mode:

```bash
erebyx save "..." --json | jq '.hints'
# ["wrap_up_recommended"]
```

### Hint values and when to honor each

| Hint | Meaning | Recommended response |
|---|---|---|
| `wrap_up_recommended` | Substrate sees a natural consolidation boundary (sustained save volume, topic shift). | Run `erebyx wrap-up` when the current task feels complete. |
| `restore_identity_recommended` | Voice drift detected — your AI is wandering from established patterns. (v0.2) | Run `erebyx restore-identity` to re-anchor. |
| `load_context_recommended` | Retrieval scores trending low — your AI is operating without context it could have. | Run `erebyx load-context` to reload working memory. |
| `compact_imminent` | Sustained save volume; consolidate before the harness compacts the context window. | Run `erebyx wrap-up` before the harness forces a compact. |

Honoring hints is **optional** — the CLI never acts on them automatically. Hints are advisory; your harness or script decides cadence.

### Disabling hints

```bash
export EREBYX_HINTS_DISABLED=1
```

Disables hint emission server-side. Use only for debugging — hints are how the substrate tells you when to wrap-up.

---

## Cold-session auto-fire

The substrate runs `restore_identity` + `load_context` automatically on the first call against a fresh `(instance_id, session_id)` tuple. You don't need to call them manually unless you want explicit control.

The CLI surfaces this transparently:

```bash
erebyx save "First save of the session" --json | jq '.auto_fired'
# ["restore_identity", "load_context"]
```

### Opting out

```bash
export EREBYX_DISABLE_AUTO_FIRE=1
```

Use this if your harness handles its own session warm-up and you want the substrate to skip auto-fire. Most users want auto-fire **on** — it's the seamless integration story.

---

## Common errors and fix paths

| Error | Cause | Fix |
|---|---|---|
| `401 Unauthorized` | API key missing or wrong | Check `echo $EREBYX_API_KEY` is exported and starts with `erebyx_` |
| `404 Not Found` on `/v0/memory/remember` | Substrate version mismatch | Verify `erebyx health` reports `v0.1.1+` |
| `connection refused` | Wrong `EREBYX_API_URL` | Default is `https://core.erebyx.com`. Confirm with `erebyx health` |
| `accepted: false, reason: below_durability_threshold` | Save was filtered as low-signal | Lower `--importance` threshold or omit; default `min_durability=0.4` |
| Hints never appear | Save volume below substrate threshold | Hints emit at ~12 saves/session by default. Normal early-session behavior |
| MCP client doesn't see the server | Setup didn't run for that client | Run `erebyx doctor` to audit; `erebyx setup` to repair |

For ambiguous errors, run `erebyx doctor` — it audits your config across every detected MCP client and reports the failing surface.

---

## Per-harness integration examples

The CLI is one of several integration paths. For your specific harness, see the matching example in the substrate repo:

- [Claude Code](https://erebyx.com/core) — full lifecycle hooks, paste-the-JSON setup
- [Cursor](https://erebyx.com/core) — `.cursor/mcp.json` entry, hints automatic
- [Anthropic Agent SDK](https://erebyx.com/core) — raw tool-use loop
- [OpenAI Responses API](https://erebyx.com/core) — raw API integration
- [Letta](https://erebyx.com/core) — agent self-decides cadence
- [LangGraph](https://erebyx.com/core) — graph node lifecycle
- [AutoGen](https://erebyx.com/core) — multi-agent message events
- [CrewAI](https://erebyx.com/core) — role-based agent lifecycle
- [Raw HTTP / curl](https://erebyx.com/core) — single-line save loop
- [Future / unknown harness](https://erebyx.com/core) — protocol-level forward-compat

All examples honor the same `X-Erebyx-Hint` protocol described above.

---

## Next steps

- **Add to a script**: every command supports `--json` — pipe to `jq`, your agent loop, your CI
- **Per-AI-client setup**: re-run `erebyx setup` whenever you install a new MCP-capable AI
- **Diagnose drift**: when an AI feels off, `erebyx restore-identity` is your first move
- **Read the substrate docs**: [EREBYX Core docs](https://erebyx.com/core)

---

**Built by EREBYX, LLC** — `https://erebyx.com`
