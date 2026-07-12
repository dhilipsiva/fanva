# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state vs. intended project

This repo is currently a bare `cargo init` scaffold: `src/main.rs` is a hello-world `main()`
and `Cargo.toml` (single binary crate `fanva`, `edition = "2024"`) has **no dependencies**.
Essentially none of the real project has been built yet.

`TODO.md` is the authoritative spec. It is a phased, execute-item-by-item backlog (Phases 0–9)
for turning this scaffold into **`fanva`, an agentic English→Lojban translator**. Before doing
any substantive work, read `TODO.md` — and in particular its **"Ground truth / do-not-drift"**
header, which every later item references. Treat that header as the source of truth for pinned
revisions, verified/unverified upstream APIs, and MCP/provider protocol facts.

Items in `TODO.md` marked **unverified-pending-local-probe** must be confirmed against a live
service or a real clone before writing code against them — do not code blind against a `⚠️
UNVERIFIED` signature. Many items' "Done when" criteria require recording what a probe found in
a `docs/*.md` file (`docs/nibli-api.md`, `docs/mcp-probe.md`, `docs/proxy.md`, `docs/dictionary.md`,
`docs/providers.md`) — later phases read those records instead of re-probing, so write them.

When planning work across items, use `TODO.md`'s **"Parallelization notes"** section (bottom of
the file) for the phase dependency graph — e.g. Phases 1 and 2 are independent, and Phase 4 no
longer depends on Phase 2 since the Official gate went local.

## What the project is (big picture)

An agentic translator that takes English and produces **verified** Lojban. Two workspace members
are planned (they do not exist yet):

- `fanva-ui/` — a **Dioxus 0.7.x WASM** web app (`dx serve`/`dx build`). Holds all the logic:
  the LLM client, the MCP client, the three-gate validator, and the agentic loop.
- `fanva-proxy/` — a **Cloudflare Worker** (`wrangler`) that proxies browser requests to the
  `jbotci` MCP server. **This proxy is not optional:** jbotci enforces an Origin allowlist and
  returns **403** to any browser Origin, so a server-to-server proxy (which forwards no browser
  Origin) is the only way the WASM app can reach it.

### The core loop (planned, Phases 1–5)

```
English ──▶ LLM (BYO key, multi-provider, tool-calling)
              │  can call jbotci MCP tools mid-turn (dictionary, grammar, morphology…)
              ▼
         candidate Lojban
              │
              ▼
   three-gate validator (fail-fast, cheapest-first):
     1. gate_gerna   → gerna::parse_checked         (local, syntax)
     2. gate_smuni   → smuni::compile_from_gerna_ast (local, semantics/arity)
     3. official_gate → vendored local `camxes.js` (ilmentufa, MIT) via JS-interop (grammar gate)
              │
        pass all ──▶ Success (valid Lojban + LogicBuffer, then tersmu meaning check)
        any fail  ──▶ append the exact compiler/gate error to history, re-prompt, retry
                      (bounded by max_attempts + oscillation guard)
```

The design target is the **intersection** gerna ∧ smuni ∧ camxes — gerna is the narrowest gate
and the binding constraint, which is why `max_attempts` caps runaway LLM cost. (All three gates are
local; jbotci `gentufa` stays an LLM tool during translation, not a validator gate — see `TODO.md`.)

### Upstream dependencies (from `github.com/dhilipsiva/nibli`, pinned to one rev)

`gerna` (parse), `smuni` (semantic compile), `nibli-render` (English gloss), `smuni-dictionary`
(arity/place-structure). All four must be pinned to the **same** git rev so the shared
`nibli-types` crate dedups. The pin SHA belongs in a top-of-repo `NIBLI_REV` file — **not created
yet** (a Phase 0 item; get the SHA via `git ls-remote https://github.com/dhilipsiva/nibli.git main`
and record it there and in the Ground-truth header). `nibli-render::render_logic_buffer`
and `smuni_dictionary::back_translate` are **unverified** — confirm before use.

## Development environment (Nix flake + WSL)

This repository lives in **WSL** (`/home/dhilipsiva/projects/dhilipsiva/fanva`) while Claude Code
runs on the **Windows host**. **Run every command inside WSL** — never build from the Windows side
over the `\\wsl.localhost\...` UNC path: Cargo's incremental cache corrupts over the 9P mount,
`git` reports "dubious ownership", and tool PATHs differ (e.g. `npm` leaks in from a Windows mount).

The dev environment is a **Nix flake** (`flake.nix`; inputs pinned in `flake.lock`). Nix with
flakes is already set up in this WSL. The flake provides a pinned Rust toolchain (stable +
`wasm32-unknown-unknown`, with clippy/rustfmt/rust-analyzer), `dioxus-cli` (`dx` 0.7.x),
`wrangler`, `nodejs`, and `just`. Enter it either way:
- `nix develop` — one-off shell with all tools on PATH.
- `direnv allow` — the checked-in `.envrc` auto-loads the flake on `cd`. The flake's cargo/rustc
  shadow the global rustup toolchain in `~/.cargo/bin`.

**Invoking WSL from the Windows host (e.g. the Bash tool):**
- Pattern: `wsl.exe -- bash -lc 'cd /home/dhilipsiva/projects/dhilipsiva/fanva && nix develop --command <cmd>'`
  (or enter the shell first). For a command with pipes/quotes/loops, wrap it as
  `nix develop --command bash -lc "<full command line>"` — and if it's non-trivial, prefer the
  script-file method below, which survives both the `wsl.exe` and `--command` layers.
- Do **not** use `wsl.exe --cd <linux-path>` — it fails with `ERROR_PATH_NOT_FOUND` when the Windows
  cwd is a UNC path.
- Anything with loops, quotes, or `$vars` gets mangled when inlined through `wsl.exe` from Git Bash.
  Instead write a script file (the editor can write to `\\wsl.localhost\ubuntu\tmp\x.sh`) and run
  `wsl.exe -- bash -lc 'bash /tmp/x.sh'`.

## Commands

All commands run **inside the flake dev shell, inside WSL** (see above) — either from a
`nix develop` shell or prefixed with `nix develop --command`.

### Works today (single-crate scaffold)
- Build: `cargo build` · Run: `cargo run` · Test: `cargo test`
- Format / lint: `cargo fmt` / `cargo clippy`
- Enter/refresh the toolchain: `nix develop` · `nix flake update` (re-pin inputs). `nix flake check`
  currently only confirms the flake evaluates and the dev shell builds — there are no `checks` /
  `packages` outputs yet; add them (e.g. `cargo clippy` / `cargo fmt --check`) once the crates land.

### Planned (tooling is provided by the flake; the `fanva-ui` / `fanva-proxy` crates it targets don't exist yet — per `TODO.md`)
- Dev server (WASM UI): `dx serve --platform web`
- Release web build: `dx build --release --platform web` (or `dx bundle`)
- Native unit tests (pure-logic modules: providers, gates, agent):
  `cargo test -p fanva-ui --lib`
- Single test module / test: `cargo test -p fanva-ui --lib gates::` or
  `cargo test -p fanva-ui --lib <test_name>`
- WASM tests (gloo-net / MCP client): `wasm-pack test --node`; the camxes `official_gate`
  (JS-interop) needs a **browser** test: `wasm-pack test --headless`. Note: `wasm-pack` is
  **not in the flake yet** — add it to `flake.nix` before Phase 4/8 testing.
- Proxy dev / deploy: `wrangler dev` / `wrangler deploy` (in `fanva-proxy/`)

Testing strategy per `TODO.md`: pure-logic modules stay on **native `cargo test`** with a mocked
`chat()` and mocked `McpClient`; wasm-only modules use `wasm-pack test --node`, except the
JS-interop `official_gate`, which native tests and `--node` cannot exercise — it needs the
headless-browser wasm test.

## Non-obvious things to get right

- **Proxy CORS is the whole reason `fanva-proxy` exists.** The Worker must NOT forward the browser
  `Origin` to jbotci, must answer `OPTIONS` with the app's CORS headers, and must relay the
  `Mcp-Session-Id` response header back. See Phase 0 in `TODO.md` for the exact header list.
- **jbotci MCP** = Streamable-HTTP JSON-RPC at `https://jbotci.app/mcp`, protocol `2025-06-18`.
  Requests need `Accept: application/json, text/event-stream`; responses may come back as EITHER
  `application/json` OR `text/event-stream` (SSE) — the client must handle both. Send
  `notifications/initialized` after `initialize`. Discover the 7 tools via `tools/list` at runtime
  — **do not hardcode tool schemas**.
- **Per-provider tool-calling shapes differ** (Anthropic vs OpenAI/OpenRouter vs Gemini) — request
  format, tool-call parsing, and tool-result submission all diverge. The exact shapes are
  documented in `TODO.md`'s Ground-truth header; follow it rather than guessing. Notably OpenAI
  tool-call `arguments` is a **stringified JSON** that must be parsed and validated.
- **BYO-key model:** the LLM API key lives only in a Dioxus signal (in memory), and requests go
  straight to the provider. The only server the app owns is the jbotci proxy, which holds no
  secrets.
- **Vendoring `camxes.js` has license strings attached** (per the [DECISION] in `TODO.md`):
  vendor the **standard grammar only** (not beta/cbm/ckt), pinned to a specific ilmentufa commit,
  and ship a `NOTICE` with the MIT attribution (ilmentufa + Masato Hagiwara). fanva has **no
  `LICENSE` of its own yet** — pick one before finalizing that attribution. Also verify the
  vendored file's module format (ESM vs UMD/global) and error shape before binding it from Rust.
