//! # fanva — agentic English→Lojban translator engine
//!
//! An LLM translates English into Lojban; the output is validated by real
//! compilers; any error is fed back into the conversation and the LLM retries
//! until the Lojban is valid (bounded by an attempt cap). This crate is the
//! engine — the UI shell lives in `fanva-ui`.
//!
//! Extracted from nibli's Transparency Triad formalizer (`nibli-fanva`) at the
//! rev recorded in the repo-root `NIBLI_REV`; the Klaro arm stayed behind, so
//! every entry point here is Lojban-only.
//!
//! ## Validation gate (the "verify" firewall)
//!
//! A candidate must pass three deterministic gates before the loop accepts it:
//!
//! 1. **gerna** — `gerna::parse_checked` (grammar, the narrowest gate) — local.
//! 2. **smuni** — `smuni::compile_from_gerna_ast` (semantics/arity) — local.
//! 3. **official** — a vendored standard `camxes.js` (ilmentufa, MIT) via
//!    JS-interop (the official grammar) — local, wasm-only.
//!
//! All gates are local, so the hot validation path makes no network call.
//! jbotci (`vlacku`/`cukta`/`tersmu`/`gentufa`) is optional tooling, used only
//! as LLM tools + the deep-meaning view, reached through an app-owned proxy;
//! when no proxy is configured the loop degrades to the local gates and stays
//! fully serverless.
//!
//! A gate-clean candidate then faces the fresh-context semantic verification
//! turn ([`verify`]) — an advisory LLM judge reading the engine's own
//! back-translation; a mismatch retries through the same feedback loop.
//!
//! ## Testability
//!
//! The local gates run on pure Rust crates, so [`gates`] is
//! native-`cargo test`-able. The LLM `chat()` and the MCP client are
//! abstracted behind seams so the agent/provider logic tests with mocks on
//! native, and only the concrete transports are wasm-only.

pub mod agent;
pub mod gates;
pub mod llm;
pub mod mcp;
pub mod tools;
pub mod verify;
