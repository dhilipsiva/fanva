# System-prompt convergence measurement

How `LOJBAN_SYSTEM_PROMPT` (`fanva/src/llm/system_prompt.rs`) is measured and tuned,
and the results of the first pass (2026-07-13).

## The harness

`fanva/src/bin/measure.rs` (dev-only, behind `--features measure`) drives the **real**
agentic loop — `fanva::agent::translate_agentic` with the shipped prompt — against a
live provider over a small English corpus, then independently scores **every attempt**
with the full success condition **gerna ∧ smuni ∧ camxes**:

- gerna + smuni run in-process (`fanva::gates`);
- **camxes** runs via the ilmentufa node CLI (`fanva-verify::parser_diff::camxes_accepts`,
  the same reference parser `just verify-parser` uses) — this is the third gate that is
  otherwise browser-only.

It reports, per provider: **first-pass validity** (attempt 0 passes all three gates),
**attempts-to-converge** (mean/median), the **non-convergence** rate, transport
failures, and any 2-gate↔3-gate disagreement (`div`).

Design notes:

- **Never shipped.** The `measure` feature is off by default; `ureq`/`tokio`/node/
  `fanva-verify` are optional deps enabled only by it, so the wasm browser bundle is
  untouched (verified: `just check-ui-wasm` and `just build-ui` are unaffected).
- **jbotci is off** (`McpClient::new("")` ⇒ tool-free), matching the default translate
  path — jbotci tool-use ships off. A stub validator returns MATCH so the fail-open
  semantic verifier doesn't perturb the metric; the three hard gates are what's measured.
- Transient provider errors (429, 5xx, timeout) are retried with backoff; auth/400/404
  fail fast.

### Running it

Inside `nix develop` (needs `node` + `NIBLI_CAMXES_DIR` for the third gate), with the
full dictionary fetched so smuni scores like the deployed bundle:

```sh
just fetch-dict                 # full smuni-dictionary (else ~175 curated entries)
MEASURE_OPENROUTER_KEY=... MEASURE_GEMINI_KEY=... \
MEASURE_NVIDIA_KEY=...  MEASURE_NVIDIA_BASE_URL=https://integrate.api.nvidia.com/v1 MEASURE_NVIDIA_MODEL=meta/llama-3.1-8b-instruct \
MEASURE_SARVAM_KEY=...  MEASURE_SARVAM_MODEL=sarvam-30b \
  cargo run -p fanva --features measure --bin measure
```

Keys are read from env and never logged; a keyless per-source JSONL trace is written to
`$MEASURE_OUT` (default `/tmp/fanva-measure.jsonl`). `MEASURE_LIMIT=N` caps the corpus
for a cheap smoke run. The English corpus is `fanva/corpora/measure-english.txt`
(24 sentences spanning SVO, tense, negation, the universal `ro lo`, conjunction,
place-swap, ditransitive, property, and names).

## First measurement (2026-07-13)

Full dictionary (10,932 entries), `max_attempts=4`, jbotci off, 3-gate.

| provider | model | first-pass (before) | first-pass (after tuning) |
|---|---|---|---|
| OpenRouter | llama-3.3-70b-instruct:free | rate-limited¹ | rate-limited¹ |
| Google Gemini | gemini-3.1-flash-lite | 20/24 (83%) | **24/24 (100%)** |
| NVIDIA (NIM) | meta/llama-3.1-8b-instruct | 19/24 (79%) | 19/24 (79%)² |
| Sarvam | sarvam-30b | 24/24 (100%) | 24/24 (100%) |

¹ OpenRouter's free 70b is hard-capped at **8 requests/minute** and returned no usable
rows; measure it with account credits (a paid slug) or per-minute pacing.
² NVIDIA's *targeted grammar* cases were all fixed (below); its net is bounded by the
8b model's vocabulary quality (it invents words like `bukys`/`skolu`) and high sampling
variance — the lever there is grounding the prompt in the dictionary (a separate TODO),
not more few-shots.

Anthropic and OpenAI (2 of the nominal 5 providers) were not measured — no keys.

## The tuning pass

The failures clustered into three grammar gaps. Each addition was verified through all
three gates **in both dictionary modes** (full and the curated ~175-entry fallback, so
the `shipped_examples_are_gate_valid` guard test stays green in CI):

1. **Ditransitive place structure.** Models rendered "gives X to Y" with a stray
   preposition (`dunda … tu'a lo tadni` → unconsumed tokens). Added: a grammar note that
   `dunda` is `x1 gives x2 to x3` filled in order with no preposition for the recipient,
   plus the few-shot `"I give the book to Adam" → "mi dunda lo cukta la .adam."`.
2. **Vowel-final names.** "Mary" → `la .maria.` fails: a cmevla must end in a **consonant**.
   Added the rule with the verified form `"Mary" → "la .meris."`. (Note `la .sofias.`
   passes gerna+smuni but camxes rejects it — exactly why the metric is 3-gate.)
3. **Third-person pronouns.** "She" → `la .e.` (garbage). Added the rule that an
   antecedent-less he/she/it/they is `ko'a`, plus `"She sees the dog" → "ko'a viska lo gerku"`.

Evidence the additions worked (attempt-0 outputs, after tuning):

| case | before (fails) | after (first-pass) |
|---|---|---|
| ditransitive | `dunda … tu'a lo tadni` / wrong selbri | `… dunda lo cukta la .studentu.` |
| Mary (name) | `la .maria.` (all 4 attempts) | `la .meris. cu klama lo zarci` |
| she (pronoun) | `la .e.` (all 4 attempts) | `ko'a … lo ckule` |

Both a strong model (Gemini → 100%) and a weak one (NVIDIA 8b) adopted the taught forms;
no provider regressed.

## Caveats / next

- Metric is the default (tool-free) translate path; it does not exercise jbotci tool-use.
- OpenRouter needs credits or pacing; Anthropic + OpenAI need keys.
- For weak models, vocabulary — not grammar — is the ceiling; see the "ground the system
  prompt in the grammar + dictionary" TODO.
