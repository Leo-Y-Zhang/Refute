//! P1–P5: proofs that must verify.
//!
//! These are the vacuous half of the suite while `check()` is a stub. They earn
//! their place once the checker exists, as the control against over-strictness:
//! every rule in `docs/TDD.md` that rejects something is a rule that could
//! reject a valid proof instead.

mod common;

use refute::Verdict;

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
