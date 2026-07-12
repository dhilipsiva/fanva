# fanva (engine crate)

The agentic English→Lojban translator engine — no UI, no Dioxus. The web shell
lives in [`../fanva-ui`](../fanva-ui); the jbotci CORS proxy in
[`../fanva-proxy`](../fanva-proxy).

Extracted from nibli's Transparency Triad formalizer (`nibli-fanva`) at the rev
recorded in the repo-root `NIBLI_REV`; the Klaro arm stayed in nibli, so this
crate is Lojban-only.

## The loop

```
English ──▶ LLM (BYO key, multi-provider, tool-calling)
              │  may call jbotci MCP tools mid-turn (vlacku, cukta, gentufa, …)
              ▼
         candidate Lojban
              │
              ▼
   three-gate validator (fail-fast, cheapest-first):
     1. gerna::parse_checked            (local, grammar — the narrowest gate)
     2. smuni::compile_from_gerna_ast   (local, semantics/arity)
     3. official_gate → vendored camxes.js (ilmentufa, MIT) via JS-interop
        (wasm-only; degrades to Ok when the shim is not loaded)
              │
        pass all ──▶ fresh-context semantic verification turn (advisory,
              │      fail-open: an LLM judge reads the nibli-render
              │      back-translation and compares meaning to the source)
              │
        any fail ──▶ append the exact compiler/gate error to the conversation,
                     re-prompt, retry (bounded by max_attempts + an
                     oscillation guard + history trimming)
```

The success condition is the intersection **gerna ∧ smuni ∧ camxes**. jbotci
(`https://jbotci.app/mcp`, reached through `fanva-proxy` because jbotci 403s
browser Origins) is optional tooling: with no proxy configured the loop runs
tool-free on the local gates and flags the outcome `degraded`.

## Modules

| module | role |
|---|---|
| `agent` | `translate_agentic` — the outer self-correcting loop, `Outcome`/`Attempt` trace |
| `gates` | `GateError`, `local_gates`, `validate`, `validate_kb`, the camxes `official_gate`, `feedback_for` |
| `llm` | `Chat`/`ToolChat` seams, 5-provider request/response shaping, `HttpChat` (wasm), `LOJBAN_SYSTEM_PROMPT` |
| `mcp` | `McpClient` — Streamable-HTTP JSON-RPC jbotci client (via the proxy), `tersmu` wrapper |
| `tools` | the jbotci tool loop (`run_llm_tool_loop`), `ToolCallTrace` |
| `verify` | the fresh-context semantic verification turn (back-translation + judge prompt + verdict parsing) |

## Testing

- Native (`cargo test -p fanva --lib` / `just test-fanva`): the agent loop,
  gates, provider shapes, MCP wire — all with mocked `ToolChat`/empty-proxy
  `McpClient`; the gate tests use the REAL gerna/smuni compilers.
- Wasm (`wasm-pack test --node fanva` / `just test-fanva-wasm`): the camxes
  JS-marshalling and the shim-absent degrade path. The real camxes engine needs
  a browser and is verified manually through `fanva-ui`.
- `fanva-validate` (`src/bin/validate.rs`): the batch stdin validator the
  python Lojban flywheel subprocesses.
