//! P1–P11: proofs that must verify.
//!
//! These are the vacuous half of the suite while `check()` is a stub. They earn
//! their place once the checker exists, as the control against over-strictness:
//! every rule in `docs/TDD.md` that rejects something is a rule that could
//! reject a valid proof instead.

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

/// P12. A tautological lemma with an **empty hint list**, which is the one
/// shape that reaches the tautology path of the RAT step.
///
/// `taut_lemma` carries a RUP hint, so it goes down `check_rup` and leaves
/// `check_rat`'s own tautology exit — accept before the pivot is read and
/// before the candidate scan — with no coverage at all. Measured: deleting the
/// `unwind` on that exit left all 77 tests green.
///
/// It is also the one construct that breaks
/// `candidate_scans == rat_additions + vacuous_rat_additions`, exactly as the
/// note on `Stats::candidate_scans` says it would, so this test asserts the
/// counters itself rather than going through `counters` below.
///
/// Built here rather than in `tools/mutate.py` for P8's reason: no solver
/// emits a tautological lemma, and backward checking would trim it if one did.
/// The rest of the proof is a real RUP derivation over a formula that is
/// unsatisfiable by propagation alone.
#[test]
fn p12_a_tautological_rat_lemma_is_accepted_and_unwound() {
    let formula = "p cnf 3 4\n1 2 0\n-2 0\n-1 3 0\n-3 0\n";
    // Lemma 5 is `1 or not-1` with no hints: RAT-shaped, so it is classified
    // as a vacuous RAT addition, and accepted as a tautology before the scan
    // the classification predicts. Steps 6 and 7 are ordinary RUP.
    let proof = "5 1 -1 0 0\n6 1 0 1 2 0\n7 0 6 3 4 0\n";
    let outcome = refute::check_readers(
        Cursor::new(formula.as_bytes()),
        Cursor::new(proof.as_bytes()),
        &Limits::default(),
    );
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert_eq!(outcome.stats.vacuous_rat_additions, 1);
    // The documented exception: accepted before the scan, so the scan never
    // happened. Every other fixture in this file asserts the equality.
    assert_eq!(outcome.stats.candidate_scans, 0);
    // The assertion this test exists for. The tautology assigned a literal and
    // returned early; leave it on the trail and the next step reads a hint
    // under an assignment it was never checked against.
    assert_eq!(
        outcome.stats.assignments, outcome.stats.assignments_undone,
        "the tautology's assignment was left on the trail"
    );
}

/// Every counter below was computed from the fixture bytes by a separate
/// script, before the checker existed, and matches the table in `docs/TDD.md`
/// part 2 that the design measured with a throwaway reference checker. They
/// are predictions confirmed, not readings taken.
///
/// The equality asserted here is the one that kills two mutants at once: a
/// checker that scans the database on every addition, and — the one that
/// matters — a checker that skips the scan on an addition with no hints, which
/// is the largest false-accept hole in this milestone.
///
/// The trail balance is asserted here too, so that every positive fixture pins
/// it rather than only `b13_large_formula_unwinds_in_proportion_to_
/// assignments`, whose proof is pure RUP. A RAT step unwinds on nine paths —
/// the tautology, the missing pivot, each rejection inside a block, between
/// blocks, and at the end — and B13 walks none of them. An unwind missed on
/// any of them leaks assignments from one step into the next, which is how a
/// hint gets classified against a trail it was never checked under.
fn counters(cnf: &str, proof: &str) -> refute::Stats {
    let stats = common::outcome(cnf, proof).stats;
    assert_eq!(
        stats.candidate_scans,
        stats
            .rat_additions
            .saturating_add(stats.vacuous_rat_additions),
        "the candidate scan does not happen once per RAT-shaped addition"
    );
    assert_eq!(
        stats.assignments, stats.assignments_undone,
        "the trail was not empty at the end of the run: {} assigned, {} undone",
        stats.assignments, stats.assignments_undone
    );
    stats
}

/// Asserted on every pure-RUP fixture: no RAT line, so no scan.
#[test]
fn the_rup_only_fixtures_never_scan_for_candidates() {
    for (cnf, proof) in [
        ("tiny_unsat.cnf", "tiny_unsat.lrat"),
        ("unit_chain.cnf", "unit_chain.lrat"),
        ("taut_lemma.cnf", "taut_lemma.lrat"),
        ("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.lrat"),
        ("deletes_originals.cnf", "deletes_originals.lrat"),
        ("random_unsat.cnf", "random_unsat.lrat"),
        ("dup_literal.cnf", "dup_literal.lrat"),
    ] {
        let stats = counters(cnf, proof);
        assert_eq!(stats.candidate_scans, 0, "{proof} scanned");
        assert_eq!(stats.resolvent_blocks, 0, "{proof} had a resolvent block");
    }
}

/// P9. The flip milestone 1b exists for.
///
/// `real_rat_proof` is pigeonhole 5 into 4, straight out of `kissat` and
/// `drat-trim -L`: 80 additions, 12 carrying resolvent blocks, 8 with no hints
/// at all. Milestone 1 stopped on proof line 2 of it and printed
/// `s UNSUPPORTED`, which is what every real proof got.
#[test]
fn p9_real_rat_proof_verifies() {
    assert_eq!(
        common::verdict("real_rat_proof.cnf", "real_rat_proof.lrat"),
        Verdict::Verified
    );
    common::cli("real_rat_proof.cnf", "real_rat_proof.lrat").assert("s VERIFIED", 0);

    let stats = counters("real_rat_proof.cnf", "real_rat_proof.lrat");
    assert_eq!(stats.additions, 80);
    assert_eq!(stats.rat_additions, 12);
    assert_eq!(stats.vacuous_rat_additions, 8);
    assert_eq!(stats.deletions, 61);
    assert_eq!(stats.hints_resolved, 286);
    assert_eq!(stats.candidate_scans, 20);
    assert_eq!(stats.candidates_examined, 886);
    assert_eq!(stats.resolvent_blocks, 24);
    // Exactly as many candidates as blocks: the file accounts for the set the
    // checker found, with nothing left over on either side.
    assert_eq!(stats.resolution_candidates, 24);
    assert_eq!(stats.peak_live_clauses, 48);
}

/// P10. The same construct at scale: pigeonhole 7 into 6. 624 additions, 42
/// RAT lines, 30 empty hint lists, 108 resolvent blocks, 353 deletions.
///
/// Here for the reason `random_unsat` is here. A checker that is subtly
/// over-strict about RAT passes 5x4 — 24 blocks over 20 candidate scans is not
/// enough proof to hang a verdict on.
#[test]
fn p10_rat_pigeonhole_verifies() {
    assert_eq!(
        common::verdict("rat_pigeonhole.cnf", "rat_pigeonhole.lrat"),
        Verdict::Verified
    );
    common::cli("rat_pigeonhole.cnf", "rat_pigeonhole.lrat").assert("s VERIFIED", 0);

    let stats = counters("rat_pigeonhole.cnf", "rat_pigeonhole.lrat");
    assert_eq!(stats.additions, 624);
    assert_eq!(stats.rat_additions, 42);
    assert_eq!(stats.vacuous_rat_additions, 30);
    assert_eq!(stats.deletions, 353);
    assert_eq!(stats.hints_resolved, 8755);
    assert_eq!(stats.candidate_scans, 72);
    assert_eq!(stats.candidates_examined, 8118);
    assert_eq!(stats.resolvent_blocks, 108);
    assert_eq!(stats.resolution_candidates, 108);
    assert_eq!(stats.peak_live_clauses, 137);
}

/// P11. The one proof whose resolvent block has to propagate.
///
/// Every one of the 703 blocks measured for `docs/TDD.md` part 2 is refuted by
/// the negation of its own resolvent and carries no hints, so the hint walk
/// inside a block has no coverage from any real file. This fixture is built so
/// that it does: three hints, with the conflict on the last.
#[test]
fn p11_resolvent_block_hints_propagate() {
    assert_eq!(
        common::verdict("resolvent_propagates.cnf", "resolvent_propagates.lrat"),
        Verdict::Verified
    );
    common::cli("resolvent_propagates.cnf", "resolvent_propagates.lrat").assert("s VERIFIED", 0);

    let stats = counters("resolvent_propagates.cnf", "resolvent_propagates.lrat");
    assert_eq!(stats.additions, 2);
    assert_eq!(stats.rat_additions, 1);
    assert_eq!(stats.vacuous_rat_additions, 0);
    assert_eq!(stats.candidate_scans, 1);
    assert_eq!(stats.candidates_examined, 6);
    assert_eq!(stats.resolvent_blocks, 1);
    assert_eq!(stats.resolution_candidates, 1);
    // Seven hint lookups: four on the RUP step that derives the empty clause,
    // and three inside the one block, whose prefix is empty. Those three are
    // the coverage no real proof provides.
    assert_eq!(stats.hints_resolved, 7);
}
