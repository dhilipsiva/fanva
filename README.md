# fanva

**fanva** (Lojban: *translate*) is an agentic English→Lojban translator that
produces **verified** Lojban: an LLM drafts a translation, real compilers
reject anything invalid, and the exact compiler error is fed back to the model
until the output passes every gate — or the attempt budget runs out.

It runs entirely in your browser as a WASM app. Bring your own LLM API key
(Anthropic / OpenAI / OpenRouter / Gemini / any OpenAI-compatible endpoint);
the key lives in an in-memory signal, is sent only to your chosen provider,
and never touches a server we own.

## How it works

```
English ──▶ LLM (BYO key, multi-provider, tool-calling)
              │  may call jbotci MCP tools mid-turn (vlacku, cukta, gentufa, …)
              ▼
         candidate Lojban
              │
              ▼
   three-gate validator (fail-fast, all local):
     1. gerna   — grammar (a strict Rust parser for a fragment of Lojban;
                  the narrowest gate)
     2. smuni   — semantics/arity (compiles to first-order logic)
     3. camxes  — the official Lojban grammar (vendored ilmentufa camxes.js,
                  run in-browser)
              │
        pass all ──▶ advisory semantic verification: a fresh-context LLM judge
              │      compares the engine's own English back-translation of each
              │      line against the source (fail-open — the deterministic
              │      gates are the hard guarantee)
              │
        any fail ──▶ the compiler's error is appended to the conversation and
                     the model retries (bounded by max_attempts, an oscillation
                     guard, and history trimming)
```

The success condition is the intersection **gerna ∧ smuni ∧ camxes**: text the
strict fragment parser accepts, that compiles to well-formed logic, and that
the official grammar confirms.

## Workspace

| crate / dir | role |
|---|---|
| [`fanva/`](fanva) | the engine: agent loop, gates, 5-provider LLM layer, jbotci MCP client, semantic verifier (+ the `fanva-validate` batch bin) |
| [`fanva-ui/`](fanva-ui) | Dioxus 0.7 web app: translate box, per-attempt trace panel with gate chips, tersmu deep-meaning view, BYO-key settings |
| [`fanva-proxy/`](fanva-proxy) | Cloudflare Worker fronting `https://jbotci.app/mcp` (jbotci 403s browser Origins — this fixed reverse proxy strips them and adds CORS) |
| [`gerna/`](gerna) | the Lojban parser (recursive descent, arena ASTs, `goi.rs` pro-bridi resolution) — see [LOJBAN_COVERAGE.md](LOJBAN_COVERAGE.md) for the supported fragment (~70–75 % of practical Lojban) |
| [`smuni/`](smuni) | semantic compiler: AST → first-order `LogicBuffer` |
| [`smuni-dictionary/`](smuni-dictionary) | arity/place-structure/gloss dictionary (full build via `just fetch-dict`, curated fallback otherwise) |
| [`nibli-types/`](nibli-types), [`nibli-protocol/`](nibli-protocol), [`nibli-render/`](nibli-render) | shared AST/logic types and the deterministic English back-translation renderer |
| [`fanva-verify/`](fanva-verify) | gerna's conformance suite: the gerna ↔ camxes parse-differential + the Predilex dictionary-arity differential |
| [`fuzz/`](fuzz) | `fuzz_parse` libFuzzer target over the gerna pipeline (LSan guards the arena's leak-free invariant) |
| [`python/`](python) | the Lojban model flywheel: deterministic classifier, Claude-generated training data (validated via `fanva-validate`), QLoRA fine-tune scripts |

## Quick start

Requires [Nix with flakes](https://nixos.org/) (or provide dx/wrangler/node/
just yourself):

```sh
nix develop          # or `direnv allow` once
just ui              # dx serve → http://localhost:8080
```

Open the settings modal, pick a provider, paste an API key, translate. jbotci
tool-use ships **off**; enabling it points the app at the proxy, which gives
the model dictionary (`vlacku`), reference-grammar (`cukta`), and parser
(`gentufa`) tools mid-translation, plus the tersmu deep-meaning view.

Common recipes (`just --list` for all): `just test`, `just test-fanva`,
`just verify-parser` (the camxes differential), `just verify-dict`,
`just fuzz-parse 60`, `just build-ui`.

## jbotci and the proxy

[jbotci](https://jbotci.app) is a third-party Lojban MCP server (AGPL-3.0 — a
remote service we merely call; no code linkage). It rejects any request
carrying a browser `Origin` header, so `fanva-proxy/` exists solely to strip
that header and add the CORS jbotci doesn't implement. The proxy holds no
secrets and is optional — with it unset, translation runs fully local. If
jbotci ever enables CORS for browsers, the proxy can retire.

## Deployment

The hosted app lives at [`dhilipsiva.dev/fanva`](https://dhilipsiva.dev/fanva/).
fanva-ui is a static WASM bundle with no server; the **production** build runs in
the external `dhilipsiva/dhilipsiva.dev` site repo, so **shipping = merging to
`main`** (which pings the site to rebuild). `just build-ui` here is only a
local pre-merge sanity build. For the build/host model, the `/fanva/` subpath, the
dictionary step, and the optional jbotci proxy, see [`DEPLOY.md`](DEPLOY.md).

## Conformance

- **Parse-differential** (`just verify-parser`): every sentence gerna accepts
  must parse under the official grammar (ilmentufa camxes, pinned in the Nix
  flake at the same rev as the browser vendor copy). Current status: 411/411
  corpus + generated lines agree, zero divergences.
- **Dictionary-arity differential** (`just verify-dict`): smuni-dictionary's
  arities cross-checked against the independent CC0 Predilex thesaurus.
- **Fuzzing** (`just fuzz-parse`): libFuzzer + LeakSanitizer over the parser,
  seeded from the shipped `.lojban` corpora.

## Provenance

fanva was extracted from [nibli](https://github.com/dhilipsiva/nibli) — it was
the *Formalize* feature of nibli's Transparency Triad, in legacy-Lojban mode —
when nibli pivoted its knowledge-base language away from Lojban. The exact
source revision is recorded in [`NIBLI_REV`](NIBLI_REV); the genesis research
prompt and historical trackers live under [`docs/`](docs). Extraction was
copy-only: nothing was deleted from nibli by this repo's creation.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option. Third-party attributions (vendored ilmentufa camxes, MIT; Predilex,
CC0) are in [NOTICE](NOTICE).
