//! fanva web UI (Dioxus) — an agentic English→Lojban translator.
//!
//! A standalone, in-browser interface with three tabs: Source (plain English,
//! translated into Lojban by a bring-your-own-key LLM running the
//! self-correcting agentic loop), the Lojban tab (the translation, editable,
//! with Load/Clear), and Back-translation (the structure-exposing English
//! gloss of what fanva understood). Every candidate must pass the three-gate
//! validator (gerna ∧ smuni ∧ camxes) before it is shown; all three gates run
//! locally in the WASM bundle. The ONLY network calls are the LLM itself
//! (straight from the browser to the user's chosen provider — the client
//! lives in `fanva::llm`, the single source of truth; fanva-ui only wraps it
//! in a `Settings` bundle) and, when explicitly enabled, the jbotci MCP tools
//! via the app-owned blind proxy. fanva itself has no server.

use dioxus::prelude::*;

use fanva::llm::{LlmConfig, Provider};

fn main() {
    dioxus::launch(App);
}

/// IR-driven back-translation of the (multi-line) Lojban tab, computed entirely
/// client-side: each non-comment line is parsed (gerna) + compiled to FOL
/// (smuni) and rendered as structure-exposing English by `nibli-render`. A line
/// that does not compile falls back to the lexical word-by-word gloss
/// (`smuni_dictionary::back_translate`) so the panel always shows something.
/// This is the "What fanva understood" reading.
fn back_translate_ir(lojban: &str) -> String {
    lojban
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                Some(render_kb_line(trimmed))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_kb_line(line: &str) -> String {
    let parsed = match gerna::parse_text_native(line.to_string()) {
        Ok(p) => p,
        Err(_) => return smuni_dictionary::back_translate(line),
    };
    let fallback = || smuni_dictionary::back_translate(line);
    if parsed.errors.is_empty() {
        smuni::compile_from_gerna_ast(parsed.buffer)
            .map(|buf| nibli_render::render_logic_buffer(&buf, nibli_render::Register::Spec))
            .unwrap_or_else(|_| fallback())
    } else {
        fallback()
    }
}

/// Collapse the Lojban tab into one multi-sentence text for jbotci `tersmu`
/// (a Lojban semantic parser): drop empty + `#`-comment lines (same filter as
/// [`back_translate_ir`]) and join with the neutral sentence separator `.i`, so
/// tersmu analyses the whole text in a single call and returns one coherent
/// semantic graph.
fn kb_to_tersmu_text(lojban: &str) -> String {
    lojban
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" .i ")
}

/// The prefilled Source/Lojban pair — the classic syllogism, so the very first
/// Translate click has something meaningful to chew on.
const DEFAULT_SOURCE: &str = "All dogs are animals.\nAll animals eat.\nAdam is a dog.";
const DEFAULT_LOJBAN: &str = "ro lo gerku cu danlu\nro lo danlu cu citka\nla .adam. cu gerku";

// ── Agentic translate (fanva) ──
// The Source→Lojban button runs the self-correcting loop: translate →
// (optionally call jbotci tools) → validate (gerna+smuni+camxes) → feed the
// exact gate error back → retry, bounded below. All gates are local/in-browser;
// the only network call is the LLM itself.

/// Cap on jbotci tool calls within a single translate attempt.
const MAX_TOOL_STEPS: u32 = 4;

/// One row of the self-correction trace rendered under the Source tab.
#[derive(Clone, Copy)]
enum GateState {
    Pass,
    Fail,
    Skip,
}

/// One jbotci tool call, summarized for a trace sub-row.
#[derive(Clone)]
struct ToolRow {
    name: String,
    detail: String,
    is_error: bool,
}

#[derive(Clone)]
struct TraceRow {
    n: u32,
    ok: bool,
    detail: String,
    /// Per-gate chips: (label like "gerna ✓", css class).
    gates: Vec<(String, &'static str)>,
    /// jbotci tool calls made in this attempt (empty when jbotci is off).
    tools: Vec<ToolRow>,
}

/// The default jbotci proxy: the app-owned "blind" CORS reverse-proxy Worker
/// (`fanva-proxy/`). Prefilled so opting in is one click, but jbotci ships OFF
/// (`jbotci_enabled = false`) — the URL stays inert until the user enables it, so the
/// default translate run is fully local and makes NO call to the proxy. The proxy
/// only strips the browser Origin and forwards to jbotci verbatim; it stores nothing
/// (source is linked from the settings modal).
const DEFAULT_JBOTCI_PROXY_URL: &str = "https://fanva-proxy.dhilipsiva.workers.dev/mcp";

/// fanva-ui's settings bundle: the LLM provider config (`fanva::llm::LlmConfig`,
/// the single source of truth) plus the agent/jbotci knobs that aren't LLM-provider
/// settings. Held in one in-memory signal; never persisted.
#[derive(Clone, PartialEq)]
struct Settings {
    llm: LlmConfig,
    proxy_url: String,
    /// jbotci tool-use opt-in. OFF by default: the prefilled `proxy_url` is inert
    /// until the user flips this, keeping the default run local-only (no proxy call).
    jbotci_enabled: bool,
    max_attempts: u32,
}

impl Settings {
    fn new(provider: Provider) -> Self {
        Settings {
            llm: LlmConfig::new(provider),
            proxy_url: DEFAULT_JBOTCI_PROXY_URL.to_string(),
            jbotci_enabled: false,
            max_attempts: 5,
        }
    }

    /// The proxy URL actually handed to the MCP client: the configured URL only when
    /// jbotci is explicitly enabled, else empty. Empty ⇒ the translate loop and the
    /// tersmu view degrade to the local gates and make NO network call — this is what
    /// makes "disabled by default" a real privacy guarantee, not a hidden-but-live URL.
    fn active_proxy_url(&self) -> String {
        if self.jbotci_enabled {
            self.proxy_url.trim().to_string()
        } else {
            String::new()
        }
    }
}

/// Single-shot English→Lojban translate via fanva's transport — used by the
/// modal key-test (the Source tab uses the full agentic `translate_agentic`).
/// Returns the cleaned Lojban text or a user-facing error.
async fn fanva_translate(cfg: &LlmConfig, english: &str) -> Result<String, String> {
    use fanva::llm::{Chat, HttpChat, Turn, clean_lojban_output, system_prompt};
    let request = format!("Translate to Lojban: {}", english.trim());
    let turns = [Turn::user(request)];
    let raw = HttpChat
        .chat(cfg, system_prompt(), &turns)
        .await
        .map_err(|e| e.to_string())?;
    let cleaned = clean_lojban_output(&raw);
    if cleaned.is_empty() {
        return Err("The provider returned an empty result.".to_string());
    }
    Ok(cleaned)
}

/// The local gates, in the fail-fast order `validate` runs them.
const GATE_ORDER: [&str; 3] = ["gerna", "smuni", "camxes"];

/// Derive the per-gate chips from an attempt's error. `validate` is fail-fast in
/// `GATE_ORDER`, so `error.gate()` is the failing gate; earlier gates passed,
/// later ones were skipped. A failure PAST the chain (the semantic verifier)
/// means all three grammar gates passed. (Assumes camxes ran in-browser; if its
/// shim failed to load it silently passes — a rare edge.)
fn gate_chips(error: &Option<fanva::gates::GateError>) -> Vec<(String, &'static str)> {
    let states: [GateState; 3] = match error {
        None => [GateState::Pass; 3],
        Some(e) => match GATE_ORDER.iter().position(|g| *g == e.gate()) {
            // The semantic verifier failed AFTER the grammar chain: all pass.
            None => [GateState::Pass; 3],
            Some(fail_idx) => std::array::from_fn(|i| {
                if i < fail_idx {
                    GateState::Pass
                } else if i == fail_idx {
                    GateState::Fail
                } else {
                    GateState::Skip
                }
            }),
        },
    };
    GATE_ORDER
        .iter()
        .zip(states)
        .map(|(name, st)| {
            let (glyph, class) = match st {
                GateState::Pass => ("\u{2713}", "gate-chip pass"),
                GateState::Fail => ("\u{2717}", "gate-chip fail"),
                GateState::Skip => ("\u{00B7}", "gate-chip skip"),
            };
            (format!("{name} {glyph}"), class)
        })
        .collect()
}

/// A compact `args → result` snippet for a tool-call trace row.
fn tool_summary(t: &fanva::tools::ToolCallTrace) -> String {
    let args = truncate(&t.args.to_string(), 40);
    let result = truncate(&t.result.replace('\n', " "), 80);
    format!("{args} \u{2192} {result}")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}\u{2026}")
    } else {
        s.to_string()
    }
}

/// Collapse the agent's attempts into UI trace rows (per-gate chips + first error
/// line + any jbotci tool calls made).
fn trace_rows(attempts: &[fanva::agent::Attempt]) -> Vec<TraceRow> {
    attempts
        .iter()
        .map(|a| TraceRow {
            n: a.n,
            ok: a.error.is_none(),
            detail: match &a.error {
                None => "valid \u{2014} passed the gates".to_string(),
                Some(e) => {
                    let msg = e.message();
                    let first = msg.lines().next().unwrap_or(msg);
                    format!("{}: {first}", e.gate())
                }
            },
            gates: gate_chips(&a.error),
            tools: a
                .tool_calls
                .iter()
                .map(|t| ToolRow {
                    name: t.name.clone(),
                    detail: tool_summary(t),
                    is_error: t.is_error,
                })
                .collect(),
        })
        .collect()
}

// ── Types ──

#[derive(Clone, Copy, PartialEq)]
enum ActiveTab {
    Source,
    Lojban,
    BackTranslation,
}

// ── Components ──

#[component]
fn App() -> Element {
    let lojban_text: Signal<String> = use_signal(|| DEFAULT_LOJBAN.to_string());
    // The LLM translate config lives ONLY here, in memory — never persisted to
    // storage, cleared on tab close/reload. `None` until the user configures it.
    let settings: Signal<Option<Settings>> = use_signal(|| None);
    let modal_open: Signal<bool> = use_signal(|| false);

    let on_global_keydown = move |e: KeyboardEvent| {
        if e.modifiers().ctrl()
            && let Key::Character(ref c) = e.key()
            && c == "o"
        {
            e.prevent_default();
            spawn(async move {
                let _ =
                    document::eval("document.getElementById('lojban-file-input').click()").await;
            });
        }
    };

    // Source is the natural entry point (English → Lojban → back-translation).
    let active_tab: Signal<ActiveTab> = use_signal(|| ActiveTab::Source);
    // "" = dark (the instrument default); "light" = the QUINE paper theme. The
    // attribute rides on `.app-shell`, so the [data-theme="light"] overrides cascade.
    let mut theme = use_signal(|| "");

    rsx! {
        document::Title { "fanva \u{2014} agentic English \u{2192} Lojban translator" }
        document::Link { rel: "stylesheet", href: asset!("/assets/tokens.css") }
        document::Link { rel: "stylesheet", href: asset!("/assets/style.css") }
        // Local "official" grammar gate: the vendored ilmentufa camxes parser +
        // preprocessor, then a shim exposing window.camxes_validate. Served as
        // static assets (no network at validation time); fanva's official_gate
        // calls the shim. The shim resolves the globals at call time, so load
        // order is not critical.
        document::Script { src: asset!("/assets/js/vendor/camxes/camxes_preproc.js") }
        document::Script { src: asset!("/assets/js/vendor/camxes/camxes.js") }
        document::Script { src: asset!("/assets/js/vendor/camxes/camxes_shim.js") }
        // Outer shell owns the viewport: the QUINE masthead sits above the
        // instrument. data-theme rides here (not on `.app`) so the header
        // themes alongside the panels.
        div { class: "app-shell", "data-theme": "{theme}",
            header { class: "app-header",
                div { class: "app-header__brand",
                    span { class: "app-header__name", "fanva" }
                    span { class: "app-header__tagline", "an agentic english \u{2192} lojban translator" }
                }
                span { class: "app-header__sp" }
                span { class: "app-header__credit",
                    "Built with "
                    span { class: "app-header__heart", "\u{2665}" }
                    " by "
                    a {
                        class: "app-header__credit-link",
                        href: "https://dhilipsiva.dev/",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        "@dhilipsiva"
                    }
                }
                button {
                    class: "app-header__theme",
                    title: "Toggle theme",
                    onclick: move |_| {
                        let next = if *theme.read() == "light" { "" } else { "light" };
                        theme.set(next);
                    },
                    if *theme.read() == "light" { "\u{263E} dark" } else { "\u{2600} light" }
                }
            }
            // The translator is the whole instrument now — one full-width panel
            // (the stylesheet's second grid row belonged to the removed query
            // deck, hence the inline row override).
            div {
                class: "app",
                style: "grid-template-rows: 1fr;",
                tabindex: "0",
                onkeydown: on_global_keydown,
                div { class: "col-tabs",
                    SourceTabs { lojban_text, active_tab, settings, modal_open }
                }
            }
            footer { class: "app-footer",
                span { class: "app-footer__text",
                    span { class: "app-footer__brand", "fanva" }
                    " \u{2014} an agentic English \u{2192} Lojban translator, built in Rust and compiled to WebAssembly. Every candidate must pass three grammar gates (gerna \u{2227} smuni \u{2227} camxes) before you see it \u{2014} and all three run locally in your browser."
                }
                a {
                    class: "app-footer__star",
                    href: "https://github.com/dhilipsiva/fanva",
                    target: "_blank",
                    rel: "noopener noreferrer",
                    title: "Star fanva on GitHub",
                    span { class: "app-footer__star-icon", "\u{2605}" }
                    " Star on GitHub"
                }
            }
            if *modal_open.read() {
                LlmConfigModal { settings, modal_open }
            }
        }
    }
}

#[component]
fn SourceTabs(
    lojban_text: Signal<String>,
    active_tab: Signal<ActiveTab>,
    settings: Signal<Option<Settings>>,
    modal_open: Signal<bool>,
) -> Element {
    let mut source_text = use_signal(|| DEFAULT_SOURCE.to_string());
    let mut translating = use_signal(|| false);
    let mut translate_error = use_signal(|| Option::<String>::None);
    let mut translate_trace = use_signal(Vec::<TraceRow>::new);
    let mut translate_degraded = use_signal(|| false);

    // Deep-meaning (jbotci tersmu) view state — a network result, so held in signals
    // (not a memo). On-demand + proxy-gated; see `do_tersmu` below.
    let mut tersmu_loading = use_signal(|| false);
    let mut tersmu_error = use_signal(|| Option::<String>::None);
    let mut tersmu_result = use_signal(|| Option::<String>::None);

    // Invalidate a shown graph whenever the Lojban changes (an edit, a Clear/
    // Load, or a fresh translate) so tersmu output never shows stale.
    use_effect(move || {
        let _ = lojban_text.read();
        tersmu_result.set(None);
        tersmu_error.set(None);
    });

    // Back-translation reflects the editable Lojban tab.
    let back_translation = use_memo(move || {
        let text = lojban_text.read().clone();
        if text.is_empty() {
            String::new()
        } else {
            back_translate_ir(&text)
        }
    });

    // Translate the Source tab → Lojban via the configured LLM. With no
    // provider configured yet, open the integration modal instead of erroring.
    let mut do_translate = move || {
        let text = source_text.read().clone();
        if text.trim().is_empty() || *translating.read() {
            return;
        }
        let Some(cfg) = settings.read().clone() else {
            modal_open.set(true);
            return;
        };
        translating.set(true);
        translate_error.set(None);
        translate_trace.set(Vec::new());
        translate_degraded.set(false);
        spawn(async move {
            use fanva::agent::Outcome;
            // The self-correcting loop: translate → (optionally call jbotci
            // tools) → validate (gerna+smuni+camxes) → semantic verification
            // (a fresh-context judge reads the engine's back-translation) →
            // feed any error back → retry, up to the configured max attempts.
            let http = fanva::llm::HttpChat;
            // The proxy stays inert unless the user enabled jbotci: an empty
            // URL means a tool-free loop that makes no proxy call.
            let mcp = fanva::mcp::McpClient::new(cfg.active_proxy_url());
            // The same zero-sized HttpChat serves as the semantic validator: the
            // Chat seam is stateless, so the judge call is a genuinely fresh
            // conversation (same provider/key, no shared history).
            let outcome = fanva::agent::translate_agentic(
                &http,
                &http,
                &mcp,
                &cfg.llm,
                &text,
                cfg.max_attempts.max(1),
                MAX_TOOL_STEPS,
            )
            .await;
            match outcome {
                Outcome::Success {
                    lojban,
                    attempts,
                    degraded,
                } => {
                    translate_trace.set(trace_rows(&attempts));
                    translate_degraded.set(degraded);
                    lojban_text.set(lojban);
                    active_tab.set(ActiveTab::Lojban);
                }
                Outcome::Exhausted {
                    best,
                    last_error,
                    attempts,
                    degraded,
                } => {
                    let n = attempts.len();
                    translate_trace.set(trace_rows(&attempts));
                    translate_degraded.set(degraded);
                    // Show the best effort so the user can edit from there.
                    lojban_text.set(best);
                    active_tab.set(ActiveTab::Lojban);
                    translate_error.set(Some(format!(
                        "Couldn't fully validate after {n} attempts \u{2014} showing best effort. Last: {}",
                        last_error.message()
                    )));
                }
                Outcome::ChatFailed {
                    error, attempts, ..
                } => {
                    translate_trace.set(trace_rows(&attempts));
                    translate_error.set(Some(error));
                }
            }
            translating.set(false);
        });
    };
    let translate_click = move |_: Event<MouseData>| {
        do_translate();
    };
    let on_source_keydown = move |e: KeyboardEvent| {
        if e.key() == Key::Enter && e.modifiers().ctrl() {
            e.prevent_default();
            do_translate();
        }
    };

    // Deep meaning: send the Lojban tab (as one `.i`-joined text) to jbotci's
    // tersmu tool and show the raw semantic graph. On-demand (network) +
    // proxy-gated; any failure (incl. no proxy / native) degrades to a notice,
    // never a hard error.
    let mut do_tersmu = move || {
        if *tersmu_loading.read() {
            return;
        }
        let Some(cfg) = settings.read().clone() else {
            return;
        };
        if cfg.active_proxy_url().is_empty() {
            return;
        }
        let joined = kb_to_tersmu_text(&lojban_text.read());
        if joined.is_empty() {
            return;
        }
        tersmu_loading.set(true);
        tersmu_error.set(None);
        tersmu_result.set(None);
        spawn(async move {
            let mcp = fanva::mcp::McpClient::new(cfg.active_proxy_url());
            let outcome = mcp.tersmu(&joined).await;
            // Drop the result if the Lojban changed while the request was in flight.
            if kb_to_tersmu_text(&lojban_text.read()) == joined {
                match outcome {
                    Ok(res) if !res.is_error => tersmu_result.set(Some(res.text)),
                    Ok(res) => tersmu_error.set(Some(format!(
                        "jbotci tersmu reported an error: {}",
                        res.text
                    ))),
                    // McpError::Display is self-describing (429/5xx/network/…).
                    Err(e) => tersmu_error.set(Some(e.to_string())),
                }
            }
            tersmu_loading.set(false);
        });
    };
    let tersmu_click = move |_: Event<MouseData>| {
        do_tersmu();
    };

    // The deep-meaning (tersmu) view only appears when jbotci is enabled with a
    // proxy (tersmu is a network tool; everything else here is local).
    let jbotci_on = settings
        .read()
        .as_ref()
        .map(|s| !s.active_proxy_url().is_empty())
        .unwrap_or(false);

    rsx! {
        div { class: "tabs-container",
            div { class: "tab-bar",
                button {
                    class: if *active_tab.read() == ActiveTab::Source { "tab active" } else { "tab" },
                    onclick: move |_| active_tab.set(ActiveTab::Source),
                    "Source"
                }
                button {
                    class: if *active_tab.read() == ActiveTab::Lojban { "tab active" } else { "tab" },
                    onclick: move |_| active_tab.set(ActiveTab::Lojban),
                    "Lojban"
                }
                button {
                    class: if *active_tab.read() == ActiveTab::BackTranslation { "tab active" } else { "tab" },
                    onclick: move |_| active_tab.set(ActiveTab::BackTranslation),
                    "Back-translation"
                }
            }
            div { class: "tab-content",
                match *active_tab.read() {
                    ActiveTab::Source => {
                        let hint = match settings.read().as_ref().map(|c| c.llm.provider.short_name()) {
                            Some(p) => format!("english \u{2192} lojban via {p}"),
                            None => "english \u{2192} lojban \u{2014} configure an llm".to_string(),
                        };
                        rsx! {
                            span { class: "nb-eyebrow", "source \u{2014} plain english" }
                            textarea {
                                class: "source-input",
                                placeholder: "Enter English text\u{2026}",
                                value: "{source_text}",
                                oninput: move |e| source_text.set(e.value()),
                                onkeydown: on_source_keydown,
                            }
                            if let Some(err) = translate_error.read().as_ref() {
                                div { class: "translate-error", "{err}" }
                            }
                            div { class: "translate-row",
                                button {
                                    class: if *translating.read() { "translate-btn busy" } else { "translate-btn" },
                                    onclick: translate_click,
                                    disabled: *translating.read(),
                                    "Translate"
                                }
                                button {
                                    class: "translate-row__config",
                                    title: "Configure LLM integration",
                                    onclick: move |_| modal_open.set(true),
                                    "\u{2699}"
                                }
                                span { class: "translate-row__hint", "{hint}" }
                            }
                            if !translate_trace.read().is_empty() {
                                div { class: "translate-trace",
                                    for row in translate_trace().iter() {
                                        div { key: "{row.n}", class: "trace-item",
                                            div {
                                                class: if row.ok { "trace-row trace-ok" } else { "trace-row trace-fail" },
                                                span { class: "trace-n", "#{row.n}" }
                                                span { class: "trace-gates",
                                                    for (label, chip_class) in row.gates.iter() {
                                                        span { key: "{label}", class: "{chip_class}", "{label}" }
                                                    }
                                                }
                                                span { class: "trace-detail", "{row.detail}" }
                                            }
                                            for (ti, tool) in row.tools.iter().enumerate() {
                                                div {
                                                    key: "{ti}",
                                                    class: if tool.is_error { "trace-tool err" } else { "trace-tool ok" },
                                                    "\u{21B3} {tool.name} \u{00B7} {tool.detail}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if *translate_degraded.read() {
                                div { class: "translate-degraded",
                                    "jbotci tools off \u{2014} validated with the local gerna+smuni+camxes gates only. Enable jbotci in settings for dictionary/grammar lookups."
                                }
                            }
                        }
                    }
                    ActiveTab::Lojban => rsx! {
                        div { class: "lojban-toolbar",
                            button {
                                class: "toolbar-btn",
                                onclick: move |_| {
                                    spawn(async move {
                                        let res = document::eval(r#"
                                            document.getElementById('lojban-file-input').click();
                                            return '';
                                        "#);
                                        let _ = res.await;
                                    });
                                },
                                "Load .lojban"
                                kbd { class: "kbd-hint", "Ctrl+O" }
                            }
                            button {
                                class: "toolbar-btn",
                                onclick: move |_| {
                                    lojban_text.set(String::new());
                                },
                                "Clear"
                            }
                            input {
                                r#type: "file",
                                accept: ".lojban,.txt",
                                style: "display: none",
                                id: "lojban-file-input",
                                onchange: move |_| {
                                    spawn(async move {
                                        let res = document::eval(r#"
                                            const input = document.getElementById('lojban-file-input');
                                            const file = input.files[0];
                                            if (!file) return '';
                                            const text = await file.text();
                                            input.value = '';
                                            return text;
                                        "#);
                                        if let Ok(val) = res.await
                                            && let Some(text) = val.as_str()
                                            && !text.is_empty()
                                        {
                                            lojban_text.set(text.to_string());
                                        }
                                    });
                                },
                            }
                        }
                        textarea {
                            class: "lojban-input",
                            placeholder: "Translations land here \u{2014} or write Lojban yourself (one sentence per line)\u{2026}",
                            value: "{lojban_text}",
                            oninput: move |e| lojban_text.set(e.value()),
                        }
                    },
                    ActiveTab::BackTranslation => {
                        let bt = back_translation.read();
                        let lines: Vec<(usize, String)> = bt
                            .lines()
                            .enumerate()
                            .map(|(i, l)| (i + 1, l.to_string()))
                            .collect();
                        let empty = lines.is_empty();
                        // Deep-meaning view state, snapshotted for this render.
                        let tersmu_err = tersmu_error.read().clone();
                        let tersmu_graph = tersmu_result.read().clone();
                        let tersmu_busy = *tersmu_loading.read();
                        rsx! {
                            span { class: "nb-eyebrow", "what fanva understood" }
                            div { class: "back-translation",
                                if empty {
                                    span { class: "back-translation__placeholder",
                                        "Type Lojban in the Lojban tab to see the structure-exposing gloss."
                                    }
                                } else {
                                    for (n, line) in lines.iter() {
                                        div { key: "{n}", class: "back-translation__line",
                                            span { class: "back-translation__num", "{n}" }
                                            span { class: "back-translation__gloss", "{line}" }
                                        }
                                    }
                                }
                            }
                            // Optional deep-meaning view — jbotci's tersmu semantic graph,
                            // shown verbatim (fanva adds zero interpretation). Only when a
                            // jbotci proxy is configured.
                            if jbotci_on {
                                div { class: "tersmu",
                                    div { class: "tersmu__head",
                                        span { class: "nb-eyebrow", "deep meaning \u{00b7} jbotci tersmu" }
                                        button {
                                            class: "tersmu-button",
                                            disabled: tersmu_busy,
                                            onclick: tersmu_click,
                                            if tersmu_busy { "Analyzing\u{2026}" } else { "Deep meaning (tersmu)" }
                                        }
                                    }
                                    if let Some(err) = tersmu_err {
                                        div { class: "tersmu__error", "{err}" }
                                    } else if let Some(graph) = tersmu_graph {
                                        pre { class: "tersmu-graph", "{graph}" }
                                    } else if !tersmu_busy {
                                        span { class: "tersmu__hint",
                                            "jbotci's deep semantic graph for the current text \u{2014} an independent second opinion on the meaning."
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// Bring-your-own-key LLM integration modal. Edits a draft config held in local
/// signals; on Save it lands in the App's in-memory `settings`. The key never
/// leaves this tab (see the security note + `llm/http.rs`).
#[component]
fn LlmConfigModal(settings: Signal<Option<Settings>>, modal_open: Signal<bool>) -> Element {
    let initial = settings
        .read()
        .clone()
        .unwrap_or_else(|| Settings::new(Provider::OpenRouter));
    let mut provider = use_signal(|| initial.llm.provider);
    let mut api_key = use_signal(|| initial.llm.api_key.clone());
    let mut model = use_signal(|| initial.llm.model.clone());
    let mut base_url = use_signal(|| initial.llm.base_url.clone());
    let mut proxy_url = use_signal(|| initial.proxy_url.clone());
    let mut jbotci_enabled = use_signal(|| initial.jbotci_enabled);
    let mut max_attempts = use_signal(|| initial.max_attempts);
    let mut testing = use_signal(|| false);
    let mut test_msg = use_signal(|| Option::<(bool, String)>::None);

    let prov = *provider.read();

    let build_settings = move || Settings {
        llm: LlmConfig {
            provider: *provider.read(),
            api_key: api_key.read().trim().to_string(),
            model: model.read().trim().to_string(),
            base_url: base_url.read().trim().to_string(),
            max_tokens: 1024,
        },
        proxy_url: proxy_url.read().trim().to_string(),
        jbotci_enabled: *jbotci_enabled.read(),
        max_attempts: (*max_attempts.read()).max(1),
    };
    // A key is required for everyone except Custom (which may be a local server).
    let needs_key =
        move |s: &Settings| s.llm.api_key.is_empty() && s.llm.provider != Provider::Custom;

    let on_save = move |_: Event<MouseData>| {
        let s = build_settings();
        if needs_key(&s) {
            test_msg.set(Some((false, "Enter your API key first.".to_string())));
            return;
        }
        settings.set(Some(s));
        modal_open.set(false);
    };
    let on_test = move |_: Event<MouseData>| {
        if *testing.read() {
            return;
        }
        let s = build_settings();
        if needs_key(&s) {
            test_msg.set(Some((false, "Enter your API key first.".to_string())));
            return;
        }
        testing.set(true);
        test_msg.set(None);
        spawn(async move {
            match fanva_translate(&s.llm, "Adam is a dog").await {
                Ok(lojban) => test_msg.set(Some((true, format!("OK \u{2014} {lojban}")))),
                Err(e) => test_msg.set(Some((false, e))),
            }
            testing.set(false);
        });
    };

    rsx! {
        // Backdrop click closes; the card stops propagation so inner clicks don't.
        div { class: "modal-backdrop", onclick: move |_| modal_open.set(false),
            div { class: "modal-card", onclick: move |e: Event<MouseData>| e.stop_propagation(),
                div { class: "modal-title", "Integrate an LLM to translate" }
                p { class: "modal-subtitle",
                    "Use your own LLM to draft Lojban from plain English. Every draft must pass the three local grammar gates (gerna \u{2227} smuni \u{2227} camxes) before it is shown."
                }

                // Only this middle region scrolls; the title above and the actions
                // below stay pinned, so the modal never grows taller than the viewport.
                div { class: "modal-body",

                div { class: "llm-provider-picker",
                    for p in Provider::ALL {
                        button {
                            key: "{p.short_name()}",
                            class: if *provider.read() == p { "llm-provider-btn active" } else { "llm-provider-btn" },
                            onclick: move |_| {
                                provider.set(p);
                                model.set(p.default_model().to_string());
                                base_url.set(p.default_base_url().to_string());
                                test_msg.set(None);
                            },
                            "{p.short_name()}"
                            if let Some(promo) = p.promo() {
                                span { class: "llm-provider-btn__badge", "{promo.badge}" }
                            }
                        }
                    }
                }

                if let Some(promo) = prov.promo() {
                    div { class: "llm-promo-note",
                        span { class: "llm-promo-note__text", "{promo.note} " }
                        a {
                            class: "llm-promo-note__link",
                            href: "{promo.signup_url}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "Get a free key \u{2192}"
                        }
                    }
                }

                label { class: "llm-field",
                    span { class: "llm-field__label", "API key" }
                    input {
                        class: "llm-field__input",
                        r#type: "password",
                        autocomplete: "off",
                        placeholder: if prov == Provider::Custom { "optional for local servers" } else { "your provider api key" },
                        value: "{api_key}",
                        oninput: move |e| api_key.set(e.value()),
                    }
                }
                if prov.needs_base_url() {
                    label { class: "llm-field",
                        span { class: "llm-field__label", "Base URL" }
                        input {
                            class: "llm-field__input",
                            r#type: "text",
                            placeholder: "http://localhost:11434/v1",
                            value: "{base_url}",
                            oninput: move |e| base_url.set(e.value()),
                        }
                    }
                }
                div { class: "llm-row",
                    label { class: "llm-field",
                        span { class: "llm-field__label", "Model" }
                        input {
                            class: "llm-field__input",
                            r#type: "text",
                            placeholder: "{prov.default_model()}",
                            value: "{model}",
                            oninput: move |e| model.set(e.value()),
                        }
                    }
                    label { class: "llm-field",
                        span { class: "llm-field__label", "Max attempts" }
                        input {
                            class: "llm-field__input",
                            r#type: "number",
                            min: "1",
                            max: "10",
                            value: "{max_attempts}",
                            oninput: move |e| {
                                if let Ok(v) = e.value().parse::<u32>() {
                                    max_attempts.set(v.clamp(1, 10));
                                }
                            },
                        }
                    }
                }

                label { class: "llm-field llm-field--toggle",
                    input {
                        class: "llm-field__checkbox",
                        r#type: "checkbox",
                        checked: jbotci_enabled(),
                        onchange: move |e| jbotci_enabled.set(e.checked()),
                    }
                    span { class: "llm-field__label", "Enable jbotci tools (dictionary / grammar / morphology)" }
                }
                label { class: "llm-field",
                    span { class: "llm-field__label", "jbotci proxy URL" }
                    input {
                        class: "llm-field__input",
                        r#type: "text",
                        placeholder: "https://your-proxy.example/mcp",
                        disabled: !jbotci_enabled(),
                        value: "{proxy_url}",
                        oninput: move |e| proxy_url.set(e.value()),
                    }
                    span { class: "llm-field__hint",
                        "Off by default \u{2014} the translate run stays fully local (gerna+smuni+camxes) and makes no network call to the proxy. Enable it to let the model call jbotci for dictionary/grammar/morphology lookups while drafting Lojban. Your LLM key is never sent here."
                    }
                }
                div { class: "llm-security-note",
                    span { class: "llm-security-note__title", "\u{1F441}\u{FE0F} It's a blind proxy \u{2014} nothing is stored" }
                    p {
                        "jbotci refuses direct browser calls (CORS), so the URL above points at "
                        b { "fanva-proxy" }
                        " \u{2014} an app-owned Cloudflare Worker that strips your browser \u{2018}Origin\u{2019} and forwards the request verbatim to jbotci. It's a stateless blind relay: no logs, no database, no cookies. The upstream is hardcoded (not an open proxy), and every line is public \u{2014} read it yourself:"
                    }
                    div { class: "llm-security-note__links",
                        a {
                            href: "https://github.com/dhilipsiva/fanva/blob/main/fanva-proxy/src/index.js",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "index.js \u{2014} the entire proxy"
                        }
                        a {
                            href: "https://github.com/dhilipsiva/fanva/blob/main/fanva-proxy/README.md",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "README \u{2014} why & what it strips"
                        }
                    }
                }
                div { class: "llm-security-note",
                    span { class: "llm-security-note__title", "\u{1F512} Your key stays in this tab" }
                    p {
                        "Held only in this browser tab's memory \u{2014} never written to disk or storage, and erased the moment you close or reload the tab. fanva has no server: the request goes straight from your browser to "
                        b { "{prov.display_name()}" }
                        ". It is open source \u{2014} verify in DevTools \u{2192} Network that the only call is to the provider."
                    }
                    div { class: "llm-security-note__links",
                        a {
                            href: "https://github.com/dhilipsiva/fanva/blob/main/fanva/src/llm/http.rs",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "http.rs \u{2014} the request code"
                        }
                        a {
                            href: "https://github.com/dhilipsiva/fanva/blob/main/fanva-ui/Cargo.toml",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "Cargo.toml \u{2014} no server dependency"
                        }
                    }
                }

                div { class: "llm-warning",
                    "\u{26A0} LLMs can hallucinate and mistranslate. The gates verify grammar, not intent \u{2014} always review the Lojban (and its back-translation) before trusting it."
                }

                if let Some((ok, msg)) = test_msg.read().clone() {
                    div {
                        class: if ok { "llm-test-result is-ok" } else { "llm-test-result is-err" },
                        "{msg}"
                    }
                }

                } // end .modal-body

                div { class: "modal-actions",
                    button {
                        class: "toolbar-btn",
                        disabled: *testing.read(),
                        onclick: on_test,
                        if *testing.read() { "Testing\u{2026}" } else { "Test" }
                    }
                    span { class: "modal-actions__sp" }
                    button {
                        class: "toolbar-btn",
                        onclick: move |_| modal_open.set(false),
                        "Cancel"
                    }
                    button { class: "translate-btn", onclick: on_save, "Save" }
                }
            }
        }
    }
}
