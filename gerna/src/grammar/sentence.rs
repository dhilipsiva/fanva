//! Sentence-level parsing: simple bridi, forethought connectives, tense, attitudinals.

use super::*;

#[allow(dead_code)]
impl<'a, 'arena> Parser<'a, 'arena> {
    // ─── Tense & Attitudinal ──────────────────────────────────

    /// Try to consume a tense marker (pu/ca/ba).
    pub(crate) fn try_parse_tense(&mut self) -> Option<Tense> {
        let t = match self.peek_cmavo()? {
            "pu" => Tense::Pu,
            "ca" => Tense::Ca,
            "ba" => Tense::Ba,
            _ => return None,
        };
        self.pos += 1;
        Some(t)
    }

    /// Try to consume a deontic attitudinal (ei/ehe).
    pub(crate) fn try_parse_attitudinal(&mut self) -> Option<Attitudinal> {
        let a = match self.peek_cmavo()? {
            "ei" => Attitudinal::Ei,
            "e'e" => Attitudinal::Ehe,
            _ => return None,
        };
        self.pos += 1;
        Some(a)
    }

    // ─── Sentence ─────────────────────────────────────────────

    /// Try to parse a prenex: `(ro (da|de|di))+ zo'u`. Returns the universally
    /// quantified variable names in prenex order, restoring the position if this
    /// is not a prenex (e.g. `ro lo gerku ...` — a universal description — or a
    /// `ro <var>` sequence with no `zo'u` terminator). `zo'u` already lexes as a
    /// plain cmavo.
    fn try_parse_prenex(&mut self) -> Option<Vec<&'arena str>> {
        let saved = self.save();
        let mut vars = Vec::new();
        while self.eat_cmavo("ro") {
            match self.peek_cmavo() {
                Some(v @ ("da" | "de" | "di")) => {
                    let name = &*self.arena.alloc_str(v);
                    self.pos += 1;
                    vars.push(name);
                }
                // `ro` not followed by a logic variable (e.g. `ro lo gerku`):
                // this is a universal description, not a prenex.
                _ => {
                    self.restore(saved);
                    return None;
                }
            }
        }
        if vars.is_empty() || !self.eat_cmavo("zo'u") {
            // No `ro <var>` sequence, or no `zo'u` terminator: not a prenex.
            self.restore(saved);
            return None;
        }
        Some(vars)
    }

    /// Parse a sentence: prenex, forethought connective, or simple bridi.
    pub(crate) fn parse_sentence(&mut self) -> Result<Sentence<'arena>, ParseError> {
        self.enter()?;

        // Prenex `ro da ro de zo'u <sentence>` quantifies a whole (recursive)
        // sentence body. Try it before the forethought/simple paths.
        if let Some(vars) = self.try_parse_prenex() {
            let body = self.parse_sentence()?;
            self.leave();
            return Ok(Sentence::Prenex {
                vars: self.arena.alloc_slice_fill_iter(vars),
                body: self.arena.alloc(body),
            });
        }

        let conn = if self.eat_cmavo("ganai") {
            Some(SentenceConnective::GanaiGi)
        } else if self.eat_cmavo("ge") {
            Some(SentenceConnective::GeGi)
        } else if self.eat_cmavo("ga") {
            Some(SentenceConnective::GaGi)
        } else if self.eat_cmavo("go") {
            Some(SentenceConnective::GoGi)
        } else {
            None
        };

        if let Some(connective) = conn {
            let left = self.parse_sentence()?;

            if !self.eat_cmavo("gi") {
                self.leave();
                return Err(
                    self.error("expected 'gi' after forethought connective and first sentence")
                );
            }

            let right = self.parse_sentence()?;

            self.leave();
            return Ok(Sentence::Connected {
                connective,
                left: self.arena.alloc(left),
                right: self.arena.alloc(right),
            });
        }

        let bridi = self.parse_simple_sentence()?;

        // GIhA bridi-tail connectives: `mi klama gi'e citka` — each tail is a full
        // predication (its own selbri + trailing sumti) SHARING the head terms. Two
        // lowerings, chosen by the head:
        //  - CONSTANT head (name, non-`da` pro-sumti, number, quoted, `la`-desc):
        //    repeating the head per tail is semantically exact, so desugar to the
        //    same `Sentence::Connected`/`Afterthought` shape as `.i je` — the proven
        //    path (smuni/flatten/go'i handle it unchanged).
        //  - QUANTIFIED/DESCRIPTION head (`lo terdi`, `da`, `re lo …`): repeating it
        //    would RE-QUANTIFY the head per tail (`∃x.p(x) ∧ ∃y.q(y)` instead of
        //    `∃x.(p(x) ∧ q(x))`) — a wrong TRUE on disjoint witnesses. Emit a
        //    `Sentence::SharedHead` so smuni binds the head witness ONCE across all
        //    tails. (The fix; these heads were previously rejected fail-closed.)
        //
        // Fused `gi'enai`… arrive as single Cmavo tokens (lexer's
        // `reclassify_fused_giha_nai`); spaced `gi'e nai` is two tokens — both negate
        // the right tail.
        let mut tails: Vec<GihaTail<'arena>> = Vec::new();
        loop {
            let (connective, fused_nai) = match self.peek_cmavo() {
                Some("gi'e") => (Connective::Je, false),
                Some("gi'a") => (Connective::Ja, false),
                Some("gi'o") => (Connective::Jo, false),
                Some("gi'u") => (Connective::Ju, false),
                Some("gi'enai") => (Connective::Je, true),
                Some("gi'anai") => (Connective::Ja, true),
                Some("gi'onai") => (Connective::Jo, true),
                Some("gi'unai") => (Connective::Ju, true),
                _ => break,
            };
            self.pos += 1;
            let right_negated = fused_nai || self.eat_cmavo("nai");
            tails.push(self.parse_bridi_tail(connective, right_negated)?);
        }

        self.leave();
        if tails.is_empty() {
            return Ok(Sentence::Simple(bridi));
        }
        if bridi.head_terms.iter().all(giha_safe_head) {
            // Constant head → the proven repeated-head `.i je` desugar (each tail's
            // `Bridi` shares the head slice by reference).
            let head_terms = bridi.head_terms;
            let mut sentence = Sentence::Simple(bridi);
            for t in &tails {
                let tail_bridi = Bridi {
                    selbri: t.selbri.clone(),
                    head_terms,
                    tail_terms: t.tail_terms,
                    negated: t.negated,
                    tense: None,
                    attitudinal: None,
                };
                sentence = Sentence::Connected {
                    connective: SentenceConnective::Afterthought {
                        left_negated: false,
                        connective: t.connective,
                        right_negated: t.right_negated,
                    },
                    left: self.arena.alloc(sentence),
                    right: self.arena.alloc(Sentence::Simple(tail_bridi)),
                };
            }
            return Ok(sentence);
        }
        // Quantified/description head → SharedHead (bind the witness once). A
        // connected sumti in the shared head would need distributing the whole unit
        // — not yet supported; fail closed (it was rejected before too).
        if bridi.head_terms.iter().any(head_has_connective) {
            return Err(self.error(
                "a GIhA bridi-tail with a connected sumti (.e/.a/.o/.u) in the shared \
                 head is not yet supported; restate as separate `.i je` sentences",
            ));
        }
        Ok(Sentence::SharedHead {
            head: self.arena.alloc(bridi),
            tails: self.arena.alloc_slice_fill_iter(tails),
        })
    }

    /// Parse one GIhA bridi-tail: `[na] selbri tail-terms [vau]`, into a
    /// [`GihaTail`] that reuses the chain's shared head. A leading `na` negates
    /// this tail only (`GihaTail::negated`, mirroring the simple-sentence unwrap of
    /// a top-level `Selbri::Negated`). `connective`/`right_negated` are read by the
    /// caller from the GIhA token (and any following `nai`).
    fn parse_bridi_tail(
        &mut self,
        connective: Connective,
        right_negated: bool,
    ) -> Result<GihaTail<'arena>, ParseError> {
        if matches!(self.peek_cmavo(), Some("pu" | "ca" | "ba")) {
            return Err(self.error(
                "a tense marker on a GIhA bridi-tail is not supported (a tense \
                 before the first selbri applies to the first tail only); \
                 restate as separate `.i` sentences to tense each claim",
            ));
        }
        let selbri = match self.try_parse_selbri()? {
            Some(s) => s,
            None => return Err(self.error("expected selbri after bridi-tail connective")),
        };
        let (selbri, negated) = match selbri {
            Selbri::Negated(inner) => (inner.clone(), true),
            other => (other, false),
        };

        let tail_terms = self.parse_terms();
        self.eat_cmavo("vau");

        Ok(GihaTail {
            connective,
            right_negated,
            selbri,
            tail_terms: self.arena.alloc_slice_fill_iter(tail_terms),
            negated,
        })
    }

    /// Parse a simple (non-connected) sentence into a Bridi.
    pub(crate) fn parse_simple_sentence(&mut self) -> Result<Bridi<'arena>, ParseError> {
        self.enter()?;

        let mut tense = self.try_parse_tense();
        let mut attitudinal = self.try_parse_attitudinal();

        let head_terms = self.parse_terms();
        while self.eat_pause() {}
        self.eat_cmavo("cu");

        if tense.is_none() {
            tense = self.try_parse_tense();
        }
        if attitudinal.is_none() {
            attitudinal = self.try_parse_attitudinal();
        }

        let selbri = match self.try_parse_selbri()? {
            Some(s) => s,
            None => {
                // No selbri. A clause with head terms but no predicate is a bare
                // sumti / observative — NOT a complete bridi. Fail closed with a
                // clear, distinct error rather than fabricating a `go'i` selbri
                // (which is indistinguishable from an explicit `go'i` and, once
                // resolved with no antecedent, reports the cryptic "go'i has no
                // antecedent"). A dangling sumti connective keeps its precise,
                // positioned diagnostic (matching the terminal unconsumed-token
                // check). Explicit `go'i` is the `Some(_)` branch above.
                self.leave();
                let msg = if head_terms.is_empty() {
                    "expected selbri or terms"
                } else if self.is_dangling_sumti_connective() {
                    self.unconsumed_error_msg()
                } else {
                    "a bridi needs a selbri: a bare sumti is not a complete statement"
                };
                return Err(self.error(msg));
            }
        };

        let (selbri, negated) = match selbri {
            Selbri::Negated(inner) => (inner.clone(), true),
            other => (other, false),
        };

        let tail_terms = self.parse_terms();
        self.eat_cmavo("vau");

        self.leave();
        Ok(Bridi {
            selbri,
            head_terms: self.arena.alloc_slice_fill_iter(head_terms),
            tail_terms: self.arena.alloc_slice_fill_iter(tail_terms),
            negated,
            tense,
            attitudinal,
        })
    }
}

/// True if a head sumti denotes a CONSTANT that can be repeated per GIhA tail
/// without changing meaning: names, non-variable pro-sumti, quoted literals, and
/// numbers (plus place-tagged wrappers of those). Such heads use the proven
/// repeated-head `.i je` desugar. Everything that mints a fresh witness/quantifier
/// per compilation — descriptions, `da`/`de`/`di`, quantified descriptions — routes
/// to the smuni-level `SharedHead` instead (which binds the witness ONCE).
fn giha_safe_head(s: &Sumti) -> bool {
    match s {
        Sumti::ProSumti(w) => !matches!(*w, "da" | "de" | "di"),
        Sumti::Name(_) | Sumti::QuotedLiteral(_) | Sumti::Number(_) => true,
        // `la <selbri>` is a rigid name — smuni compiles it to a bare Constant
        // (no witness, no quantifier), so repeating it per tail is exact.
        Sumti::Description {
            gadri: Gadri::La, ..
        } => true,
        Sumti::Tagged(_, inner) => giha_safe_head(inner),
        _ => false,
    }
}

/// True if a GIhA shared-head term is (or wraps) a connected sumti (`.e`/`.a`/…).
/// Distributing a connected sumti across a whole shared-head GIhA unit is not yet
/// supported, so such (non-constant) heads fail closed (restate as `.i je`).
fn head_has_connective(s: &Sumti) -> bool {
    match s {
        Sumti::Connected { .. } => true,
        Sumti::Tagged(_, inner) => head_has_connective(inner),
        _ => false,
    }
}
