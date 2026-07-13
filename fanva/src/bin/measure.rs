//! `measure` — LLM→Lojban convergence-measurement harness (dev-only, `--features measure`).
//!
//! Drives the REAL agentic loop (`fanva::agent::translate_agentic` with the shipped
//! `LOJBAN_SYSTEM_PROMPT`) against a live provider over a small English corpus, then
//! independently scores every attempt with the FULL three-gate metric — gerna + smuni
//! in-process, camxes via the ilmentufa node CLI (`fanva-verify::parser_diff`). Reports
//! first-pass three-gate validity and attempts-to-converge per provider.
//!
//! NOT shipped: gated behind `--features measure`, so `ureq`/node/`fanva-verify` never
//! enter the wasm browser bundle. Run inside `nix develop` (needs node +
//! `NIBLI_CAMXES_DIR` for the third gate):
//!
//! ```sh
//! MEASURE_OPENROUTER_KEY=... MEASURE_GEMINI_KEY=... \
//!   cargo run -p fanva --features measure --bin measure
//! ```
//!
//! Per-provider env: `MEASURE_<P>_KEY` (required to include the provider),
//! `MEASURE_<P>_MODEL`, and for the Custom endpoints `MEASURE_<P>_BASE_URL`
//! (`P` ∈ OPENROUTER, GEMINI, NVIDIA, SARVAM). Keys are read from env and never logged.
//! A keyless per-source JSONL trace is written to `$MEASURE_OUT` (default
//! `/tmp/fanva-measure.jsonl`) for the tuning analysis.

use std::time::Duration;

use fanva::agent::{Attempt, Outcome, translate_agentic};
use fanva::gates;
use fanva::llm::{
    ChatError, ChatResponse, LlmConfig, Provider, ToolChat, ToolDecl, Turn,
    build_chat_request_tools, parse_chat_response,
};
use fanva::mcp::McpClient;
use fanva_verify::parser_diff::{CamxesConfig, available, camxes_accepts};
use serde_json::{Value, json};

const CORPUS: &str = include_str!("../../corpora/measure-english.txt");
const MAX_ATTEMPTS: u32 = 4;

// ── native transport (dev-only) ───────────────────────────────────────────────

/// A native `ToolChat` that actually sends to the provider (the shipped `HttpChat`
/// is wasm-only). Reuses the pure `build_chat_request_tools` / `parse_chat_response`;
/// only the send is new. Blocking `ureq` inside the async method is fine — the
/// harness runs one request at a time under `block_on`, no concurrency.
struct NativeHttpChat {
    agent: ureq::Agent,
}

impl NativeHttpChat {
    fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(120))
                .build(),
        }
    }
}

impl ToolChat for NativeHttpChat {
    async fn chat_tools(
        &self,
        cfg: &LlmConfig,
        system: &str,
        turns: &[Turn],
        tools: &[ToolDecl],
    ) -> Result<ChatResponse, ChatError> {
        let provider = cfg.provider;
        let (url, headers, body) = build_chat_request_tools(cfg, system, turns, tools);
        let body_str = body.to_string();
        // Retry transient failures (429 rate-limit, 5xx, network timeout) with a
        // short backoff — a measurement metric shouldn't be tanked by a flaky free
        // tier or a cold model. Non-transient errors (auth, 400/404) fail fast.
        let mut last = ChatError("no request attempted".into());
        for i in 0..3u32 {
            if i > 0 {
                std::thread::sleep(Duration::from_secs(4 * i as u64));
            }
            let mut req = self
                .agent
                .post(&url)
                .set("content-type", "application/json");
            for (name, value) in &headers {
                req = req.set(name, value);
            }
            match req.send_string(&body_str) {
                Ok(resp) => {
                    let text = resp
                        .into_string()
                        .map_err(|e| ChatError(format!("read body: {e}")))?;
                    let json: Value = serde_json::from_str(&text).map_err(|_| {
                        ChatError(format!("{} sent unparseable JSON", provider.display_name()))
                    })?;
                    return Ok(parse_chat_response(provider, &json));
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let body = resp.into_string().unwrap_or_default();
                    let msg = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                        .unwrap_or_else(|| body.chars().take(200).collect());
                    last = ChatError(format!("HTTP {code}: {msg}"));
                    if code == 429 || (500..600).contains(&code) {
                        continue; // transient — back off and retry
                    }
                    return Err(last); // auth / bad-request / not-found: fail fast
                }
                Err(e) => {
                    // Transport/timeout — retryable.
                    last = ChatError(format!("request failed: {e}"));
                }
            }
        }
        Err(last)
    }
}

/// A stub validator: the fresh-context semantic verifier is fail-open and advisory,
/// so returning MATCH keeps the metric focused on the three hard gates.
struct StubValidator;

impl ToolChat for StubValidator {
    async fn chat_tools(
        &self,
        _cfg: &LlmConfig,
        _system: &str,
        _turns: &[Turn],
        _tools: &[ToolDecl],
    ) -> Result<ChatResponse, ChatError> {
        Ok(ChatResponse {
            text: Some("MATCH".to_string()),
            tool_calls: vec![],
        })
    }
}

// ── the third gate (camxes via node) ──────────────────────────────────────────

/// True if EVERY non-empty, non-comment line passes the official camxes grammar.
fn camxes_ok(cfg: &CamxesConfig, candidate: &str) -> bool {
    candidate
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .all(|line| matches!(camxes_accepts(cfg, line), Ok(true)))
}

/// The full three-gate verdict on a candidate: gerna + smuni (in-process) ∧ camxes.
/// `validate_kb` on native = gerna + smuni (camxes is the wasm-only in-app gate),
/// so we add camxes explicitly here to score the real success condition.
fn three_gate_ok(cfg: &CamxesConfig, candidate: &str) -> bool {
    gates::validate_kb(candidate).is_ok() && camxes_ok(cfg, candidate)
}

// ── provider config from env ──────────────────────────────────────────────────

struct ProviderRun {
    label: &'static str,
    cfg: LlmConfig,
}

fn env_model(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn provider_runs() -> Vec<ProviderRun> {
    let mut v = Vec::new();

    if let Ok(key) = std::env::var("MEASURE_OPENROUTER_KEY") {
        let mut cfg = LlmConfig::new(Provider::OpenRouter);
        cfg.api_key = key;
        cfg.model = env_model(
            "MEASURE_OPENROUTER_MODEL",
            Provider::OpenRouter.default_model(),
        );
        v.push(ProviderRun {
            label: "OpenRouter",
            cfg,
        });
    }
    if let Ok(key) = std::env::var("MEASURE_GEMINI_KEY") {
        let mut cfg = LlmConfig::new(Provider::Gemini);
        cfg.api_key = key;
        cfg.model = env_model("MEASURE_GEMINI_MODEL", Provider::Gemini.default_model());
        v.push(ProviderRun {
            label: "Gemini",
            cfg,
        });
    }
    if let Ok(key) = std::env::var("MEASURE_NVIDIA_KEY") {
        let mut cfg = LlmConfig::new(Provider::Custom);
        cfg.api_key = key;
        cfg.base_url = env_model(
            "MEASURE_NVIDIA_BASE_URL",
            "https://integrate.api.nvidia.com/v1",
        );
        cfg.model = env_model("MEASURE_NVIDIA_MODEL", "meta/llama-3.3-70b-instruct");
        v.push(ProviderRun {
            label: "NVIDIA",
            cfg,
        });
    }
    if let Ok(key) = std::env::var("MEASURE_SARVAM_KEY") {
        let mut cfg = LlmConfig::new(Provider::Custom);
        cfg.api_key = key;
        cfg.base_url = env_model("MEASURE_SARVAM_BASE_URL", "https://api.sarvam.ai/v1");
        cfg.model = env_model("MEASURE_SARVAM_MODEL", "sarvam-m");
        v.push(ProviderRun {
            label: "Sarvam",
            cfg,
        });
    }
    v
}

// ── stats ─────────────────────────────────────────────────────────────────────

fn median(sorted: &[u32]) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) as f64 / 2.0
    } else {
        sorted[mid] as f64
    }
}

fn outcome_attempts(o: &Outcome) -> &[Attempt] {
    match o {
        Outcome::Success { attempts, .. }
        | Outcome::Exhausted { attempts, .. }
        | Outcome::ChatFailed { attempts, .. } => attempts,
    }
}

fn outcome_kind(o: &Outcome) -> &'static str {
    match o {
        Outcome::Success { .. } => "success",
        Outcome::Exhausted { .. } => "exhausted",
        Outcome::ChatFailed { .. } => "chat_failed",
    }
}

struct ProviderStats {
    label: &'static str,
    model: String,
    n: u32,
    first_pass: u32,
    converged: u32,
    chat_failed: u32,
    divergence: u32,
    mean_attempts: f64,
    median_attempts: f64,
}

fn main() {
    let camxes = CamxesConfig::default();
    let camxes_up = available(&camxes);
    if !camxes_up {
        eprintln!(
            "WARNING: camxes unavailable (need node + NIBLI_CAMXES_DIR — run inside `nix develop`). \
             The third gate will reject every line, so 3-gate numbers will read 0%."
        );
    }

    let mut sources: Vec<&str> = CORPUS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    // `MEASURE_LIMIT=N` caps the corpus — for a cheap smoke run before the full pass.
    if let Some(k) = std::env::var("MEASURE_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        sources.truncate(k);
    }

    let runs = provider_runs();
    if runs.is_empty() {
        eprintln!("No MEASURE_*_KEY env vars set — nothing to measure. See the module docs.");
        std::process::exit(2);
    }

    let out_path =
        std::env::var("MEASURE_OUT").unwrap_or_else(|_| "/tmp/fanva-measure.jsonl".into());
    let mut trace = String::new();

    let chat = NativeHttpChat::new();
    let mcp = McpClient::new(""); // empty proxy ⇒ jbotci off ⇒ tool-free path

    eprintln!(
        "measuring {} sources × {} providers (3-gate, camxes {}, max_attempts={})",
        sources.len(),
        runs.len(),
        if camxes_up { "up" } else { "DOWN" },
        MAX_ATTEMPTS,
    );

    let mut stats = Vec::new();
    for run in &runs {
        eprintln!("== {} ({}) ==", run.label, run.cfg.model);
        let n = sources.len() as u32;
        let mut first_pass = 0u32;
        let mut converged = 0u32;
        let mut chat_failed = 0u32;
        let mut divergence = 0u32;
        let mut conv_attempts: Vec<u32> = Vec::new();

        for &src in &sources {
            let outcome = futures::executor::block_on(translate_agentic(
                &chat,
                &StubValidator,
                &mcp,
                &run.cfg,
                src,
                MAX_ATTEMPTS,
                0,
            ));
            let attempts = outcome_attempts(&outcome);

            if let Outcome::ChatFailed { error, .. } = &outcome
                && attempts.is_empty()
            {
                chat_failed += 1;
                eprintln!("  chat-failed: {src} — {error}");
                trace.push_str(&format!(
                    "{}\n",
                    json!({
                        "provider": run.label, "model": run.cfg.model, "source": src,
                        "outcome": "chat_failed", "error": error, "attempts": []
                    })
                ));
                continue;
            }

            // First all-3-gate-passing attempt (1-indexed via Attempt::n).
            let mut conv_at: Option<u32> = None;
            let mut attempt_records = Vec::new();
            for a in attempts {
                let ok3 = three_gate_ok(&camxes, &a.candidate);
                if ok3 && conv_at.is_none() {
                    conv_at = Some(a.n);
                }
                attempt_records.push(json!({
                    "n": a.n,
                    "candidate": a.candidate,
                    "loop_gate_error": a.error.as_ref().map(|e| format!("{}: {}", e.gate(), e.message())),
                    "three_gate_ok": ok3,
                }));
            }

            let first_ok3 = attempts
                .first()
                .map(|a| three_gate_ok(&camxes, &a.candidate))
                .unwrap_or(false);
            // Loop's native 2-gate verdict on attempt 0 (error.is_none()) vs our 3-gate.
            let loop_2gate_first = attempts.first().map(|a| a.error.is_none()).unwrap_or(false);
            if loop_2gate_first != first_ok3 {
                divergence += 1;
            }
            if first_ok3 {
                first_pass += 1;
            }
            if let Some(k) = conv_at {
                converged += 1;
                conv_attempts.push(k);
            }
            if matches!(outcome, Outcome::ChatFailed { .. }) {
                chat_failed += 1;
            }

            trace.push_str(&format!(
                "{}\n",
                json!({
                    "provider": run.label, "model": run.cfg.model, "source": src,
                    "outcome": outcome_kind(&outcome),
                    "first_pass_3gate": first_ok3, "converged_at": conv_at,
                    "attempts": attempt_records,
                })
            ));
        }

        conv_attempts.sort_unstable();
        let mean = if converged > 0 {
            conv_attempts.iter().sum::<u32>() as f64 / converged as f64
        } else {
            f64::NAN
        };
        stats.push(ProviderStats {
            label: run.label,
            model: run.cfg.model.clone(),
            n,
            first_pass,
            converged,
            chat_failed,
            divergence,
            mean_attempts: mean,
            median_attempts: median(&conv_attempts),
        });
    }

    if let Err(e) = std::fs::write(&out_path, &trace) {
        eprintln!("(could not write trace to {out_path}: {e})");
    } else {
        eprintln!("per-source trace written to {out_path}");
    }

    // ── report ──
    println!();
    println!(
        "# fanva convergence — {} sources, 3-gate (gerna∧smuni∧camxes), max_attempts={}, jbotci off",
        sources.len(),
        MAX_ATTEMPTS
    );
    if !camxes_up {
        println!("# NOTE: camxes was DOWN — 3-gate columns are not meaningful this run.");
    }
    println!();
    println!(
        "{:<12} {:<34} {:>7} {:>9} {:>9} {:>6} {:>6} {:>6}",
        "provider", "model", "1st-3g", "conv", "meanA", "medA", "fail", "div"
    );
    println!("{}", "-".repeat(96));
    for s in &stats {
        let model = if s.model.len() > 33 {
            format!("{}…", &s.model[..32])
        } else {
            s.model.clone()
        };
        let fp = format!(
            "{}/{} {:.0}%",
            s.first_pass,
            s.n,
            100.0 * s.first_pass as f64 / s.n as f64
        );
        let cv = format!(
            "{}/{} {:.0}%",
            s.converged,
            s.n,
            100.0 * s.converged as f64 / s.n as f64
        );
        let mean = if s.mean_attempts.is_nan() {
            "-".into()
        } else {
            format!("{:.2}", s.mean_attempts)
        };
        let med = if s.median_attempts.is_nan() {
            "-".into()
        } else {
            format!("{:.1}", s.median_attempts)
        };
        println!(
            "{:<12} {:<34} {:>7} {:>9} {:>9} {:>6} {:>6} {:>6}",
            s.label, model, fp, cv, mean, med, s.chat_failed, s.divergence
        );
    }
    println!();
    println!(
        "1st-3g = attempt-0 passed all three gates; conv = converged within {MAX_ATTEMPTS} attempts;"
    );
    println!(
        "meanA/medA = attempts-to-converge among converged; fail = transport failures; div = 2-gate↔3-gate disagreements."
    );
}
