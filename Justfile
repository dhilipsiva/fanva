# fanva — agentic English→Lojban translator.
# Run inside the Nix dev shell (`nix develop` or direnv) so dx/wrangler/node/
# wasm-pack/cargo-fuzz and the NIBLI_CAMXES_DIR / NIBLI_NIGHTLY_BIN env are present.

# List recipes.
default:
    @just --list

# ── Build & test ─────────────────────────────────────────────────────────────

# Native tests for the whole workspace (gerna, smuni, dictionaries, render,
# engine, verify units, UI bins).
test:
    cargo test --workspace -- --nocapture

# The translator engine's native tests (agent loop + history trim, local gates,
# LLM request/response shapes, MCP wire/types, tool loop, prompt guard).
test-fanva:
    cargo test -p fanva --lib -- --nocapture

# gerna parser suite (grammar/pipeline/flattener/integration test modules).
test-gerna:
    cargo test -p gerna --lib -- --nocapture

# fanva camxes-gate marshalling under node (wasm-bindgen-test): verifies
# read_camxes_result + the official_gate degrade path. The real camxes engine
# needs a browser DOM and is checked manually via `just ui`. Skips cleanly when
# wasm-pack is absent.
test-fanva-wasm:
    @if ! command -v wasm-pack >/dev/null 2>&1; then \
        echo 'test-fanva-wasm SKIPPED: wasm-pack unavailable (nix develop provides it)'; \
    else \
        wasm-pack test --node fanva; \
    fi

# The UI must keep compiling for the browser target without a full dx build.
check-ui-wasm:
    cargo check -p fanva-ui --target wasm32-unknown-unknown

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Build the batch validator the python flywheel subprocesses.
build-validate:
    cargo build -p fanva --bin fanva-validate

# ── Conformance gates ────────────────────────────────────────────────────────

# gerna <-> camxes parse-differential (the FRONT-END gate): every sentence gerna
# accepts must parse under the official Lojban grammar (ilmentufa camxes, driven
# via node over the shipped corpora + seeded random batches). One-directional:
# gerna implements a fragment, so gerna-rejects carry no signal. The Nix dev
# shell provides node + the pinned ilmentufa checkout (NIBLI_CAMXES_DIR); skips
# cleanly when either is absent.
verify-parser:
    cargo test -p fanva-verify --test parser_differential -- --nocapture --test-threads=1

# Predilex dictionary-arity differential over smuni-dictionary: an independent
# CC0 arity oracle; flags UNDERCOUNTS (dictionary truncating places Predilex
# proves exist). Runs in full or fallback dictionary mode automatically.
verify-dict:
    cargo test -p fanva-verify --test predilex_differential -- --nocapture --test-threads=1

# ── Data ─────────────────────────────────────────────────────────────────────

# Fetch the lensisku English bulk export of jbovlaste (CC-BY-SA) that
# smuni-dictionary/build.rs consumes for the full-vocabulary build; without it
# builds fall back to the curated in-tree entries (CI mode). NOTE: the endpoint
# 401s HEAD requests — plain GET works.
fetch-dict:
    curl -fsSL "https://lensisku.lojban.org/api/export/cached/en/json" \
      -o dictionary-en.json
    @echo "Wrote dictionary-en.json ($(wc -c < dictionary-en.json) bytes)"

# ── Fuzzing (gerna parser) ───────────────────────────────────────────────────

# Seed fuzz/corpus/fuzz_parse from the shipped .lojban corpora (every
# non-comment, non-':'-prefixed line becomes one seed).
fuzz-seed:
    #!/usr/bin/env python3
    import pathlib
    out = pathlib.Path("fuzz/corpus/fuzz_parse")
    out.mkdir(parents=True, exist_ok=True)
    n = 0
    for src in ["readme.lojban", "gdpr.lojban", "drug-interactions.lojban", "determinism-corpus.lojban"]:
        for line in pathlib.Path(src).read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or line.startswith(":"):
                continue
            (out / f"seed_{n:05d}").write_text(line)
            n += 1
    print(f"seeded {n} corpus entries")

# libFuzzer over gerna::parse_text_native (recovery paths included). Leak
# detection stays ON (libFuzzer default): the AST arena is leak-free by
# invariant (see gerna/src/ast.rs — no owned String/Vec in arena-moved nodes),
# and LSan is the gate that keeps it that way. Needs the pinned nightly
# (NIBLI_NIGHTLY_BIN, provided by the Nix dev shell).
fuzz-parse SECONDS="0":
    @test -n "${NIBLI_NIGHTLY_BIN:-}" || { echo "NIBLI_NIGHTLY_BIN is not set — run inside the Nix dev shell"; exit 1; }
    cd fuzz && PATH="$NIBLI_NIGHTLY_BIN:$PATH" cargo fuzz run fuzz_parse -- -max_len=4096 {{ if SECONDS != "0" { "-max_total_time=" + SECONDS } else { "" } }}

# Short seeded fuzz pass for CI.
fuzz-ci SECONDS="120": fuzz-seed (fuzz-parse SECONDS)

# ── Python Lojban flywheel ───────────────────────────────────────────────────

# Deterministic no-LLM Lojban→FOL+English classifier over readme.lojban.
classify:
    cd python && python3 lojban_classifier.py

test-classifier:
    cd python && python3 test_classifier.py

# Claude-generated English→Lojban training pairs, validated via fanva-validate
# (needs ANTHROPIC_API_KEY; writes data/training_raw.jsonl). Extra args pass
# through (e.g. --resume, --dry-run).
generate-training *ARGS: build-validate
    python3 python/generate_training_data.py {{ARGS}}

training-stats:
    python3 python/training_stats.py

export-hf:
    python3 python/training_stats.py --export-hf

# QLoRA fine-tune / eval / refine / push (heavy lazy python deps — see
# python/nibli_model.py; eval metric = gerna pass rate via fanva-validate).
model-train *ARGS: build-validate
    python3 python/nibli_model.py train {{ARGS}}

model-eval *ARGS: build-validate
    python3 python/nibli_model.py eval {{ARGS}}

model-refine *ARGS: build-validate
    python3 python/nibli_model.py refine {{ARGS}}

model-push *ARGS:
    python3 python/nibli_model.py push {{ARGS}}

# ── UI & proxy ───────────────────────────────────────────────────────────────

# Dioxus dev server for fanva-ui.
ui PORT="8080":
    cd fanva-ui && dx serve --port {{PORT}}

# Release web build (static output under target/dx/fanva-ui/release/web/public/).
build-ui:
    cd fanva-ui && dx build --release

# fanva-proxy uses plain npm/wrangler — see fanva-proxy/DEPLOY.md.

# ── CI aggregates ────────────────────────────────────────────────────────────

ci: fmt-check clippy test check-ui-wasm verify-parser verify-dict

ci-wasm: test-fanva-wasm

ci-all: ci ci-wasm
