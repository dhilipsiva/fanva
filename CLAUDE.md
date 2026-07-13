# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

**fanva** — an agentic English→Lojban translator, extracted from
`github.com/dhilipsiva/nibli` (it was the Transparency Triad's "Formalize"
feature in legacy-Lojban mode) at the rev pinned in `NIBLI_REV`. The extraction
was copy-only; the Klaro/reasoning side stayed in nibli. `README.md` has the
architecture overview; `TODO.md` tracks open work; `docs/` holds provenance
material (genesis prompt, historical trackers, known-failures reference).

### The core loop (implemented, in `fanva/`)

```
English → LLM (BYO key; Anthropic/OpenAI/OpenRouter/Gemini/Custom; may call
           jbotci MCP tools mid-turn through fanva-proxy)
        → candidate Lojban
        → gates: gerna::parse_checked → smuni::compile_from_gerna_ast →
                 camxes official_gate (wasm-only, vendored ilmentufa)
        → advisory fresh-context semantic verifier (fail-open)
        → on any failure: exact compiler error appended to history, retry
          (max_attempts, oscillation guard, MAX_HISTORY_PAIRS=3 trim)
```

Success = the intersection **gerna ∧ smuni ∧ camxes**; gerna is the narrowest
gate and the binding constraint.

### Workspace members

`fanva` (engine + `fanva-validate` bin) · `fanva-ui` (Dioxus 0.7 web app; the
ONLY crate with a UI) · `fanva-verify` (parse-differential + Predilex gates) ·
`gerna` · `smuni` · `smuni-dictionary` · `nibli-types` · `nibli-protocol` ·
`nibli-render` (upstream names kept for provenance). Excluded from the
workspace: `fuzz/` (cargo-fuzz package) and `fanva-proxy/` (JS Cloudflare
Worker). The `nibli-*` crates and `gerna`/`smuni`/`smuni-dictionary` are
vendored copies — all path-deps resolve to the ONE in-repo `nibli-types`
(a git-dep mix would create two incompatible `AstBuffer` types).

## Development environment (Nix flake + WSL)

This repository lives in **WSL** (`/home/dhilipsiva/projects/dhilipsiva/fanva`) while Claude Code
runs on the **Windows host**. **Run every command inside WSL** — never build from the Windows side
over the `\\wsl.localhost\...` UNC path: Cargo's incremental cache corrupts over the 9P mount,
`git` reports "dubious ownership", and tool PATHs differ (e.g. `npm` leaks in from a Windows mount).

The dev environment is a **Nix flake** (`flake.nix`; inputs pinned in `flake.lock`). It provides
the pinned Rust toolchain (stable + `wasm32-unknown-unknown`), `dx` (dioxus-cli), `wrangler`,
`nodejs`, `just`, `wasm-pack`, `cargo-fuzz` + a pinned nightly (`NIBLI_NIGHTLY_BIN`), `python3`,
and the pinned ilmentufa checkout (`NIBLI_CAMXES_DIR`). Enter it with `nix develop` or
`direnv allow` (checked-in `.envrc`).

**Invoking WSL from the Windows host (e.g. the Bash tool):**
- Pattern: `wsl.exe -- bash -lc 'cd /home/dhilipsiva/projects/dhilipsiva/fanva && nix develop --command <cmd>'`
  (or enter the shell first). For a command with pipes/quotes/loops, wrap it as
  `nix develop --command bash -lc "<full command line>"` — and if it's non-trivial, prefer the
  script-file method below.
- Do **not** use `wsl.exe --cd <linux-path>` — it fails with `ERROR_PATH_NOT_FOUND` when the Windows
  cwd is a UNC path.
- Anything with loops, quotes, or `$vars` gets mangled when inlined through `wsl.exe` from Git Bash.
  Instead write a script file (the editor can write to `\\wsl.localhost\ubuntu\tmp\x.sh`), strip CR
  (`sed -i 's/\r$//' /tmp/x.sh`), and run `wsl.exe -- bash -lc 'bash /tmp/x.sh'`.

## Commands

All inside the flake dev shell, inside WSL. `just --list` shows everything; the load-bearing ones:

- Workspace tests: `just test` (= `cargo test --workspace`)
- Engine tests: `just test-fanva` (`cargo test -p fanva --lib`) · single module:
  `cargo test -p fanva --lib gates::` · parser suite: `just test-gerna`
- Wasm tests (camxes marshalling): `just test-fanva-wasm` (`wasm-pack test --node fanva`)
- UI wasm check without dx: `just check-ui-wasm`
- Conformance gates: `just verify-parser` (gerna↔camxes differential; real run needs the
  Nix shell's node + `NIBLI_CAMXES_DIR`, else it self-skips) · `just verify-dict` (Predilex)
- Fuzz: `just fuzz-seed && just fuzz-parse 60` (needs `NIBLI_NIGHTLY_BIN`; LSan stays on)
- UI dev server: `just ui` (= `cd fanva-ui && dx serve --port 8080`) · release build:
  `just build-ui` (output `target/dx/fanva-ui/release/web/public/`)
- Dictionary: `just fetch-dict` (optional 10 MB lensisku export → full-vocab
  smuni-dictionary build; without it build.rs falls back to ~175 curated entries with a
  cargo:warning — tests are written to pass in both modes)
- Flywheel: `just classify`, `just test-classifier`, `just generate-training` (needs
  `ANTHROPIC_API_KEY`), `just model-train|eval|refine|push`
- Proxy: `cd fanva-proxy && npm install && npx wrangler dev` (see `fanva-proxy/DEPLOY.md`;
  the acceptance test is a curl of the MCP `initialize` through the local worker)
- CI aggregate: `just ci` / `just ci-all` (what `.github/workflows/ci.yml` runs)

## Non-obvious things to get right

- **The camxes gate is fail-open.** `official_gate` returns `Ok` when
  `window.camxes_validate` is absent (shim not loaded, or native target). Nothing errors —
  translation silently runs on 2 gates instead of 3. When touching the UI's script tags or the
  vendor dir, verify in the browser that `window.camxes_validate` is defined.
- **Two camxes artifacts, one pin.** The flake's `ilmentufa` input (node CLI used by
  `verify-parser`) and the vendored browser files in `fanva-ui/assets/js/vendor/camxes/` must
  stay at the SAME upstream rev (currently `778ea13…`, recorded in the flake URL, `flake.lock`,
  `VENDOR.md`, and `NOTICE`). Refresh procedure is in `VENDOR.md`; only `camxes_shim.js` is
  first-party — never edit `camxes.js`/`camxes_preproc.js`.
- **Layout-sensitive `include_str!`s.** `fanva-verify/src/corpora.rs` and gerna's test modules
  embed the repo-root `*.lojban` corpora via `../../` paths, and `fanva-verify/src/predilex.rs`
  embeds `../vendor/predilex/*.csv` (byte-exact; `.gitattributes` pins LF — don't let a Windows
  checkout CRLF them). Moving crates or corpora breaks compiles.
- **jbotci MCP client invariants** (`fanva/src/mcp/`): Streamable-HTTP at protocol
  `2025-06-18`; responses arrive as `application/json` OR `text/event-stream` (both handled in
  `wire.rs`); `Mcp-Session-Id` echoed, 404-with-session ⇒ re-initialize; empty proxy URL ⇒
  `is_available() == false` ⇒ the agent runs tool-free and flags `degraded`. Tool schemas are
  discovered via `tools/list` — never hardcoded.
- **Per-provider quirks are pinned in code** (`fanva/src/llm/request.rs`): Anthropic needs
  `anthropic-dangerous-direct-browser-access: true` and NO temperature; OpenAI tool-call
  `arguments` is stringified JSON; Gemini uses role `"model"`, correlates tool results by NAME,
  requires `thoughtSignature` echo, and rejects raw JSON Schema (see `sanitize_gemini_schema`).
- **BYO-key model:** the API key lives only in a Dioxus signal; requests go browser-direct to
  the provider. The only server this repo owns is `fanva-proxy` (no secrets; jbotci itself is
  AGPL-3.0 — a remote service, no code linkage).
- **The deployed worker is shared.** `https://fanva-proxy.dhilipsiva.workers.dev/mcp` (the
  UI's prefilled default) currently also serves nibli's UI until nibli's Lojban purge lands.
  Don't redeploy or change `ALLOWED_ORIGINS` casually; local dev origins go in `.dev.vars`.
- **The prompt is grammar-grounded; its guard tests are a gate.** `LOJBAN_SYSTEM_PROMPT` is
  assembled at runtime (a `LazyLock<String>`) with its grammar block embedded verbatim from
  `gerna::GRAMMAR_EBNF`, so the prompt's grammar can't drift from the parser. The
  `system_prompt.rs` tests pin this (the block equals `gerna::GRAMMAR_EBNF`), keep every
  few-shot example gate-valid, AND require every example word to be a real `smuni-dictionary`
  entry (smuni defaults unknown words to arity 2, so a plain gate-parse wouldn't catch an
  out-of-dictionary word). Editing the prompt means keeping it green in BOTH dictionary modes
  (fallback and full) — if the fallback build flags a word, add it to the curated set in
  `smuni-dictionary/build.rs`. On the gerna side, `gerna::GRAMMAR_EBNF` is itself grounded by
  `grammar_ebnf_constructs_parse` (one parse-verified example per documented construct); the
  const and that test move together with the parser. NOTE: toggling `dictionary-en.json` to
  switch dict modes needs a `cargo clean -p smuni-dictionary` (an `mv`-restored file keeps its
  old mtime, so the build can otherwise stay cached in the wrong mode).
- **`fanva-validate` is the python flywheel's contract:** JSON-per-line on stdout, accepts
  `--lang lojban` for compatibility. `generate_training_data.py`/`nibli_model.py` subprocess
  `target/{debug,release}/fanva-validate`.
