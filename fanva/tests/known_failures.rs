//! Known-failure regression backlog — the gerna/smuni miscompilation cases from
//! the 2026-06-10 deep code-review panel (`code-review-panel-2026-06-10.json`),
//! carried from `nibli-engine/tests/known_failures.rs` @ 6c59357c95c8. The
//! originals were written against nibli-engine reasoning APIs (`assert_text`,
//! `query_holds`, `check_contradictions`, `kb()`), which fanva does not have — so
//! here each is re-expressed as its **compile-time invariant**: parse the Lojban
//! text (`gerna::parse_checked`), compile to FOL (`smuni::compile_from_gerna_ast`,
//! via `fanva::gates::local_gates`), and assert on the flat `LogicBuffer`. Every
//! case was FIXED upstream, so these are green regression guards, not red pins.
//!
//! Reject-or-represent: several panel fixes were "fail closed on the unrepresentable
//! shape" — an equally valid outcome — so those cases assert the invariant only when
//! compilation SUCCEEDS (a compile error is accepted).
//!
//! NOT ported (no reasoning engine in fanva — logji/nibli-engine stayed in nibli):
//! contradiction detection (`la .adam. na gerku` then a contrary positive), KB
//! retraction multiplicity (duplicate ground fact survives retracting one copy),
//! the Ch-12 entailment-search perf blowup (query time, not compile), and the
//! du-equivalence stack-overflow (reasoning recursion; `du` is not surface-parseable).
//! Their compile-level slices are kept where one exists (see `negated_ground_fact_*`
//! and `ch12_consent_sentences_compile`).

use fanva::gates::local_gates;
use nibli_types::logic::{LogicBuffer, LogicNode};

/// True if the compiled buffer contains the `base` predicate or any of its
/// Neo-Davidsonian role predicates (`base_xN`). Ported from the reference file:
/// smuni event-decomposes (`gerku` → `gerku(ev) ∧ gerku_x1(ev, …)`), so a plain
/// `rel == base` check misses the role atoms.
fn has_predicate_base(buf: &LogicBuffer, base: &str) -> bool {
    let role_prefix = format!("{base}_x");
    buf.nodes.iter().any(|n| match n {
        LogicNode::Predicate((rel, _)) => rel == base || rel.starts_with(&role_prefix),
        _ => false,
    })
}

fn has_node(buf: &LogicBuffer, pred: impl Fn(&LogicNode) -> bool) -> bool {
    buf.nodes.iter().any(pred)
}

/// Follow the flat `u32` child indices from `id`; true if the subtree holds an
/// `ExistsNode`.
fn subtree_has_exists(buf: &LogicBuffer, id: u32) -> bool {
    match &buf.nodes[id as usize] {
        LogicNode::ExistsNode(_) => true,
        LogicNode::ForAllNode((_, b))
        | LogicNode::NotNode(b)
        | LogicNode::PastNode(b)
        | LogicNode::PresentNode(b)
        | LogicNode::FutureNode(b)
        | LogicNode::ObligatoryNode(b)
        | LogicNode::PermittedNode(b) => subtree_has_exists(buf, *b),
        LogicNode::CountNode((_, _, b)) => subtree_has_exists(buf, *b),
        LogicNode::AndNode((l, r)) | LogicNode::OrNode((l, r)) => {
            subtree_has_exists(buf, *l) || subtree_has_exists(buf, *r)
        }
        LogicNode::Predicate(_) | LogicNode::ComputeNode(_) => false,
    }
}

/// True if some `ForAllNode`'s BODY subtree contains an `ExistsNode` — i.e. a `∃`
/// nested INSIDE a `∀` (`∀x.…∃y…`), the correct scope. The bug this pins closed the
/// `∃` OUTSIDE the `∀` (`∃y.∀x…`), which drops the rule.
fn exists_under_forall(buf: &LogicBuffer) -> bool {
    buf.nodes.iter().any(|n| match n {
        LogicNode::ForAllNode((_, body)) => subtree_has_exists(buf, *body),
        _ => false,
    })
}

fn is_or(n: &LogicNode) -> bool {
    matches!(n, LogicNode::OrNode(_))
}
fn is_not(n: &LogicNode) -> bool {
    matches!(n, LogicNode::NotNode(_))
}
fn is_past(n: &LogicNode) -> bool {
    matches!(n, LogicNode::PastNode(_))
}

// ─── smuni silently discards meaning ────────────────────────────────────────

#[test]
fn abstraction_body_over_connected_keeps_consequent() {
    // The `lo nu ganai … gi …` abstraction body's consequent `klama` must survive.
    // The flattener bug bound the abstraction to the antecedent bridi index, so
    // `klama` was dropped entirely from the compiled FOL.
    let buf = local_gates("mi djica lo nu ganai la .adam. gerku gi la .adam. klama kei")
        .expect("should compile");
    assert!(
        has_predicate_base(&buf, "klama"),
        "the abstraction body's consequent `klama` was dropped. FOL: {:?}",
        buf.nodes
    );
}

#[test]
fn abstraction_body_over_rel_clause_keeps_head() {
    // Abstraction body `lo gerku poi ke'a barda cu klama`; head predicate `klama`
    // must survive (the bug bound the abstraction to the `barda` rel-clause bridi).
    let buf =
        local_gates("mi djica lo nu lo gerku poi ke'a barda cu klama kei").expect("should compile");
    assert!(
        has_predicate_base(&buf, "klama"),
        "the abstraction body head `klama` was dropped. FOL: {:?}",
        buf.nodes
    );
}

#[test]
fn rel_clause_on_name_keeps_both_conjuncts() {
    // `la .adam. poi gerku cu klama` must compile BOTH the restriction (`gerku`) and
    // the main claim (`klama`) — the `poi gerku` clause must not be silently dropped
    // (which produced an unsound TRUE for an unproven restriction).
    let buf = local_gates("la .adam. poi gerku cu klama").expect("should compile");
    assert!(
        has_predicate_base(&buf, "gerku") && has_predicate_base(&buf, "klama"),
        "the `poi gerku` restriction was dropped. FOL: {:?}",
        buf.nodes
    );
}

// ─── Zero-ingest connective assertions ──────────────────────────────────────

#[test]
fn disjunction_ija_compiles_both_operands() {
    // `.i ja` (or) must ingest BOTH operands — it once compiled/accepted but ingested
    // nothing. Assert a disjunction node with both predicates present.
    let buf = local_gates("la .adam. cu gerku .i ja la .adam. cu mlatu").expect("should compile");
    assert!(
        has_node(&buf, is_or),
        "`.i ja` did not compile to a disjunction"
    );
    assert!(
        has_predicate_base(&buf, "gerku") && has_predicate_base(&buf, "mlatu"),
        "`.i ja` dropped an operand. FOL: {:?}",
        buf.nodes
    );
}

#[test]
fn xor_iju_compiles_both_operands() {
    // `.i ju` (xor) flattens to `And(Or(a,b), Not(And(a,b)))` — both operands must
    // still appear (it once ingested nothing).
    let buf = local_gates("la .adam. cu gerku .i ju la .adam. cu mlatu").expect("should compile");
    assert!(
        has_node(&buf, is_or) && has_node(&buf, is_not),
        "`.i ju` did not compile to the xor shape"
    );
    assert!(
        has_predicate_base(&buf, "gerku") && has_predicate_base(&buf, "mlatu"),
        "`.i ju` dropped an operand. FOL: {:?}",
        buf.nodes
    );
}

// ─── Fail-open rule compilation (ganai antecedent must not be dropped) ───────

#[test]
fn ganai_tensed_antecedent_survives() {
    // `ganai la .adam. pu bajra gi la .adam. danlu` — the tensed antecedent must be
    // represented (the `_ => None` catch-all once dropped the tense atom, leaving the
    // rule with an empty/unconditional condition). Reject-or-represent: assert only
    // when it compiles.
    if let Ok(buf) = local_gates("ganai la .adam. pu bajra gi la .adam. danlu") {
        assert!(
            has_predicate_base(&buf, "bajra") && has_node(&buf, is_past),
            "the tensed antecedent (`pu bajra`) was dropped from the rule. FOL: {:?}",
            buf.nodes
        );
    }
}

#[test]
fn ganai_disjunctive_antecedent_survives() {
    // `ganai ga … gi … gi …` — the disjunctive antecedent must survive (both disjuncts
    // + an `Or`), not be dropped to an unconditional rule.
    if let Ok(buf) = local_gates("ganai ga la .adam. gerku gi la .adam. mlatu gi la .adam. danlu") {
        assert!(
            has_node(&buf, is_or)
                && has_predicate_base(&buf, "gerku")
                && has_predicate_base(&buf, "mlatu"),
            "the disjunctive antecedent was dropped from the rule. FOL: {:?}",
            buf.nodes
        );
    }
}

// ─── Negation compiles (the reasoning contradiction-check is out of scope) ──

#[test]
fn negated_ground_fact_compiles_to_not() {
    // The compile slice of the contradiction-detection case: `la .adam. na gerku` must
    // compile to a `Not` over `gerku` — the `na` must not be a silent no-op at compile
    // time. (Detecting the later contradiction is reasoning-engine work, not ported.)
    if let Ok(buf) = local_gates("la .adam. na gerku") {
        assert!(
            has_node(&buf, is_not) && has_predicate_base(&buf, "gerku"),
            "the negative fact `na gerku` did not compile to a Not. FOL: {:?}",
            buf.nodes
        );
    }
}

// ─── Semantic place-structure / firewall ────────────────────────────────────

#[test]
fn fa_tag_beyond_arity_errors() {
    // `fu` tags place x5; `gerku` is 2-place, so the x5 term `do` cannot be placed —
    // this must FAIL CLOSED with a semantic error, not silently drop the bound term.
    assert!(
        local_gates("fu do gerku").is_err(),
        "an over-arity FA tag (`fu` = x5 on 2-place `gerku`) must error, not silently drop"
    );
}

#[test]
fn tanru_in_poi_compiles() {
    // `lo gerku poi sutra bajra cu klama` — a valid tanru (`sutra bajra`) inside a poi
    // relative clause must be accepted (the ambiguity firewall once falsely rejected it).
    assert!(
        local_gates("lo gerku poi sutra bajra cu klama").is_ok(),
        "a valid tanru-in-poi relative clause was rejected"
    );
}

// ─── gerna lexer truncation ─────────────────────────────────────────────────

#[test]
fn lexer_truncation_surfaces_error() {
    // The bare `7` is unlexable mid-sentence; the input must NOT be silently truncated
    // (dropping the trailing sentence). The preferred fix is a positioned parse error;
    // if instead the input is accepted, the trailing `danlu` must have survived.
    let s = "mi klama 7 do prami .i lo gerku cu danlu";
    match gerna::parse_checked(s) {
        Err(_) => {} // positioned parse error — the preferred fix. Pass.
        Ok(ast) => {
            let buf = smuni::compile_from_gerna_ast(ast).expect("should compile if it parsed");
            assert!(
                has_predicate_base(&buf, "danlu"),
                "input was silently truncated at `7`; the trailing sentence vanished. FOL: {:?}",
                buf.nodes
            );
        }
    }
}

// ─── Quantifier-closure scoping ─────────────────────────────────────────────

#[test]
fn da_after_universal_scopes_exists_inside_forall() {
    // `ro lo gerku cu citka da` = "every dog eats something" = `∀x.(dog(x) → ∃y.eats(x,y))`.
    // The `da` existential must close INSIDE the `∀` (the bug closed it outside, `∃y.∀x`,
    // which drops the rule). Assert the compiled root is a `∀` whose body holds the `∃`.
    let buf = local_gates("ro lo gerku cu citka da").expect("should compile");
    assert!(
        matches!(buf.nodes[buf.roots[0] as usize], LogicNode::ForAllNode(_)),
        "`ro lo … da` root must be a universal (∀ outermost). FOL: {:?}",
        buf.nodes
    );
    assert!(
        exists_under_forall(&buf),
        "the `da` existential closed OUTSIDE the universal — rule scope lost. FOL: {:?}",
        buf.nodes
    );
}

// ─── Ch-12 consent case study: the COMPILE slice ────────────────────────────
// The nibli case pinned QUERY TIME (a ∃-heavy entailment search blowup) — reasoning
// work with no home here. Its compile slice is worth guarding: the three complex
// nested-abstraction sentences must all compile without error.

#[test]
fn ch12_consent_sentences_compile() {
    for s in [
        ".i lo prenu cu ponse lo datni",
        ".i lo prenu cu curmi lo nu lo datni cu se pilno",
        ".i ro lo prenu poi curmi lo nu lo datni cu se pilno cu se bilga lo nu lo datni cu se pilno",
    ] {
        assert!(
            local_gates(s).is_ok(),
            "Ch-12 consent sentence failed to compile: {s:?} → {:?}",
            local_gates(s).err()
        );
    }
}
