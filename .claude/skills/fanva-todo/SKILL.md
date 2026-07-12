---
name: fanva-todo
description: Work a single item from TODO.md end-to-end for the agentic Lojban translator repo — plan, implement, test/validate/verify (cargo + wasm build; the three-gate gerna+smuni+jbotci-gentufa validator; the jbotci MCP proxy + tool-use loop), then remove the item from TODO.md (or update it if only partially done), commit, and push. Use for the fanva translator's TODO items, one at a time. Trigger when the user says "work the next TODO item", "do the next fanva task", "pick up TODO.md", or names a specific TODO bullet.
---

# fanva-todo — one TODO item, end to end

Advance the agentic Lojban translator (`fanva`) by completing **exactly one** item from
`TODO.md` per invocation: plan → implement → verify → tidy docs → commit → push. Never batch
multiple items. Never commit unverified or red code.

This project is a Dioxus/WASM app (`fanva-ui`) plus a tiny pass-through proxy (`fanva-proxy`). It
depends on the public nibli crates (`gerna`, `smuni`, optionally `nibli-render`/`smuni-dictionary`)
as git deps, and integrates the **jbotci** Lojban toolkit (`https://jbotci.app/mcp`) as a
first-class citizen — the LLM calls jbotci tools (dictionary/grammar/parser/semantics) mid-
translation, and the app uses jbotci `gentufa`/`tersmu` as oracles. The core value is the self-
correcting translate→validate→feedback loop. The **success gate is three gates, all of which must
pass**: `gerna::parse_checked` (engine grammar) + `smuni::compile_from_gerna_ast` (engine semantics)
+ jbotci `gentufa` (official grammar). jbotci is reachable only via `fanva-proxy` (jbotci 403s
cross-origin browser calls).

## Environment

This project is developed on WSL2 (Ubuntu). Run every shell command through the WSL wrapper:

```
wsl -d Ubuntu -e bash -lc "cd ~/path/to/fanva-repo && <CMD>"
```

Toolchain: `cargo` + `dx` (Dioxus CLI) for `fanva-ui`, plus the proxy platform's CLI (e.g.
`wrangler` for Cloudflare Workers) for `fanva-proxy`. Native unit tests run on the host target; the
app builds for `wasm32-unknown-unknown` via `dx build` / `dx serve`.

**jbotci reachability.** Items that touch jbotci (`gentufa` gate, `tersmu` meaning, LLM tool-use)
need the proxy reachable. Prefer testing the app-side logic against a **mocked MCP client** (so
tests are hermetic and offline), and separately verify the live path with a curl through the proxy.
A quick liveness probe of the proxy (replace the URL):
`curl -s -X POST "$PROXY_URL" -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fanva","version":"0.1.0"}}}'`
should return a 200 with jbotci's `serverInfo`. (A direct browser-origin call to jbotci itself
returns 403 — that block is exactly what the proxy exists to bypass.)

## Procedure

1. **Pick the item.** Read `TODO.md`. Choose the **first unchecked bullet in the earliest
   incomplete phase** — that is the next actionable task. Restate the item verbatim and quote its
   `Done when:` acceptance criterion. If the user named a specific bullet, use that instead.
   Do **not** pull in later items or expand scope beyond this one bullet.

2. **Plan (briefly).** Identify the exact file(s), function(s), and signature(s) the item touches.
   Reuse existing patterns (the ported `llm.rs`, `validate.rs`, `agent.rs`, the `nibli-ui`
   scaffolding). If the item is ambiguous, under-specified, or blocked by an unfinished earlier
   item, **stop and ask** rather than guessing.

3. **Implement** the single item.

4. **Test / validate / verify.** Do this in its **own step, before any commit** — never chain
   verify-and-push. Choose what the item's `Done when:` requires:
   - Type-check: `cargo check`.
   - Unit tests: `cargo test` (run the specific test the `Done when:` names; the three-gate
     validator and the agent — with a mocked `chat` AND a mocked MCP client — are pure Rust and test
     on the native target, hermetically and offline).
   - jbotci path: for items touching the proxy, the `gentufa`/`tersmu` oracles, or the LLM tool-use
     loop, verify the live leg with the proxy liveness curl above (or a scripted `tools/call` for
     `gentufa` on a known-good and a known-bad sentence), and confirm graceful degradation when the
     proxy is unreachable (local gerna+smuni still gate; a banner shows). Never hard-fail the app on
     jbotci being down.
   - WASM build: `dx build` (or `cargo build --target wasm32-unknown-unknown`) must succeed for
     any item touching browser code.
   - UI behavior: if `Done when:` is an observable UI behavior, `dx serve` and verify it (attempt
     trace renders, converges to ✓, back-translation shows, etc.). Report what you observed.
   - **Confirm the `Done when:` criterion actually holds and paste the evidence** (test output /
     command exit / observed behavior). If it fails, fix it or report the blocker — do not proceed
     to commit with a red or unmet criterion.

5. **Update `TODO.md`.** Keep it truthful:
   - If the item is **fully done**, DELETE the bullet entirely (do not strike it through or mark it
     COMPLETE — remove it).
   - If **partially done**, UPDATE the bullet to state exactly what remains; relocate any newly-
     discovered sub-items to the correct phase. Never leave a stale claim.
   - Items are plain bullets (`- `), never numbered; reference any related item by its text.

6. **Update docs** if this item changed user-facing behavior or capabilities (README, a
   transparency note, etc.). Skip if purely internal.

7. **Commit** all changes together — code + docs + `TODO.md` — with a Conventional Commits message
   describing the item. End the message with:

   ```
   Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
   ```

   (Write the message to a temp file and `git commit -F` — heredocs/`-m` mangle backticks and
   punctuation inside the WSL `bash -lc` wrapper.)

8. **Push** to the repo (SSH tends to hang in this environment — push over HTTPS via the `gh`
   credential helper):

   ```
   git -c credential.helper='!gh auth git-credential' push https://github.com/<owner>/<fanva-repo>.git HEAD:main
   ```

9. **Report** in a couple of lines: what landed, the evidence that its `Done when:` is satisfied,
   and what the **next** first-remaining bullet now is. Then stop — one item per invocation.

## Guardrails

- **One item per run.** If tempted to knock out "just one more," stop instead.
- **Never commit red.** A failing build or test means fix-or-report, not commit.
- **Verify in a separate step from pushing.** Read the test/CI result in its own tool call before
  running the commit+push — do not `;`-chain them.
- **Honesty over green.** If a `Done when:` can't be met as written, say so and propose an amended
  item rather than quietly weakening the check.
- **Scope discipline.** Out-of-scope improvements you notice become *new bullets in `TODO.md`*, not
  extra work in this commit.
