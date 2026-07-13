//! End-to-end (parse → compile) regression for the four GIhA shared-head corners.
//! The smuni unit tests (`giha_shared_head_*` in `smuni/src/semantic.rs`) pin each
//! corner precisely on a hand-built AST; these tests exercise the FULL gerna→smuni
//! pipeline, so a parser change that reshapes `ri'a mi` or a connected sumti can't
//! silently pass the unit tests while the real pipeline mis-compiles. Corner 4 (a
//! connected sumti in the shared head) is a gerna PARSE-TIME desugar into
//! `Connected(SharedHead, SharedHead)` and can ONLY be tested this way.

use fanva::gates::local_gates;
use nibli_types::logic::{LogicBuffer, LogicNode, LogicalTerm};

/// True if `base` or any of its Neo-Davidsonian role predicates (`base_xN`) appears.
fn has_pred_base(buf: &LogicBuffer, base: &str) -> bool {
    let role_prefix = format!("{base}_x");
    buf.nodes.iter().any(|n| {
        matches!(n, LogicNode::Predicate((rel, _)) if rel == base || rel.starts_with(&role_prefix))
    })
}

/// The variable names at arg position `pos` of every `role` predicate, in tree order.
fn role_vars_all(buf: &LogicBuffer, role: &str, pos: usize) -> Vec<String> {
    buf.nodes
        .iter()
        .filter_map(|n| match n {
            LogicNode::Predicate((rel, args)) if rel == role => match args.get(pos) {
                Some(LogicalTerm::Variable(v)) => Some(v.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Sorted, de-duplicated witness names at position `pos` of `role` (a set).
fn witness_set(buf: &LogicBuffer, role: &str, pos: usize) -> Vec<String> {
    let mut v = role_vars_all(buf, role, pos);
    v.sort();
    v.dedup();
    v
}

/// The single shared witness (asserts exactly one), read from `{base}_x1`.
fn the_one_witness(buf: &LogicBuffer, base: &str) -> String {
    let s = witness_set(buf, &format!("{base}_x1"), 1);
    assert_eq!(
        s.len(),
        1,
        "expected exactly one `{base}` witness, got {s:?}"
    );
    s.into_iter().next().unwrap()
}

// ─── Corner 4: connected sumti in the shared head (gerna desugar) ─────────────

#[test]
fn connected_head_binds_two_witnesses() {
    // `lo gerku .e lo mlatu cu klama gi'e citka` — the dogs-and-cats that go and eat:
    //   ∃g.(gerku(g) ∧ klama(g) ∧ citka(g)) ∧ ∃m.(mlatu(m) ∧ klama(m) ∧ citka(m)).
    // The head `.e` distributes the whole GIhA unit into two shared-head units.
    let buf = local_gates("lo gerku .e lo mlatu cu klama gi'e citka").expect("should compile");
    for base in ["gerku", "mlatu", "klama", "citka"] {
        assert!(
            has_pred_base(&buf, base),
            "missing `{base}`: {:?}",
            buf.nodes
        );
    }
    // The head `.e` joins the two operand units with And (a mis-lowering to gi'a / Or
    // would flip the root node).
    assert!(
        matches!(&buf.nodes[buf.roots[0] as usize], LogicNode::AndNode(_)),
        "head `.e` must join the operand units with And, got {:?}",
        buf.nodes[buf.roots[0] as usize]
    );
    // Each shared tail runs once per operand under a DISTINCT witness, and klama and
    // citka run under the SAME two witnesses (per-operand sharing): a disjoint
    // re-quantification of each operand's two tails would give citka a different pair.
    let klama = witness_set(&buf, "klama_x1", 1);
    let citka = witness_set(&buf, "citka_x1", 1);
    assert_eq!(klama.len(), 2, "two distinct klama witnesses: {klama:?}");
    assert_eq!(
        klama, citka,
        "klama and citka must run under the SAME two witnesses (per-operand sharing): \
         klama={klama:?} citka={citka:?}"
    );
}

#[test]
fn connected_head_reresolves_other_witness() {
    // `lo gerku fe mi .e do cu klama gi'e citka` — the connective is on the x2 filler
    // (`mi .e do`), but a sumti connective repeats the WHOLE bridi (CLL), so the head's
    // OTHER term `lo gerku` is re-resolved per operand: TWO distinct gerku witnesses,
    // not one shared dog. (Pins the CLL-faithful per-operand binding the desugar gives.)
    let buf = local_gates("lo gerku fe mi .e do cu klama gi'e citka").expect("should compile");
    assert_eq!(
        witness_set(&buf, "gerku_x1", 1).len(),
        2,
        "`lo gerku` must be re-resolved into two distinct witnesses: {:?}",
        role_vars_all(&buf, "gerku_x1", 1)
    );
}

#[test]
fn connected_head_and_modal_tail() {
    // Co-occurrence: connected sumti in the HEAD + a BAI modal in a TAIL
    // (`lo gerku .e lo mlatu cu klama gi'e bajra ri'a mi`). Two witnesses (head `.e`
    // distributes), and the tail modal `rinka` appears once PER operand (twice total).
    let buf =
        local_gates("lo gerku .e lo mlatu cu klama gi'e bajra ri'a mi").expect("should compile");
    assert_eq!(
        witness_set(&buf, "klama_x1", 1).len(),
        2,
        "two operand witnesses"
    );
    assert_eq!(
        role_vars_all(&buf, "rinka", 1).len(),
        2,
        "the tail modal runs once per distributed operand: {:?}",
        role_vars_all(&buf, "rinka", 1)
    );
}

// ─── Corners 1-3 end-to-end (also pinned precisely in smuni unit tests) ───────

#[test]
fn modal_in_head_shares_one_witness() {
    // Corner 1: `lo gerku ri'a mi cu klama gi'e bajra` → one shared gerku witness
    // carried by klama, bajra, AND the once-conjoined head modal `rinka`.
    let buf = local_gates("lo gerku ri'a mi cu klama gi'e bajra").expect("should compile");
    let w = the_one_witness(&buf, "gerku");
    for role in ["klama_x1", "bajra_x1"] {
        for var in role_vars_all(&buf, role, 1) {
            assert_eq!(var, w, "{role} must carry the shared witness");
        }
    }
    let rinka = role_vars_all(&buf, "rinka", 1); // rinka(mi, w) — x2 is the link
    assert_eq!(rinka.len(), 1, "head modal conjoined once: {rinka:?}");
    assert_eq!(
        rinka[0], w,
        "the head modal must link to the shared witness"
    );
}

#[test]
fn modal_in_tail_shares_one_witness() {
    // Corner 2: `lo gerku cu klama gi'e bajra ri'a mi` → one shared witness; the tail
    // modal appears once (in the bajra branch), linked to that witness.
    let buf = local_gates("lo gerku cu klama gi'e bajra ri'a mi").expect("should compile");
    let w = the_one_witness(&buf, "gerku");
    let rinka = role_vars_all(&buf, "rinka", 1);
    assert_eq!(rinka.len(), 1, "tail modal appears once: {rinka:?}");
    assert_eq!(rinka[0], w, "tail modal must link to the shared witness");
    for var in role_vars_all(&buf, "bajra_x1", 1) {
        assert_eq!(var, w);
    }
}

#[test]
fn connected_tail_distributes_under_one_witness() {
    // Corner 3: `lo gerku cu klama gi'e citka lo plise .e lo badna` → the citka tail
    // distributes into two, both carrying the ONE shared gerku witness in x1.
    let buf =
        local_gates("lo gerku cu klama gi'e citka lo plise .e lo badna").expect("should compile");
    let w = the_one_witness(&buf, "gerku");
    assert!(
        has_pred_base(&buf, "plise") && has_pred_base(&buf, "badna"),
        "both operands must survive"
    );
    let citka = role_vars_all(&buf, "citka_x1", 1);
    assert_eq!(
        citka.len(),
        2,
        "the tail must distribute to two citka: {citka:?}"
    );
    for var in &citka {
        assert_eq!(*var, w, "each distributed citka shares the head witness");
    }
}

#[test]
fn connected_under_head_modal_fails_closed() {
    // The ONE shape still unsupported (by design, sound): a connected sumti UNDER a BAI
    // modal in the shared head (`ri'a mi .e do`). It must fail closed, not mis-compile.
    assert!(
        local_gates("lo gerku ri'a mi .e do cu klama gi'e bajra").is_err(),
        "a connected sumti under a head BAI modal must fail closed"
    );
}
