//! Bridi and sentence compilation: the main compilation entry points.
//!
//! Compiles bridi (predication) nodes and sentence connectives into FOL.
//! Handles place tags (fa/fe/fi/fo/fu), modal tags (BAI, fi'o), sumti
//! connective expansion, quantifier closure, da/de/di existential wrapping,
//! tense wrappers (pu/ca/ba), and deontic attitudinals (ei/e'e).
use super::*;

/// A connected sumti (`.e`/`.a`/`.o`/`.u`) found in a bridi's term list, ready to
/// distribute. Captures the connective + operands plus the wrapper (if any) the
/// connected sumti sits under, so distribution can preserve the place tag / BAI
/// modal over each operand.
struct ConnectedSplit {
    /// Position of the connected term within `head_terms ++ tail_terms`.
    term_pos: usize,
    wrapper: ConnWrapper,
    connective: Connective,
    right_negated: bool,
    left_id: u32,
    right_id: u32,
}

/// What a connected sumti sits under in a bridi term slot.
enum ConnWrapper {
    /// A bare `Sumti::Connected` term.
    Bare,
    /// `Sumti::Tagged((tag, Connected(..)))` — a place tag over a connected sumti.
    Place(PlaceTag),
    /// `Sumti::ModalTagged((modal, Connected(..)))` — a BAI modal over a connected sumti.
    Modal(ModalTag),
}

impl SemanticCompiler {
    /// Compiles a bridi (predication) into FOL with quantifier scoping and tense wrapping.
    pub fn compile_bridi(
        &mut self,
        bridi: &Bridi,
        selbris: &[Selbri],
        sumtis: &[Sumti],
        sentences: &[Sentence],
    ) -> LogicalForm {
        // Frame-local checkpoint for rel clauses attached to non-quantifier
        // sumti (see `pending_matrix_conjuncts`): only conjuncts pushed by
        // THIS bridi's sumti are drained into THIS bridi's matrix; nested
        // bridi (rel clause bodies, abstractions) drain their own.
        let matrix_conjunct_checkpoint = self.pending_matrix_conjuncts.len();
        // Frame-scoped `ma` closure: each `compile_bridi` frame drains only the
        // `ma` vars pushed during ITS frame (see the drain near the end). A
        // nested bridi (rel-clause body, abstraction body) takes its own
        // checkpoint AFTER any ancestor pushes, so it can no longer steal an
        // enclosing bridi's pending `ma` var (mirrors `matrix_conjunct_checkpoint`).
        let ma_checkpoint = self.ma_vars.len();

        let all_terms: Vec<u32> = bridi
            .head_terms
            .iter()
            .chain(bridi.tail_terms.iter())
            .copied()
            .collect();

        // Distribute connected sumti (`.e`/`.a`/`.o`/`.u`) — including a
        // connected sumti nested under a place tag (`fa mi .e do`) or BAI modal
        // (`ri'a do .e ti`), where the wrapper is preserved over each operand.
        // Only the first connected term is split here; the recursive
        // `compile_bridi` re-scans for the rest, so every connected sumti in the
        // bridi distributes.
        if let Some(split) = Self::find_connected_term(&all_terms, sumtis) {
            return self.distribute_connected(bridi, split, selbris, sumtis, sentences);
        }

        let target_arity = self.get_selbri_arity(bridi.relation, selbris);

        let mut positioned: Vec<Option<LogicalTerm>> = vec![None; target_arity];

        // A relative clause's implicit `ke'a` subject occupies x1 (the CLL
        // default), pushing the clause's explicit sumti to x2+. Place it as the
        // x1 ARGUMENT here — BEFORE `apply_selbri` runs any `se`/`te`/`ve`/`xe`
        // conversion — so `poi se prami la .alis.` routes `ke'a` through the
        // conversion to the correct underlying role (prami_x2), exactly as an
        // explicit subject would. One-shot: only the clause's main (first) bridi
        // consumes it; nested bridi (abstraction bodies) see `None`. Marking
        // `kea_used` makes the caller skip the post-hoc `inject_variable`, which
        // cannot see conversion and would refill the vacated x1 slot.
        if bridi.head_terms.is_empty() && target_arity >= 1 {
            if let Some(subject) = self.pending_clause_subject.take() {
                positioned[0] = Some(LogicalTerm::Variable(subject));
                self.kea_used = true;
            }
        }

        // Surface-ordered scope introductions (descriptions + bare da/de/di),
        // folded in reverse below so the leftmost binder is outermost.
        let mut markers: Vec<ScopeMarker> = Vec::new();
        // da/de/di already recorded as a `Bare` marker — dedups a co-referring
        // `da` and lets the safety-net residual pass skip surface-captured vars.
        let mut introduced: std::collections::HashSet<lasso::Spur> =
            std::collections::HashSet::new();
        let mut modal_entries: Vec<(ModalTag, LogicalTerm, Vec<QuantifierEntry>)> = Vec::new();
        // Untagged sumti that overflowed the selbri's arity (placed nowhere).
        // Preserves the prior silent-drop behaviour for over-arity untagged
        // sumti, and drives the fail-closed `du` n-ary check below.
        let mut overflow_untagged: usize = 0;
        // CLL place counter (CLL ch.9, FA cmavo): `fa/fe/fi/fo/fu` set the place
        // number; a following UNTAGGED sumti fills the place AFTER the last tag,
        // not the first free slot. Starts at x1 and skips slots already filled
        // (a ke'a x1 pre-fill, or an out-of-order tag).
        let mut next_place: usize = 0;

        for &term_id in bridi.head_terms.iter().chain(bridi.tail_terms.iter()) {
            match &sumtis[term_id as usize] {
                Sumti::Tagged((tag, inner_id)) => {
                    let inner = &sumtis[*inner_id as usize];
                    let (term, quants) = self.resolve_sumti(inner, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut introduced, &mut markers);
                    markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    let idx = tag.to_index();
                    if idx >= target_arity {
                        // FAIL CLOSED: a place tag beyond the selbri's arity has no
                        // slot to bind into. Silently dropping the tagged term loses
                        // meaning (panel finding 2026-06-10) — reject instead.
                        self.errors.push(format!(
                            "Place tag `{}` targets place x{}, but the selbri only has \
                             {} place(s); the tagged term cannot be placed.",
                            tag.name(),
                            idx + 1,
                            target_arity
                        ));
                    } else if positioned[idx].is_some() {
                        // FAIL CLOSED: a tag re-targeting an already-filled place
                        // (`fe X fe Y`, or a tag colliding with an untagged term /
                        // ke'a) would last-win and drop the earlier term.
                        self.errors.push(format!(
                            "Place tag `{}` targets place x{}, which is already filled; \
                             the same place cannot be set twice.",
                            tag.name(),
                            idx + 1
                        ));
                    } else {
                        positioned[idx] = Some(term);
                        next_place = idx + 1; // CLL: resume AFTER the tagged place
                    }
                }
                Sumti::ModalTagged((modal_tag, inner_id)) => {
                    let inner = &sumtis[*inner_id as usize];
                    let (term, quants) = self.resolve_sumti(inner, sumtis, selbris, sentences);
                    // A bare `da`/`de`/`di` carried by a BAI modal (`ri'a da`) is
                    // introduced at this surface position. Its description quants
                    // (rare: `ri'a lo broda`) stay innermost (appended after the
                    // loop, with the modal predicate).
                    self.record_bare_marker(&term, &mut introduced, &mut markers);
                    // BAI modals are not place-filling — they do NOT advance the
                    // place counter.
                    modal_entries.push((*modal_tag, term, quants));
                }
                other => {
                    let (term, quants) = self.resolve_sumti(other, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut introduced, &mut markers);
                    markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    // Skip slots already filled (ke'a x1, or an out-of-order tag),
                    // then fill the current place and advance.
                    while next_place < target_arity && positioned[next_place].is_some() {
                        next_place += 1;
                    }
                    if next_place < target_arity {
                        positioned[next_place] = Some(term);
                        next_place += 1;
                    } else {
                        overflow_untagged += 1;
                    }
                }
            }
        }

        let args: Vec<LogicalTerm> = positioned
            .into_iter()
            .map(|slot| slot.unwrap_or(LogicalTerm::Unspecified))
            .collect();

        // Fail-closed: untagged sumti that overflow the selbri's places were
        // dropped above (counted in `overflow_untagged`); reject rather than lose
        // meaning. `du` (a 2-place identity, consumed binary by logji's union-find)
        // gets a specific message; any other selbri errors too — but ONLY when its
        // arity is KNOWN in jbovlaste (an unknown word defaults to arity 2 and its
        // real arity may be higher, so an "overflow" there is unprovable; this also
        // keeps the no-XML build, where many proxy words default to 2, from
        // false-firing).
        let head_name = self.get_selbri_head_name(bridi.relation, selbris);
        if overflow_untagged > 0 {
            if head_name == "du" {
                self.errors.push(format!(
                    "`du` (identity) is a 2-place relation, but {} extra sumti were supplied; \
                     n-ary identity is unsupported.",
                    overflow_untagged
                ));
            } else if JbovlasteSchema::get_arity(head_name).is_some() {
                self.errors.push(format!(
                    "{} untagged sumti overflow the selbri `{}`'s {} place(s); the extra \
                     sumti cannot be placed.",
                    overflow_untagged, head_name, target_arity
                ));
            }
        }

        let mut final_form = self.apply_selbri(bridi.relation, &args, selbris, sumtis, sentences);

        for (modal_tag, tagged_term, modal_quants) in modal_entries {
            markers.extend(modal_quants.into_iter().map(ScopeMarker::Desc));

            let (modal_gismu, modal_arity) = match &modal_tag {
                ModalTag::Fixed(bai) => {
                    let gismu = Self::bai_to_gismu(bai);
                    let arity = JbovlasteSchema::get_arity_or_default(gismu);
                    (self.interner.get_or_intern(gismu), arity)
                }
                ModalTag::Fio(selbri_id) => {
                    let name = self.get_selbri_head_name(*selbri_id, selbris);
                    let arity = self.get_selbri_arity(*selbri_id, selbris);
                    (self.interner.get_or_intern(name), arity)
                }
            };

            // FAIL CLOSED: a modal relates its tagged sumti (the modal selbri's x1)
            // to the main bridi's x1 (its x2), so the modal selbri needs at least 2
            // places. A 1-place selbri has no x2 to carry the main-bridi link — only
            // reachable via `fi'o` over an arity-1 selbri (every BAI gismu is
            // arity >= 2). Silently dropping `main_x1` loses meaning, so reject.
            if modal_arity < 2 {
                let modal_name = self.interner.resolve(&modal_gismu).to_string();
                self.errors.push(format!(
                    "Modal tag `{}` maps to a {}-place selbri, but a modal needs at \
                     least 2 places (x1 = the tag's own sumti, x2 = the main bridi's \
                     x1 link); the main bridi's x1 cannot be carried.",
                    modal_name, modal_arity
                ));
                continue;
            }

            let main_x1 = args.first().cloned().unwrap_or(LogicalTerm::Unspecified);
            let mut modal_args = vec![LogicalTerm::Unspecified; modal_arity];
            modal_args[0] = tagged_term;
            modal_args[1] = main_x1;

            let modal_form = LogicalForm::Predicate {
                relation: modal_gismu,
                args: modal_args,
            };

            final_form = LogicalForm::And(Box::new(final_form), Box::new(modal_form));
        }

        // Conjoin rel clauses attached to non-quantifier sumti (la names, le
        // descriptions, pro-sumti) into the bridi matrix. These were
        // previously compiled then silently DISCARDED (panel finding
        // 2026-06-10), so `la .adam. poi gerku cu klama` answered TRUE with
        // only klama(adam) known.
        let pending: Vec<LogicalForm> = self
            .pending_matrix_conjuncts
            .split_off(matrix_conjunct_checkpoint);
        for conj in pending {
            final_form = LogicalForm::And(Box::new(final_form), Box::new(conj));
        }

        // Quantifier scope follows Lojban surface order (leftmost = outermost).
        // `markers` recorded every scope introduction — description quantifiers
        // AND bare logic variables (da/de/di) — in source order during the term
        // loop above. Folding the list in REVERSE makes the first-introduced
        // quantifier the outermost binder, so `da citka ro lo gerku` yields
        // `∃da.∀x` (the leading bare var outscopes the universal — an
        // Exists-over-ForAll root that logji's assertion dispatch now accepts by
        // skolemizing the leading ∃) while `ro lo gerku cu citka da` yields
        // `∀x.∃da` (unchanged).
        //
        // Safety net: a da/de/di reachable only via a merged predicate — a be/bei
        // role arg (`klama be da`) or any var the surface loop did not capture —
        // has no well-defined surface position, so it is collected from the built
        // `final_form` and closed INNERMOST (the conservative default). This
        // guarantees no bare var is ever left free; `introduced` excludes the
        // surface-captured vars so none is double-wrapped. Binder tracking in
        // `collect_free_logic_vars` skips abstraction-bound and prenex-bound vars,
        // and the description bodies / rel-clause restrictors are not folded into
        // `final_form` yet (they wrap below), so they are correctly out of scope.
        //
        // This innermost closure is a DELIBERATE, ACCEPTED boundary (not a
        // deferred TODO): a be/bei-arg or restrictor-internal `da` is soundly
        // closed innermost. A restrictor-internal `da` can never diverge (it is
        // bound inside the very quantifier whose domain the restrictor defines);
        // the ONLY construct where surface order would differ is an obscure
        // be-arg `da` preceding a tail-term universal (`klama be da ro lo gerku`),
        // where innermost gives ∀∃ vs surface ∃∀ — and even there innermost
        // merely under-claims (sound for assertions). Surface interleaving would
        // need source spans the flat AST does not carry, for ~zero semantic gain.
        // Locked by `test_da_in_be_arg_closed`,
        // `test_be_arg_da_with_universal_stays_innermost`, and
        // `test_restrictor_internal_da_closed_innermost`.
        let mut all_free_seen = std::collections::HashSet::new();
        let mut all_free: Vec<lasso::Spur> = Vec::new();
        let mut bound_vars: Vec<lasso::Spur> = Vec::new();
        Self::collect_free_logic_vars(
            &final_form,
            &self.interner,
            &self.prenex_vars,
            &mut bound_vars,
            &mut all_free_seen,
            &mut all_free,
        );
        for var in &all_free {
            if !introduced.contains(var) {
                final_form = LogicalForm::Exists(*var, Box::new(final_form));
            }
        }

        let has_universal_quantifier = markers.iter().any(|m| {
            matches!(
                m,
                ScopeMarker::Desc(e)
                    if matches!(e.kind, QuantifierKind::Universal | QuantifierKind::UniversalLe)
            )
        });

        for marker in markers.into_iter().rev() {
            final_form = match marker {
                ScopeMarker::Desc(entry) => {
                    self.close_quantifier(entry, final_form, selbris, sumtis, sentences)
                }
                ScopeMarker::Bare(var) => LogicalForm::Exists(var, Box::new(final_form)),
            };
        }

        // Rare corner: a rel clause on a non-quantifier sumti nested inside a
        // description restrictor (e.g. the be-arg in `lo gerku be la .adam.
        // poi prenu`) pushes its conjunct while the closure loop above
        // compiles the restrictor — too late to join the matrix. Conjoin it
        // at the top level when sound (no universal: the root stays a ground
        // conjunction); under a universal the root must remain ForAll for
        // rule compilation, so FAIL CLOSED rather than silently drop.
        let late: Vec<LogicalForm> = self
            .pending_matrix_conjuncts
            .split_off(matrix_conjunct_checkpoint);
        if !late.is_empty() {
            if has_universal_quantifier {
                self.errors.push(
                    "Relative clause on a name/description inside a universal \
                     description's restrictor cannot be represented; restate it \
                     as a separate sentence."
                        .to_string(),
                );
            } else {
                for conj in late {
                    final_form = LogicalForm::And(Box::new(final_form), Box::new(conj));
                }
            }
        }

        for var in self.ma_vars.drain(ma_checkpoint..) {
            final_form = LogicalForm::Exists(var, Box::new(final_form));
        }

        if bridi.negated {
            final_form = LogicalForm::Not(Box::new(final_form));
        }

        match &bridi.tense {
            Some(Tense::Pu) => {
                final_form = LogicalForm::Past(Box::new(final_form));
            }
            Some(Tense::Ca) => {
                final_form = LogicalForm::Present(Box::new(final_form));
            }
            Some(Tense::Ba) => {
                final_form = LogicalForm::Future(Box::new(final_form));
            }
            None => {}
        }

        match &bridi.attitudinal {
            Some(Attitudinal::Ei) => {
                final_form = LogicalForm::Obligatory(Box::new(final_form));
            }
            Some(Attitudinal::Ehe) => {
                final_form = LogicalForm::Permitted(Box::new(final_form));
            }
            None => {}
        }

        final_form
    }

    /// Walk a compiled `LogicalForm` collecting free `da`/`de`/`di` logic
    /// variables for existential closure. Tracks binders (`Exists`/`ForAll`/
    /// `Count`) so a var already bound (e.g. by an abstraction body's own
    /// closure) is skipped — no double-wrap — and excludes prenex-bound vars.
    /// Dedups via `seen`; `out` preserves first-appearance order.
    fn collect_free_logic_vars(
        form: &LogicalForm,
        interner: &Rodeo,
        prenex: &std::collections::HashSet<lasso::Spur>,
        bound: &mut Vec<lasso::Spur>,
        seen: &mut std::collections::HashSet<lasso::Spur>,
        out: &mut Vec<lasso::Spur>,
    ) {
        match form {
            LogicalForm::Predicate { args, .. } => {
                for arg in args {
                    if let LogicalTerm::Variable(spur) = arg {
                        let name = interner.resolve(spur);
                        if matches!(name, "da" | "de" | "di")
                            && !bound.contains(spur)
                            && !prenex.contains(spur)
                            && seen.insert(*spur)
                        {
                            out.push(*spur);
                        }
                    }
                }
            }
            LogicalForm::And(l, r)
            | LogicalForm::Or(l, r)
            | LogicalForm::Biconditional(l, r)
            | LogicalForm::Xor(l, r) => {
                Self::collect_free_logic_vars(l, interner, prenex, bound, seen, out);
                Self::collect_free_logic_vars(r, interner, prenex, bound, seen, out);
            }
            LogicalForm::Not(inner)
            | LogicalForm::Past(inner)
            | LogicalForm::Present(inner)
            | LogicalForm::Future(inner)
            | LogicalForm::Obligatory(inner)
            | LogicalForm::Permitted(inner) => {
                Self::collect_free_logic_vars(inner, interner, prenex, bound, seen, out);
            }
            LogicalForm::Exists(v, body) | LogicalForm::ForAll(v, body) => {
                bound.push(*v);
                Self::collect_free_logic_vars(body, interner, prenex, bound, seen, out);
                bound.pop();
            }
            LogicalForm::Count { var, body, .. } => {
                bound.push(*var);
                Self::collect_free_logic_vars(body, interner, prenex, bound, seen, out);
                bound.pop();
            }
        }
    }

    /// Record a surface-ordered `Bare` scope marker if `term` is a bare logic
    /// variable (`da`/`de`/`di`) seen for the first time in this bridi frame and
    /// not prenex-bound. Dedups a co-referring var via `introduced`. Reads only
    /// `self.interner`/`self.prenex_vars`; mutates the caller's frame-local
    /// `introduced`/`markers`.
    fn record_bare_marker(
        &self,
        term: &LogicalTerm,
        introduced: &mut std::collections::HashSet<lasso::Spur>,
        markers: &mut Vec<ScopeMarker>,
    ) {
        if let LogicalTerm::Variable(spur) = term {
            let spur = *spur;
            let is_logic_var = matches!(self.interner.resolve(&spur), "da" | "de" | "di");
            if is_logic_var && !self.prenex_vars.contains(&spur) && introduced.insert(spur) {
                markers.push(ScopeMarker::Bare(spur));
            }
        }
    }

    /// Find the first term (in `head_terms ++ tail_terms` order) that is — or
    /// wraps, one level under a place tag / BAI modal — a connected sumti.
    /// Returns everything `distribute_connected` needs to split it.
    fn find_connected_term(all_terms: &[u32], sumtis: &[Sumti]) -> Option<ConnectedSplit> {
        for (term_pos, &term_id) in all_terms.iter().enumerate() {
            let term = &sumtis[term_id as usize];
            let (wrapper, inner) = match term {
                Sumti::Connected(_) => (ConnWrapper::Bare, term),
                Sumti::Tagged((tag, inner_id)) => {
                    (ConnWrapper::Place(*tag), &sumtis[*inner_id as usize])
                }
                Sumti::ModalTagged((modal, inner_id)) => {
                    (ConnWrapper::Modal(*modal), &sumtis[*inner_id as usize])
                }
                _ => continue,
            };
            if let Sumti::Connected((left_id, connective, right_negated, right_id)) = inner {
                return Some(ConnectedSplit {
                    term_pos,
                    wrapper,
                    connective: *connective,
                    right_negated: *right_negated,
                    left_id: *left_id,
                    right_id: *right_id,
                });
            }
        }
        None
    }

    /// Distribute a connected sumti into a logical combination of two bridi (one
    /// per operand), preserving any place tag / BAI modal wrapper over each
    /// operand. Recurses through `compile_bridi`, which re-scans for further
    /// connected sumti, so every connected term in the bridi is distributed.
    fn distribute_connected(
        &mut self,
        bridi: &Bridi,
        split: ConnectedSplit,
        selbris: &[Selbri],
        sumtis: &[Sumti],
        sentences: &[Sentence],
    ) -> LogicalForm {
        // A wrapped connected sumti needs a tag/modal node synthesised over each
        // operand (these don't exist in the immutable `sumtis` buffer); a bare
        // connected sumti reuses its existing operand ids unchanged.
        let mut ext: Vec<Sumti> = sumtis.to_vec();
        let (left_slot, right_slot) = match split.wrapper {
            ConnWrapper::Bare => (split.left_id, split.right_id),
            ConnWrapper::Place(tag) => {
                let base = ext.len() as u32;
                ext.push(Sumti::Tagged((tag, split.left_id)));
                ext.push(Sumti::Tagged((tag, split.right_id)));
                (base, base + 1)
            }
            ConnWrapper::Modal(modal) => {
                let base = ext.len() as u32;
                ext.push(Sumti::ModalTagged((modal, split.left_id)));
                ext.push(Sumti::ModalTagged((modal, split.right_id)));
                (base, base + 1)
            }
        };

        let head_len = bridi.head_terms.len();
        let term_pos = split.term_pos;
        let substitute = |replacement_id: u32| -> Bridi {
            let mut head = bridi.head_terms.clone();
            let mut tail = bridi.tail_terms.clone();
            if term_pos < head_len {
                head[term_pos] = replacement_id;
            } else {
                tail[term_pos - head_len] = replacement_id;
            }
            Bridi {
                relation: bridi.relation,
                head_terms: head,
                tail_terms: tail,
                negated: bridi.negated,
                tense: bridi.tense,
                attitudinal: bridi.attitudinal,
            }
        };

        let left_bridi = substitute(left_slot);
        let right_bridi = substitute(right_slot);

        let left_form = self.compile_bridi(&left_bridi, selbris, &ext, sentences);
        let mut right_form = self.compile_bridi(&right_bridi, selbris, &ext, sentences);

        if split.right_negated {
            right_form = LogicalForm::Not(Box::new(right_form));
        }

        match split.connective {
            Connective::Je => LogicalForm::And(Box::new(left_form), Box::new(right_form)),
            Connective::Ja => LogicalForm::Or(Box::new(left_form), Box::new(right_form)),
            Connective::Jo => LogicalForm::Biconditional(Box::new(left_form), Box::new(right_form)),
            Connective::Ju => LogicalForm::Xor(Box::new(left_form), Box::new(right_form)),
        }
    }

    /// Compiles a sentence node (simple bridi or connected sentences) into FOL.
    pub fn compile_sentence(
        &mut self,
        sentence_id: u32,
        selbris: &[Selbri],
        sumtis: &[Sumti],
        sentences: &[Sentence],
    ) -> LogicalForm {
        match &sentences[sentence_id as usize] {
            Sentence::Simple(bridi) => self.compile_bridi(bridi, selbris, sumtis, sentences),
            Sentence::Prenex((vars, body_id)) => {
                // `ro da [ro de ...] zo'u BODY` → ∀da. ∀de. … BODY.
                // Intern each prenex variable and mark it bound so the body's
                // compile_bridi does NOT existentially close it; then wrap the
                // compiled body in nested ForAll (outermost = first variable).
                let spurs: Vec<lasso::Spur> = vars
                    .iter()
                    .map(|v| self.interner.get_or_intern(v))
                    .collect();
                let saved: Vec<lasso::Spur> = spurs
                    .iter()
                    .filter(|s| self.prenex_vars.insert(**s))
                    .copied()
                    .collect();

                let mut form = self.compile_sentence(*body_id, selbris, sumtis, sentences);

                // Wrap inner-to-outer so the first variable is the outermost ∀.
                for spur in spurs.iter().rev() {
                    form = LogicalForm::ForAll(*spur, Box::new(form));
                }

                // Restore: only remove the vars THIS prenex introduced (a nested
                // prenex may share a name with an outer one).
                for s in saved {
                    self.prenex_vars.remove(&s);
                }
                form
            }
            Sentence::Connected((connective, left_id, right_id)) => {
                let left_form = self.compile_sentence(*left_id, selbris, sumtis, sentences);
                let right_form = self.compile_sentence(*right_id, selbris, sumtis, sentences);

                match connective {
                    SentenceConnective::GanaiGi => LogicalForm::Or(
                        Box::new(LogicalForm::Not(Box::new(left_form))),
                        Box::new(right_form),
                    ),
                    SentenceConnective::GeGi => {
                        LogicalForm::And(Box::new(left_form), Box::new(right_form))
                    }
                    SentenceConnective::GaGi => {
                        LogicalForm::Or(Box::new(left_form), Box::new(right_form))
                    }
                    SentenceConnective::GoGi => {
                        LogicalForm::Biconditional(Box::new(left_form), Box::new(right_form))
                    }
                    SentenceConnective::Afterthought((left_neg, conn, right_neg)) => {
                        let l = if *left_neg {
                            LogicalForm::Not(Box::new(left_form))
                        } else {
                            left_form
                        };
                        let r = if *right_neg {
                            LogicalForm::Not(Box::new(right_form))
                        } else {
                            right_form
                        };
                        match conn {
                            Connective::Je => LogicalForm::And(Box::new(l), Box::new(r)),
                            Connective::Ja => LogicalForm::Or(Box::new(l), Box::new(r)),
                            Connective::Jo => LogicalForm::Biconditional(Box::new(l), Box::new(r)),
                            Connective::Ju => LogicalForm::Xor(Box::new(l), Box::new(r)),
                        }
                    }
                }
            }
            Sentence::SharedHead((head, tails)) => {
                self.compile_shared_head(head, tails, selbris, sumtis, sentences)
            }
        }
    }

    /// Compile a shared-head GIhA chain (`X S1 gi'e S2 …` with a
    /// quantified/description head like `lo terdi`/`da`): resolve the head ONCE and
    /// distribute its witness over the conjoined tails, so
    /// `lo terdi cu na se tarmi gi'e kunti` yields
    /// `∃v.(terdi(v) ∧ (¬(∃e.tarmi(e) ∧ tarmi_x2(e,v)) ∧ ∃e2.(kunti(e2) ∧ kunti_x1(e2,v))))`
    /// — ONE shared witness, not two disjoint ones. Constant heads never reach here
    /// (the parser keeps them on the repeated-head `.i je` desugar). Fails closed on
    /// the rare sub-corners this focused path does not model (a BAI modal, or a
    /// connected sumti in a tail); those were rejected entirely before this fix, so
    /// failing closed here is a strict improvement, never a regression.
    fn compile_shared_head(
        &mut self,
        head: &Bridi,
        tails: &[GihaTail],
        selbris: &[Selbri],
        sumtis: &[Sumti],
        sentences: &[Sentence],
    ) -> LogicalForm {
        // 1. Resolve the SHARED head terms ONCE: positioned args (by place), the
        //    head's scope markers (the `lo`/`da` witness), and its surface-captured
        //    bare vars. A BAI modal in a shared head is not modeled — fail closed.
        let mut head_positioned: Vec<(usize, LogicalTerm)> = Vec::new();
        let mut head_markers: Vec<ScopeMarker> = Vec::new();
        let mut head_introduced: std::collections::HashSet<lasso::Spur> =
            std::collections::HashSet::new();
        let mut next_place: usize = 0;
        for &term_id in &head.head_terms {
            match &sumtis[term_id as usize] {
                Sumti::Tagged((tag, inner_id)) => {
                    let inner = &sumtis[*inner_id as usize];
                    let (term, quants) = self.resolve_sumti(inner, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut head_introduced, &mut head_markers);
                    head_markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    head_positioned.push((tag.to_index(), term));
                    next_place = tag.to_index() + 1;
                }
                Sumti::ModalTagged(_) => {
                    self.errors.push(
                        "a BAI modal in the shared head of a quantified/description GIhA \
                         chain is not yet supported; restate as separate `.i` sentences"
                            .to_string(),
                    );
                }
                other => {
                    let (term, quants) = self.resolve_sumti(other, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut head_introduced, &mut head_markers);
                    head_markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    head_positioned.push((next_place, term));
                    next_place += 1;
                }
            }
        }
        let head_next_place = next_place;

        // 2. Build each branch matrix (head predication first, then each tail),
        //    every branch reusing the SHARED head args; combine left-associatively
        //    with the connectives. `right_negated` (from `gi'enai`/`gi'e nai`) wraps
        //    that tail's matrix in `Not` — inside the eventual shared scope.
        let mut acc = self.build_giha_branch(
            &head_positioned,
            head_next_place,
            head.relation,
            &head.tail_terms,
            head.negated,
            head.tense,
            head.attitudinal,
            &head_introduced,
            selbris,
            sumtis,
            sentences,
        );
        for t in tails {
            let mut r = self.build_giha_branch(
                &head_positioned,
                head_next_place,
                t.relation,
                &t.tail_terms,
                t.negated,
                None,
                None,
                &head_introduced,
                selbris,
                sumtis,
                sentences,
            );
            if t.right_negated {
                r = LogicalForm::Not(Box::new(r));
            }
            acc = match t.connective {
                Connective::Je => LogicalForm::And(Box::new(acc), Box::new(r)),
                Connective::Ja => LogicalForm::Or(Box::new(acc), Box::new(r)),
                Connective::Jo => LogicalForm::Biconditional(Box::new(acc), Box::new(r)),
                Connective::Ju => LogicalForm::Xor(Box::new(acc), Box::new(r)),
            };
        }

        // 3. Close the SHARED head scope ONCE around the combined tails. Free-var
        //    safety net first (a head `da` reachable only via a merged predicate),
        //    excluding surface-captured head vars; then the reverse marker fold binds
        //    the head's `lo`/`da` witness over BOTH tails — the whole point.
        let mut all_free_seen = std::collections::HashSet::new();
        let mut all_free: Vec<lasso::Spur> = Vec::new();
        let mut bound_vars: Vec<lasso::Spur> = Vec::new();
        Self::collect_free_logic_vars(
            &acc,
            &self.interner,
            &self.prenex_vars,
            &mut bound_vars,
            &mut all_free_seen,
            &mut all_free,
        );
        for var in &all_free {
            if !head_introduced.contains(var) {
                acc = LogicalForm::Exists(*var, Box::new(acc));
            }
        }
        for marker in head_markers.into_iter().rev() {
            acc = match marker {
                ScopeMarker::Desc(entry) => {
                    self.close_quantifier(entry, acc, selbris, sumtis, sentences)
                }
                ScopeMarker::Bare(var) => LogicalForm::Exists(var, Box::new(acc)),
            };
        }
        acc
    }

    /// Build ONE branch matrix of a shared-head GIhA chain: the shared head args
    /// (already resolved, `head_positioned`) seed the head places, this branch's own
    /// `tail_terms` fill the rest, then the branch's OWN scope (tail-local `lo`/`da`)
    /// closes here — but the shared head vars (in `head_introduced`) are left FREE so
    /// the caller binds them once around all tails. Per-branch `negated`/tense/
    /// attitudinal wrap this matrix inside the shared scope. Fails closed on a BAI
    /// modal or connected sumti in a tail (unsupported on this path).
    #[allow(clippy::too_many_arguments)]
    fn build_giha_branch(
        &mut self,
        head_positioned: &[(usize, LogicalTerm)],
        head_next_place: usize,
        relation: u32,
        tail_terms: &[u32],
        negated: bool,
        tense: Option<Tense>,
        attitudinal: Option<Attitudinal>,
        head_introduced: &std::collections::HashSet<lasso::Spur>,
        selbris: &[Selbri],
        sumtis: &[Sumti],
        sentences: &[Sentence],
    ) -> LogicalForm {
        let target_arity = self.get_selbri_arity(relation, selbris);
        let mut positioned: Vec<Option<LogicalTerm>> = vec![None; target_arity];

        // Seed the shared head args.
        for (place, term) in head_positioned {
            if *place >= target_arity {
                self.errors.push(format!(
                    "the shared GIhA head fills place x{}, but this tail's selbri has \
                     only {} place(s); the shared head cannot be placed.",
                    place + 1,
                    target_arity
                ));
            } else if positioned[*place].is_some() {
                self.errors.push(
                    "the shared GIhA head targets a place that is already filled.".to_string(),
                );
            } else {
                positioned[*place] = Some(term.clone());
            }
        }

        // Position this branch's own tail terms after the head places.
        let mut markers: Vec<ScopeMarker> = Vec::new();
        let mut introduced: std::collections::HashSet<lasso::Spur> =
            std::collections::HashSet::new();
        let mut next_place = head_next_place;
        for &term_id in tail_terms {
            match &sumtis[term_id as usize] {
                Sumti::Tagged((tag, inner_id)) => {
                    let inner = &sumtis[*inner_id as usize];
                    let (term, quants) = self.resolve_sumti(inner, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut introduced, &mut markers);
                    markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    let idx = tag.to_index();
                    if idx < target_arity && positioned[idx].is_none() {
                        positioned[idx] = Some(term);
                        next_place = idx + 1;
                    } else {
                        self.errors.push(format!(
                            "place tag x{} is unavailable in this GIhA tail.",
                            idx + 1
                        ));
                    }
                }
                Sumti::ModalTagged(_) | Sumti::Connected(_) => {
                    self.errors.push(
                        "a BAI modal or connected sumti (.e/.a/.o/.u) in a shared-head \
                         GIhA tail is not yet supported; restate as separate `.i` sentences"
                            .to_string(),
                    );
                }
                other => {
                    let (term, quants) = self.resolve_sumti(other, sumtis, selbris, sentences);
                    self.record_bare_marker(&term, &mut introduced, &mut markers);
                    markers.extend(quants.into_iter().map(ScopeMarker::Desc));
                    while next_place < target_arity && positioned[next_place].is_some() {
                        next_place += 1;
                    }
                    if next_place < target_arity {
                        positioned[next_place] = Some(term);
                        next_place += 1;
                    }
                }
            }
        }

        let args: Vec<LogicalTerm> = positioned
            .into_iter()
            .map(|slot| slot.unwrap_or(LogicalTerm::Unspecified))
            .collect();
        let mut form = self.apply_selbri(relation, &args, selbris, sumtis, sentences);

        // Close this branch's OWN scope (tail-local `lo`/`da`), leaving the shared
        // head vars free. The free-var net collects only bare `da`/`de`/`di`; it and
        // the marker fold both skip `head_introduced`, so a shared head `da` is NOT
        // closed here (the caller binds it once around all tails).
        let mut all_free_seen = std::collections::HashSet::new();
        let mut all_free: Vec<lasso::Spur> = Vec::new();
        let mut bound_vars: Vec<lasso::Spur> = Vec::new();
        Self::collect_free_logic_vars(
            &form,
            &self.interner,
            &self.prenex_vars,
            &mut bound_vars,
            &mut all_free_seen,
            &mut all_free,
        );
        for var in &all_free {
            if !introduced.contains(var) && !head_introduced.contains(var) {
                form = LogicalForm::Exists(*var, Box::new(form));
            }
        }
        for marker in markers.into_iter().rev() {
            form = match marker {
                ScopeMarker::Desc(entry) => {
                    self.close_quantifier(entry, form, selbris, sumtis, sentences)
                }
                ScopeMarker::Bare(var) => LogicalForm::Exists(var, Box::new(form)),
            };
        }

        if negated {
            form = LogicalForm::Not(Box::new(form));
        }
        match tense {
            Some(Tense::Pu) => form = LogicalForm::Past(Box::new(form)),
            Some(Tense::Ca) => form = LogicalForm::Present(Box::new(form)),
            Some(Tense::Ba) => form = LogicalForm::Future(Box::new(form)),
            None => {}
        }
        match attitudinal {
            Some(Attitudinal::Ei) => form = LogicalForm::Obligatory(Box::new(form)),
            Some(Attitudinal::Ehe) => form = LogicalForm::Permitted(Box::new(form)),
            None => {}
        }
        form
    }
}
