//! P1–P5: proofs that must verify.
//!
//! These are the vacuous half of the suite while `check()` is a stub. They earn
//! their place once the checker exists, as the control against over-strictness:
//! every rule in `docs/TDD.md` that rejects something is a rule that could
//! reject a valid proof instead.

mod common;

use std::io::Cursor;

use refute::{Limits, Verdict};

/// P1. The end-to-end happy path: real `kissat` output, real `drat-trim -L`
/// output, 3 variables, 8 clauses, small enough to check by eye.
#[test]
fn p1_tiny_unsat_verifies() {
    assert_eq!(
        common::verdict("tiny_unsat.cnf", "tiny_unsat.lrat"),
        Verdict::Verified
    );
    common::cli("tiny_unsat.cnf", "tiny_unsat.lrat").assert("s VERIFIED", 0);
}

/// P2. Eleven unit lemmas and an empty clause: the trail is pushed and unwound
/// once per step rather than once for the whole proof.
#[test]
fn p2_unit_chain_verifies() {
    assert_eq!(
        common::verdict("unit_chain.cnf", "unit_chain.lrat"),
        Verdict::Verified
    );
    common::cli("unit_chain.cnf", "unit_chain.lrat").assert("s VERIFIED", 0);
}

/// P3. Locks in the one permissive rule on additions. Adding `x or not-x`
/// preserves satisfiability and can never be the empty clause, so accepting it
/// is sound; rejecting it would be a false rejection with no safety benefit.
#[test]
fn p3_tautological_lemma_is_accepted() {
    assert_eq!(
        common::verdict("taut_lemma.cnf", "taut_lemma.lrat"),
        Verdict::Verified
    );
    common::cli("taut_lemma.cnf", "taut_lemma.lrat").assert("s VERIFIED", 0);
}

/// P4. The formula already contains the empty clause, so the proof is one line.
#[test]
fn p4_empty_clause_in_cnf_verifies() {
    assert_eq!(
        common::verdict("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.lrat"),
        Verdict::Verified
    );
    common::cli("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.lrat").assert("s VERIFIED", 0);
}

/// P5. 44 real additions and 43 real deletions, including the `22 d 0` empty
/// deletion line that occurs once in every `drat-trim -L` file measured.
#[test]
fn p5_deletes_originals_verifies() {
    assert_eq!(
        common::verdict("deletes_originals.cnf", "deletes_originals.lrat"),
        Verdict::Verified
    );
    common::cli("deletes_originals.cnf", "deletes_originals.lrat").assert("s VERIFIED", 0);
}

/// P6. A real random-3-SAT refutation: 980 RUP lemmas, no unsupported
/// construct anywhere in the file.
///
/// Added beyond the five in `docs/TDD.md`. Every other positive fixture is tens
/// of steps, and a checker that is subtly over-strict passes all of them. This
/// is the one that would break first, and `drat-trim` verifies the same
/// artefact, so a disagreement here is a disagreement between two independent
/// implementations rather than a test of Refute against itself.
#[test]
fn p6_random_unsat_verifies() {
    assert_eq!(
        common::verdict("random_unsat.cnf", "random_unsat.lrat"),
        Verdict::Verified
    );
    common::cli("random_unsat.cnf", "random_unsat.lrat").assert("s VERIFIED", 0);
}

/// P7. A formula clause with a literal written twice, and its real proof.
///
/// `1 2 -3 -3` is the clause `1 2 -3`. The proof is `tiny_unsat`'s, unchanged,
/// and it propagates `-3` from exactly that clause: a checker counting free
/// literals rather than distinct ones sees two and calls the hint non-unit.
/// `drat-trim` verifies the same lemma sequence against this formula during
/// fixture generation, so the disagreement would be Refute's alone.
#[test]
fn p7_repeated_literal_in_a_formula_clause_verifies() {
    assert_eq!(
        common::verdict("dup_literal.cnf", "dup_literal.lrat"),
        Verdict::Verified
    );
    common::cli("dup_literal.cnf", "dup_literal.lrat").assert("s VERIFIED", 0);
}

/// P8. The same defect one step further on: a *lemma* with a repeated literal,
/// used as a hint by a later step.
///
/// Built here rather than in `tools/mutate.py` because there is no provenance
/// to record — no solver emits such a lemma and `drat-trim`'s LRAT would not
/// preserve it if one did. What can be borrowed is the verdict: `drat-trim`
/// verifies the same two lemmas, `1 1 0` and `0`, against this formula.
#[test]
fn p8_repeated_literal_in_a_lemma_verifies() {
    let formula = "p cnf 3 4\n1 2 0\n-2 0\n-1 3 0\n-3 0\n";
    // Lemma 5 is the unit clause (1) with its literal written twice; step 6
    // then uses it as the unit hint that starts the final propagation.
    let proof = "5 1 1 0 1 2 0\n6 0 5 3 4 0\n";
    let outcome = refute::check_readers(
        Cursor::new(formula.as_bytes()),
        Cursor::new(proof.as_bytes()),
        &Limits::default(),
    );
    assert_eq!(outcome.verdict, Verdict::Verified);
}
