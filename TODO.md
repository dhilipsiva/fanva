# TODO.md — fanva

Open work, in rough priority order. Items ported from nibli's trackers at the
extraction rev (`NIBLI_REV`) are marked *(from nibli)*; the historical phased
backlogs live in `docs/archive/`. House style: a bullet is deleted entirely
when it lands.

## Ship it

- **Deploy fanva-ui at `dhilipsiva.dev/fanva` (site-repo owned)** — the fanva-side
  deploy wiring has landed: [`DEPLOY.md`](DEPLOY.md) runbook, the
  `redeploy-site.yml` trigger (`event_type=fanva-updated`, self-skips until
  `SITE_DISPATCH_TOKEN` exists), and the root-relative `just build-ui` sanity build.
  The proxy needs **no** change — the fanva origin `https://dhilipsiva.dev` is already
  in `fanva-proxy`'s `ALLOWED_ORIGINS`. What remains is **owned by the external
  `dhilipsiva/dhilipsiva.dev` repo session** (like voksa/nibli): the production build
  applying the `/fanva/` base_path, hosting the bundle (with `just fetch-dict` before
  `dx build`), and registering the `fanva-updated` dispatch handler + this repo's
  `SITE_DISPATCH_TOKEN` secret. The live end-to-end acceptance is the *Live-key
  end-to-end test* bullet below.
- **Live-key end-to-end test** *(from nibli, Phase 9 residue)* — with a real
  BYO key: translate a sample, watch the trace panel show tool calls + the
  three gate chips, confirm the tersmu deep-meaning view renders, and confirm
  `window.camxes_validate` is loaded (the camxes gate is fail-open — without
  the shim you are silently running 2 gates, not 3).

## Translator quality

- **System-prompt convergence tuning — measure more providers + iterate** — the
  measurement harness (`fanva/src/bin/measure.rs`, `--features measure`; drives the
  real loop + scores 3-gate with camxes-via-node) and a first data-driven tuning pass
  have landed (see [`docs/convergence-measurement.md`](docs/convergence-measurement.md)):
  added few-shots/rules for the ditransitive place structure, vowel-final cmevla, and
  the `ko'a` pronoun, all gate-valid in both dictionary modes. First run (2026-07-13,
  full dict): Gemini 83%→100%, Sarvam 100%, NVIDIA-8b 79% (grammar cases fixed, net
  bounded by vocabulary). Remaining: measure **Anthropic + OpenAI** (no keys yet) and
  **OpenRouter** (its free tier is capped at 8 req/min → 0 rows; needs credits or
  pacing); then iterate. The prompt's grammar is now embedded from `gerna::GRAMMAR_EBNF`
  (drift-proof) and its example vocabulary is drift-guarded against `smuni-dictionary`;
  the remaining lever for the weak-model vocabulary ceiling is injecting a
  *source-relevant* vocabulary subset into the prompt at request time (nibli's Option C —
  not done; needs English→gloss matching, and `system_prompt()` is already a runtime
  builder so it's a natural extension).

## gerna / smuni backlog *(from nibli)*

- **GIhA shared-head: handle the fail-closed corners** — the `Sentence::SharedHead`
  fix (quantified/description-head GIhA binds ONE witness) fails closed on three
  sub-shapes it doesn't model: a connected sumti (`.e`/`.a`) in the shared HEAD
  (rejected in gerna `head_has_connective`), and a BAI modal or connected sumti in a
  shared-head TAIL (rejected in smuni `compile_shared_head`/`build_giha_branch`).
  Handle them (distribute the shared-head unit / model the modal) to lift the last
  `.i je` restate requirement. Low priority — all fail closed (sound, not a silent
  wrong answer). *(The determinism-corpus GIhA + negative-conjunct lines are now
  seeded there for the fuzzer, and pinned under camxes in
  `fanva-verify/tests/parser_differential.rs` + gerna's negative-conjunct test.)*

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
