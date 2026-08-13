//! N1–N12 and R1–R8: the corruption controls.
//!
//! The R series is milestone 1b's, one per new rejection rule. Each was run
//! against the milestone-1 build first, where it reported `s UNSUPPORTED` for
//! a construct that now has to be rejected outright, and each was observed
//! failing there before its rule existed.
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

use refute::{Lit, Reason, Rejection, Verdict};

fn rejection(cnf: &str, proof: &str) -> Rejection {
    match common::verdict(cnf, proof) {
        Verdict::NotVerified(rejection) => rejection,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn rejection_reason(cnf: &str, proof: &str) -> Reason {
    rejection(cnf, proof).reason
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

/// N12, now R8's first half. A proof that is a bare empty clause with no hints.
///
/// The single line that forced a three-way verdict in milestone 1, where it
/// was `Unsupported(EmptyHints)` and exit 2. An empty hint list is now checked
/// rather than declined, and this one cannot be: an empty lemma has no first
/// literal, so it has no pivot, so the RAT predicate cannot be evaluated at
/// all. Fail closed — a rejection, exit 1, and never exit 0.
#[test]
fn n12_bare_empty_clause_is_rejected() {
    let run = common::cli("n12_bare_empty_clause.cnf", "n12_bare_empty_clause.lrat");
    assert_ne!(run.code, 0, "a bare empty clause exited 0");
    run.assert("s NOT VERIFIED", 1);
    assert!(matches!(
        common::verdict("n12_bare_empty_clause.cnf", "n12_bare_empty_clause.lrat"),
        Verdict::NotVerified(_)
    ));
}

/// Asserts a rejection, without naming the rule. The safety property on its
/// own: not verified, not unsupported, not a pass.
fn assert_rejected(cnf: &str, proof: &str) {
    assert_refused(cnf, proof);
    let verdict = common::verdict(cnf, proof);
    assert!(
        matches!(verdict, Verdict::NotVerified(_)),
        "expected a rejection for {proof}, got {verdict:?}"
    );
}

/// Asserts the rule that caught it, and where.
///
/// The reason is the tripwire, not the safety property, and on the R series it
/// earns its keep twice over. Relaxing the rule that a block must name an
/// uncovered candidate — letting it name any live clause — leaves every one of
/// these fixtures rejected, by a different rule, and only this assertion
/// notices: measured, by making that change and running the suite.
fn assert_caught_by(cnf: &str, proof: &str, step: u64, line: u64, resolvent: u64, reason: &Reason) {
    assert_rejected(cnf, proof);
    let rejection = rejection(cnf, proof);
    assert_eq!(&rejection.reason, reason, "caught by a different rule");
    assert_eq!(rejection.step, Some(step), "the wrong step");
    assert_eq!(rejection.line, line, "the wrong proof line");
    assert_eq!(
        rejection.resolvent,
        Some(resolvent),
        "the wrong resolvent block"
    );
}

/// The same, for a rejection reached before any block was being checked, where
/// `Rejection::resolvent` is `None` rather than a block identifier.
fn assert_caught_before_the_blocks(cnf: &str, proof: &str, step: u64, line: u64, reason: &Reason) {
    assert_rejected(cnf, proof);
    let rejection = rejection(cnf, proof);
    assert_eq!(&rejection.reason, reason, "caught by a different rule");
    assert_eq!(rejection.step, Some(step), "the wrong step");
    assert_eq!(rejection.line, line, "the wrong proof line");
    assert_eq!(
        rejection.resolvent, None,
        "no block was being checked when this was rejected"
    );
}

/// R1. The wrong pivot: the first two literals of a RAT lemma swapped.
///
/// The pivot is the lemma's first literal *as written in the file*, and
/// `normalize` sorts on the way into the database — so a checker reading the
/// pivot after normalisation scans for the wrong literal and rejects the
/// smallest real RAT proof there is. This is the mutation that says which of
/// the two it did.
#[test]
fn r1_wrong_pivot() {
    assert_caught_by(
        "r01_wrong_pivot.cnf",
        "r01_wrong_pivot.lrat",
        48,
        4,
        46,
        &Reason::NotAResolutionCandidate {
            pivot: Lit::new(-5).unwrap(),
        },
    );
}

/// R2. The last resolvent block, and its hints, deleted.
///
/// The load-bearing control of this milestone. Every resolvent block in every
/// real proof is refuted by the negation of its own resolvent, so a checker
/// that skipped candidates whose resolvent is trivially refuted would accept
/// the deletion of *any* real block, and this mutation would be undetectable.
#[test]
fn r2_resolvent_block_dropped() {
    assert_caught_by(
        "r02_block_dropped.cnf",
        "r02_block_dropped.lrat",
        48,
        4,
        47,
        &Reason::MissingResolvent {
            pivot: Lit::new(-21).unwrap(),
        },
    );
}

/// R3. A resolvent block redirected to a clause deleted earlier in the proof.
///
/// Sound to skip — a step checked against a smaller formula still refutes the
/// larger one — but a producer naming a clause that is not there is not
/// producing this proof.
#[test]
fn r3_block_names_a_deleted_clause() {
    assert_caught_by(
        "r03_block_names_deleted_clause.cnf",
        "r03_block_names_deleted_clause.lrat",
        49,
        6,
        6,
        &Reason::NotAResolutionCandidate {
            pivot: Lit::new(-21).unwrap(),
        },
    );
}

/// R4. A resolvent block's last hint redirected, on the one fixture whose
/// block hints are ever walked.
#[test]
fn r4_block_hint_redirected() {
    assert_caught_by(
        "r04_block_hint_redirected.cnf",
        "r04_block_hint_redirected.lrat",
        7,
        1,
        1,
        &Reason::HintSatisfied(5),
    );
}

/// R4b. The same block's conflict hint dropped, so its walk runs out.
#[test]
fn r4b_block_conflict_hint_dropped() {
    assert_caught_by(
        "r04b_block_conflict_hint_dropped.cnf",
        "r04b_block_conflict_hint_dropped.lrat",
        7,
        1,
        1,
        &Reason::NoConflict,
    );
}

/// R5. An empty-hint lemma reordered so that its pivot does have resolution
/// candidates.
///
/// The one to read first. An empty hint list is a claim — "this pivot has no
/// resolution candidate" — and a checker that takes it at face value passes
/// every other test in this suite while accepting any clause in the world.
/// The candidate set is computed by the checker from its own database, so the
/// claim is checked rather than believed.
#[test]
fn r5_empty_hints_with_candidates() {
    assert_caught_by(
        "r05_empty_hints_with_candidates.cnf",
        "r05_empty_hints_with_candidates.lrat",
        46,
        2,
        3,
        &Reason::MissingResolvent {
            pivot: Lit::new(-9).unwrap(),
        },
    );
}

/// R6. An extra resolvent block naming a live clause that is not a candidate.
#[test]
fn r6_extra_block() {
    assert_caught_by(
        "r06_extra_block.cnf",
        "r06_extra_block.lrat",
        48,
        4,
        1,
        &Reason::NotAResolutionCandidate {
            pivot: Lit::new(-21).unwrap(),
        },
    );
}

/// R7. A hint appended to a block whose resolvent its own negation already
/// refutes, so the hint can never be reached. Padding, and real output never
/// does it — the same argument as `EarlyConflict` in milestone 1.
#[test]
fn r7_padded_block() {
    assert_caught_by(
        "r07_padded_block.cnf",
        "r07_padded_block.lrat",
        48,
        4,
        47,
        &Reason::ResolventFalsifiedEarly,
    );
}

/// R8. An empty lemma with a RAT-shaped hint list, `46 0 -1 0`.
///
/// The other half is `n12_bare_empty_clause` above. Both are the same rule:
/// no literals means no pivot means no predicate to evaluate.
#[test]
fn r8_rat_without_pivot() {
    assert_rejected("r08_rat_without_pivot.cnf", "r08_rat_without_pivot.lrat");
    assert_eq!(
        rejection_reason("r08_rat_without_pivot.cnf", "r08_rat_without_pivot.lrat"),
        Reason::RatWithoutPivot
    );
    assert_eq!(
        rejection_reason("n12_bare_empty_clause.cnf", "n12_bare_empty_clause.lrat"),
        Reason::RatWithoutPivot
    );
}

/// R9. One lemma with two resolvent blocks, where the second is only refuted
/// once the first block's propagations have been taken back.
///
/// Hand-built, because no mutation of a real proof reaches it: every block in
/// every real file is refuted by the negation of its own resolvent and
/// propagates nothing at all, so no real block leaves a trail for the next one
/// to inherit. Here the first block propagates `-2` and conflicts; leave that
/// in place and the second block's resolvent looks already refuted, the lemma
/// is accepted, the empty clause follows from it, and `s VERIFIED` is printed
/// for a formula `kissat` reports SATISFIABLE. Its satisfiability is checked
/// by `kissat` during fixture generation for that reason.
///
/// Written after the code rather than before it, and justified by a recorded
/// mutation kill instead: deleting the `unwind(base)` between blocks makes
/// this test, and only this test, fail.
#[test]
fn r09_second_block_needs_its_own_trail() {
    assert_caught_by(
        "r09_second_block_needs_its_own_trail.cnf",
        "r09_second_block_needs_its_own_trail.lrat",
        4,
        1,
        2,
        &Reason::NoConflict,
    );
}

/// R10. A lemma written `-2 -2`, with an empty hint list.
///
/// The negative half of the duplicate-literal coverage. P8 and B19 pin that a
/// repeated literal is *accepted*; nothing pinned what it must not be read as.
/// A repeat is idempotent — `-2 -2` is the clause `-2`, and the second copy is
/// falsified by the first, not satisfied by it — but a checker that reads it as
/// `x or not-x` has a tautology, and a tautology is accepted before the
/// candidate scan runs. Both clauses of this formula hold `2`, so that scan is
/// the only thing between this file and `s VERIFIED` on a formula `kissat`
/// reports SATISFIABLE.
#[test]
fn r10_repeated_literal_is_not_a_tautology() {
    assert_caught_by(
        "r10_repeated_literal_is_not_a_tautology.cnf",
        "r10_repeated_literal_is_not_a_tautology.lrat",
        3,
        1,
        1,
        &Reason::MissingResolvent {
            pivot: Lit::new(-2).unwrap(),
        },
    );
}

/// R11. A RAT-shaped line whose hint prefix already reaches a conflict.
///
/// **This one is not a safety property, and should not be read as one.** The
/// lemma is RUP, the proof is valid, `kissat` reports the formula
/// UNSATISFIABLE and `drat-trim` verifies the same lemma sequence in DRAT form
/// during fixture generation. Relaxing this rule would accept a good proof,
/// not a bad one — it is `docs/TDD.md` part 2's open question 2, and the
/// strictness is inherited from milestone 1's `EarlyConflict`.
///
/// It is here because `Reason::RatLemmaIsRup` had no test at all, and a
/// strictness rule that no test can reach is what got `Reason::DuplicateId`
/// deleted in milestone 1. So this is a tripwire on a deliberate change: if it
/// fails, the question is whether the rule was relaxed on purpose, and if it
/// was, this test and the reason code go together.
#[test]
fn r11_rat_lemma_that_is_already_rup() {
    assert_caught_before_the_blocks(
        "r11_rat_lemma_that_is_already_rup.cnf",
        "r11_rat_lemma_that_is_already_rup.lrat",
        5,
        1,
        &Reason::RatLemmaIsRup(2),
    );
}
