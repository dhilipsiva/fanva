# TODO.md — fanva

Open work, in rough priority order. Items ported from nibli's trackers at the
extraction rev (`NIBLI_REV`) are marked *(from nibli)*; the historical phased
backlogs live in `docs/archive/`. House style: a bullet is deleted entirely
when it lands.

## Ship it

- **Deploy fanva-ui + point the proxy at it** — pick a static host for
  `just build-ui`'s output, then `wrangler deploy` fanva-proxy with
  `ALLOWED_ORIGINS` set to the new origin (the deployed worker at
  `fanva-proxy.dhilipsiva.workers.dev` still serves nibli's UI until nibli's
  Lojban purge lands — coordinate before changing its allowed origins; local
  dev origins belong in `.dev.vars`, never the deployed var). Done when: the
  hosted app translates end-to-end with jbotci enabled and no CORS errors.
- **Live-key end-to-end test** *(from nibli, Phase 9 residue)* — with a real
  BYO key: translate a sample, watch the trace panel show tool calls + the
  three gate chips, confirm the tersmu deep-meaning view renders, and confirm
  `window.camxes_validate` is loaded (the camxes gate is fail-open — without
  the shim you are silently running 2 gates, not 3).

## Translator quality

- **System-prompt convergence tuning** *(from nibli, conditional follow-on)* —
  the current `LOJBAN_SYSTEM_PROMPT` seed is conservative and unmeasured;
  measure first-pass validity / attempts-to-converge across the 5 providers
  (needs live API keys) and grow the few-shot set toward the gerna ∧ smuni ∧
  camxes intersection.
- **Ground the system prompt in the grammar + dictionary** *(technique ported
  from nibli's Klaro-side plan)* — instead of hand-curated few-shots only,
  derive the prompt's grammar-fragment cheat-sheet from gerna's own EBNF
  doc-comment (`gerna/src/grammar.rs`) and the supported-vocabulary surface
  from `smuni-dictionary`, so the prompt can't drift from what the gates
  accept. The prompt-guard test already pins example validity in both
  dictionary modes.

## gerna / smuni backlog *(from nibli)*

- **GIhA quantified/description heads: share the head witness across tails** —
  currently rejected fail-closed (gerna `giha_safe_head`): the repeated-head
  desugar would re-quantify a `da`/`lo`-head per tail, splitting one surface
  scope into independent ∃s (wrong TRUE on disjoint witnesses —
  adversarial-review finding, 2026-07-10). The real fix is compiling the head
  ONCE (one witness/variable) and distributing only the tails — needs a
  smuni-level GIhA construct instead of the parse-time desugar. Would un-block
  Genesis 1:2 (`lo terdi cu na se tarmi gi'e kunti`), which today needs a name
  head or `.i je` restate.
- **Determinism corpus: add GIhA + negative-conjunct lines** —
  `determinism-corpus.lojban` predates both; add a `gi'e` chain, a `gi'enai`
  line, and a `P .i je na Q` sequence so the corpus (a parse-differential and
  fuzz-seed input here) pins the new shapes. Pairs with the GIhA item above.
- **Port the known-failures backlog into compiled tests** — the pinned
  gerna/smuni miscompilation cases carried from nibli
  (`docs/reference/known-failures/`) are written against nibli-engine APIs;
  re-express them as gerna/smuni-level regression tests (parse → compile →
  assert on the LogicBuffer) so they gate here instead of being documentation.

## Infrastructure

- **fanva-proxy: retire once jbotci CORS lands** *(from nibli)* — int19h is
  enabling direct browser→jbotci MCP calls on his end ("no reason it shouldn't
  be allowed from the browser"). When live: verify `initialize` + `tools/list`
  + `tersmu` from fanva-ui WITHOUT the proxy, then default the proxy URL to
  the direct endpoint and deprecate the Cloudflare Worker (keep the
  local-gates degradation path). Direct crate embedding was assessed
  2026-07-10 and is OFF the table on licensing: jbotci is AGPL-3.0-or-later
  (fanva is MIT OR Apache-2.0 — linking it into the wasm bundle would
  relicense the distributed bundle AGPL), unless int19h ever dual-licenses a
  core crate. Calling his hosted server over HTTP is arm's-length and clean —
  the CORS'd-MCP route IS the integration. (Also: his parser intentionally
  diverges from camxes ~500/22k — SA erasure, ZOI preprocessing — so it could
  never replace the camxes-std gate regardless.)
- **Prune the Klaro-era CSS** — `fanva-ui/assets/style.css` (69 KB) was
  carried wholesale from nibli-ui; the KB/query/proof/lint classes are dead in
  fanva. Prune once the UI surface settles (cosmetic; no rush).
- **Full-dictionary CI variant** — CI runs the curated fallback dictionary
  (175 entries). Consider a scheduled job that runs `just fetch-dict` first so
  `verify-dict` exercises the full ~10.9k-entry build (the lensisku endpoint
  401s HEAD requests; plain GET works — and note nibli's book CI used a
  token-gated URL variant, so verify which endpoint holds).
