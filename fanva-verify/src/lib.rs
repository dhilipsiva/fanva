//! fanva-verify — gerna's conformance suite, carved from nibli's `nibli-verify`
//! (the Vampire/clingo verdict oracles and the Klaro gates stayed behind).
//!
//! Two differential gates run as integration tests:
//!
//! - `tests/parser_differential.rs` — the gerna ↔ camxes parse-differential:
//!   every sentence gerna accepts must parse under the official Lojban grammar
//!   (ilmentufa camxes, driven as a node subprocess over `NIBLI_CAMXES_DIR`;
//!   skips cleanly when node or the checkout is absent). See [`parser_diff`].
//! - `tests/predilex_differential.rs` — the Predilex dictionary-arity
//!   differential over `smuni-dictionary`. See [`predilex`].

pub mod corpora;
pub mod generator;
pub mod parser_diff;
pub mod predilex;
