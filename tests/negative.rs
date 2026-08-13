//! N1–N12: the twelve corruption controls.
//!
//! The gate for milestone 1 is that every one of these produces a non-zero exit
//! and never the string `s VERIFIED`. Each was written and run against a
//! `check()` that returned `Verified` unconditionally, and observed failing,
//! before any checking code existed. A rejection test that has never been seen
//! red proves nothing.
//!
//! Two things are asserted separately on purpose: that the proof was rejected,
//! and which reason it was rejected for. The first is the safety property; the
//! second is a tripwire, so that a rule silently changing which control catches
//! a mutation is visible in a diff.

// A test asserts by panicking: `unwrap` on a fixture that must open, `panic!`
// on a verdict that must not happen, indexing a slice an assertion above it
// just sized. The package's panic floor in Cargo.toml is there for the library
// and the binary, where a panic on input-derived data is a denial of service.
// Here it would only make the failure report worse.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use refute::{Reason, Unsupported, Verdict};

fn rejection_reason(cnf: &str, proof: &str) -> Reason {
    match common::verdict(cnf, proof) {
        Verdict::NotVerified(rejection) => rejection.reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// Asserts the safety property itself, independently of the reason: nothing on
/// stdout says VERIFIED, and the exit code is not 0.
fn assert_refused(cnf: &str, proof: &str) {
    let run = common::cli(cnf, proof);
    assert_ne!(run.code, 0, "exited 0 on {proof}; stdout {:?}", run.stdout);
    assert!(
        !run.stdout.contains("s VERIFIED"),
        "printed s VERIFIED on {proof}"
    );
}

/// N1. One hint redirected to a different clause that is live at that point.
/// Any of three rules may catch it; which one is recorded so a change is loud.
#[test]
fn n1_hint_redirected() {
    assert_refused("n01_hint_redirected.cnf", "n01_hint_redirected.lrat");
    let reason = rejection_reason("n01_hint_redirected.cnf", "n01_hint_redirected.lrat");
    assert!(
        matches!(
            reason,
            Reason::HintSatisfied(_) | Reason::HintNotUnit(_) | Reason::NoConflict
        ),
        "caught by an unexpected rule: {reason:?}"
    );
}

/// N2. The last hint of the final step removed, so the walk ends without a
/// conflict.
#[test]
fn n2_last_hint_dropped() {
    assert_refused("n02_last_hint_dropped.cnf", "n02_last_hint_dropped.lrat");
    assert_eq!(
        rejection_reason("n02_last_hint_dropped.cnf", "n02_last_hint_dropped.lrat"),
        Reason::NoConflict
    );
}

/// N3. A hint pointing at a clause deleted earlier in the proof. This is the
/// control that makes strict deletion handling worth having: Refute does not
/// copy `drat-trim`'s rule of ignoring deletions of unit clauses.
#[test]
fn n3_hint_names_deleted_clause() {
    assert_refused(
        "n03_hint_deleted_clause.cnf",
        "n03_hint_deleted_clause.lrat",
    );
    assert!(matches!(
        rejection_reason(
            "n03_hint_deleted_clause.cnf",
            "n03_hint_deleted_clause.lrat"
        ),
        Reason::MissingHint(_)
    ));
}

/// N4. One literal of a lemma flipped, so the lemma no longer follows.
#[test]
fn n4_lemma_literal_flipped() {
    assert_refused(
        "n04_lemma_literal_flipped.cnf",
        "n04_lemma_literal_flipped.lrat",
    );
    let reason = rejection_reason(
        "n04_lemma_literal_flipped.cnf",
        "n04_lemma_literal_flipped.lrat",
    );
    assert!(
        matches!(
            reason,
            Reason::NoConflict | Reason::HintSatisfied(_) | Reason::HintNotUnit(_)
        ),
        "caught by an unexpected rule: {reason:?}"
    );
}

/// N5. The final empty-clause line removed. Every step still checks; the proof
/// simply never reaches a contradiction, which is the failure mode a checker
/// that stops at "no errors" would miss.
#[test]
fn n5_no_empty_clause() {
    assert_refused("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat");
    assert_eq!(
        rejection_reason("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat"),
        Reason::NoEmptyClause
    );
}

/// N6. The proof truncated to its first half, as a killed job would leave it.
#[test]
fn n6_truncated_proof() {
    assert_refused("n06_truncated.cnf", "n06_truncated.lrat");
    assert_eq!(
        rejection_reason("n06_truncated.cnf", "n06_truncated.lrat"),
        Reason::NoEmptyClause
    );
}

/// N7. The hint list of one step reversed. Order-insensitive checking would
/// accept this; the strict walk in `docs/TDD.md` exists to catch it.
#[test]
fn n7_hints_reversed() {
    assert_refused("n07_hints_reversed.cnf", "n07_hints_reversed.lrat");
    let reason = rejection_reason("n07_hints_reversed.cnf", "n07_hints_reversed.lrat");
    assert!(
        matches!(
            reason,
            Reason::EarlyConflict(_) | Reason::HintNotUnit(_) | Reason::HintSatisfied(_)
        ),
        "caught by an unexpected rule: {reason:?}"
    );
}

/// N8. A deletion line moved to before the step that uses the clause.
#[test]
fn n8_deletion_moved_early() {
    assert_refused(
        "n08_deletion_moved_early.cnf",
        "n08_deletion_moved_early.lrat",
    );
    assert!(matches!(
        rejection_reason(
            "n08_deletion_moved_early.cnf",
            "n08_deletion_moved_early.lrat"
        ),
        Reason::MissingHint(_)
    ));
}

/// N9. A valid proof checked against a different formula: two clauses
/// transposed, so every hint still resolves but resolves to the wrong clause.
#[test]
fn n9_different_formula() {
    assert_refused("n09_different_formula.cnf", "n09_different_formula.lrat");
}

/// N10. A valid proof checked against a **satisfiable** formula.
///
/// The control that matters most. A pipeline that passes here certifies a false
/// upper bound: it reports that no colouring exists for an n where one does.
/// The formula was found by flipping one literal at a time until `kissat`
/// returned SAT, so its satisfiability is a solver's claim, not an assumption.
#[test]
fn n10_satisfiable_formula_must_be_refused() {
    assert_refused(
        "n10_satisfiable_formula.cnf",
        "n10_satisfiable_formula.lrat",
    );
    assert_ne!(
        common::verdict(
            "n10_satisfiable_formula.cnf",
            "n10_satisfiable_formula.lrat"
        ),
        Verdict::Verified
    );
}

/// N11. Two step ids transposed, so the sequence stops increasing.
#[test]
fn n11_non_monotonic_ids() {
    assert_refused("n11_non_monotonic_ids.cnf", "n11_non_monotonic_ids.lrat");
    assert!(matches!(
        rejection_reason("n11_non_monotonic_ids.cnf", "n11_non_monotonic_ids.lrat"),
        Reason::NonMonotonicId { .. }
    ));
}

/// N12. A proof that is a bare empty clause with no hints.
///
/// The single line that forced a three-way verdict. Treating "no hints" as
/// acceptance accepts this, which is a checker that accepts anything. It must
/// be unsupported, exit 2, and — asserted explicitly — never exit 0.
#[test]
fn n12_bare_empty_clause_is_unsupported_not_verified() {
    let run = common::cli("n12_bare_empty_clause.cnf", "n12_bare_empty_clause.lrat");
    assert_ne!(run.code, 0, "a bare empty clause exited 0");
    run.assert("s UNSUPPORTED", 2);
    assert!(matches!(
        common::verdict("n12_bare_empty_clause.cnf", "n12_bare_empty_clause.lrat"),
        Verdict::Unsupported(Unsupported::EmptyHints { .. })
    ));
}
