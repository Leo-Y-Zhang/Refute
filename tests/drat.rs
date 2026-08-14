//! P13–P18, D1–D10 and B23–B35: the direct DRAT path.
//!
//! Milestone 1 and 1b check the file `drat-trim` writes. These check the file
//! the solver writes, so that `drat-trim` is in the chain neither as checker
//! nor as producer. Every fixture here came out of `kissat --no-binary`, or out
//! of `tools/mutate.py` operating on something that did.
//!
//! The two halves of the suite are asserted differently on purpose. A positive
//! demands `Verified` and its exact counters. A negative demands its exact
//! reason, never merely "not verified": a rejection for the wrong reason is a
//! test that would go on passing after the rule it was written for was deleted.

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

use refute::{Limits, Reason, Verdict};

/// Every name that carries both a `.lrat` and a `.drat`.
const BOTH_FORMATS: [&str; 5] = [
    "tiny_unsat",
    "deletes_originals",
    "real_rat_proof",
    "rat_pigeonhole",
    "empty_clause_in_cnf",
];

/// Checks text held here rather than in a fixture.
///
/// A fixture earns its place by carrying provenance — a solver ran, and these
/// bytes came out. Three lines written to exercise one boundary have none to
/// carry, and `tests/boundary.rs` draws the line in the same place.
fn inline(cnf: &str, proof: &str) -> refute::Outcome {
    inline_with(cnf, proof, &Limits::default())
}

fn inline_with(cnf: &str, proof: &str, limits: &Limits) -> refute::Outcome {
    refute::check_readers(
        Cursor::new(cnf.as_bytes()),
        Cursor::new(proof.as_bytes()),
        limits,
    )
}

fn rejection(verdict: Verdict) -> refute::Rejection {
    match verdict {
        Verdict::NotVerified(rejection) => rejection,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// The proof was read, a named candidate's resolvent did not follow, and the
/// message says which and on what pivot.
///
/// Written first as "rejected, and not a parse error", which is all the
/// milestone-1b binary could be held to: every DRAT file was `s NOT VERIFIED`
/// there because the LRAT reader could not read it, and a test that asserted
/// only the verdict would have been green before the checker existed and green
/// after it was deleted. The exact pivot and candidate are recorded here now
/// that there is a checker to produce them.
///
/// All eight mutants fail in the RAT candidate loop rather than on RUP, which
/// is worth knowing and was not obvious: dropping or corrupting a lemma
/// usually leaves a *later* lemma still derivable but no longer by propagation
/// alone, so the failure surfaces a step or two past the mutation.
fn assert_rat_check_failed(cnf: &str, proof: &str, pivot: i32, candidate: u64) {
    let rejection = rejection(common::verdict(cnf, proof));
    assert_eq!(
        rejection.reason,
        Reason::RatCheckFailed {
            pivot: refute::Lit::new(pivot).expect("a non-zero pivot")
        },
        "{proof}"
    );
    assert_eq!(rejection.resolvent, Some(candidate), "{proof}: candidate");
    let run = common::cli(cnf, proof);
    run.assert("s NOT VERIFIED", 1);
    assert!(
        run.stderr.contains(&format!("pivot {pivot}"))
            && run.stderr.contains(&format!("{candidate}")),
        "the message names neither the pivot nor the candidate: {:?}",
        run.stderr
    );
}

/// The two invariants every completed run satisfies, whatever it decided.
///
/// The trail is emptied after every step, so a run that assigned more than it
/// undid left something behind — which is the milestone-1b hole, in the shape
/// it would take here. The second is the classification identity: every
/// addition is counted exactly once, so a checker that skips the classification
/// on some path is visible in the arithmetic rather than only in a verdict.
fn assert_run_invariants(stats: &refute::Stats, what: &str) {
    assert_eq!(
        stats.assignments, stats.assignments_undone,
        "{what}: {} assignments, {} undone",
        stats.assignments, stats.assignments_undone
    );
    let classified = stats
        .rup_additions
        .saturating_add(stats.rat_additions)
        .saturating_add(stats.tautological_additions);
    assert_eq!(
        classified, stats.additions,
        "{what}: {classified} classified additions against {} checked",
        stats.additions
    );
}

// ------------------------------------------------------------------ positive

/// P13. The end-to-end happy path on raw solver output.
#[test]
fn p13_tiny_unsat_drat_verifies() {
    let outcome = common::outcome("tiny_unsat.cnf", "tiny_unsat.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert_run_invariants(&outcome.stats, "tiny_unsat.drat");
    common::cli("tiny_unsat.cnf", "tiny_unsat.drat").assert("s VERIFIED", 0);
}

/// P14. The smallest real proof carrying RAT, with its counters.
///
/// The numbers are the design's, measured with a throwaway reference checker
/// over this exact file and written into `docs/TDD.md` part 3 before any of
/// this existed. Asserting them here is what makes them a claim this checker
/// has to meet rather than a description of what it happens to do.
#[test]
fn p14_real_rat_proof_drat_verifies() {
    let outcome = common::outcome("real_rat_proof.cnf", "real_rat_proof.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    let stats = outcome.stats;
    assert_eq!(stats.additions, 91, "additions");
    assert_eq!(stats.deletions, 75, "deletions");
    assert_eq!(stats.unknown_deletions, 0, "unknown deletions");
    assert_eq!(stats.peak_live_clauses, 61, "peak live clauses");
    assert_eq!(stats.rup_additions, 71, "RUP additions");
    assert_eq!(stats.rat_additions, 20, "RAT additions");
    assert_eq!(stats.rat_candidates_checked, 24, "candidates checked");
    assert_run_invariants(&stats, "real_rat_proof.drat");
    common::cli("real_rat_proof.cnf", "real_rat_proof.drat").assert("s VERIFIED", 0);
}

/// P15. The same construct at a scale a subtly over-strict checker fails.
#[test]
fn p15_rat_pigeonhole_drat_verifies() {
    let outcome = common::outcome("rat_pigeonhole.cnf", "rat_pigeonhole.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    let stats = outcome.stats;
    assert_eq!(stats.additions, 702, "additions");
    assert_eq!(stats.deletions, 487, "deletions");
    assert_eq!(stats.peak_live_clauses, 348, "peak live clauses");
    assert_eq!(stats.rup_additions, 630, "RUP additions");
    assert_eq!(stats.rat_additions, 72, "RAT additions");
    assert_eq!(stats.rat_candidates_checked, 108, "candidates checked");
    assert_run_invariants(&stats, "rat_pigeonhole.drat");
}

/// P16. Every formula that has both proofs verifies under both.
///
/// The whole reason the raw proofs are committed beside the trimmed ones. Two
/// independently written readers and two independently written checkers reach
/// the same verdict on the same formula, in CI, on committed bytes, with
/// neither `kissat` nor `drat-trim` installed.
#[test]
fn p16_both_checkers_agree_on_every_name_that_has_both() {
    for name in BOTH_FORMATS {
        let lrat = common::outcome(&format!("{name}.cnf"), &format!("{name}.lrat"));
        let drat = common::outcome(&format!("{name}.cnf"), &format!("{name}.drat"));
        assert_eq!(lrat.verdict, Verdict::Verified, "{name}.lrat");
        assert_eq!(drat.verdict, Verdict::Verified, "{name}.drat");
        assert_run_invariants(&drat.stats, name);
    }
}

/// P17. Deletion of original clauses, on the DRAT path.
#[test]
fn p17_deletes_originals_drat_verifies() {
    let outcome = common::outcome("deletes_originals.cnf", "deletes_originals.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert!(outcome.stats.deletions > 0, "the proof deleted nothing");
    assert_run_invariants(&outcome.stats, "deletes_originals.drat");
}

/// P18. A formula that already holds the empty clause, and a two-byte proof.
#[test]
fn p18_empty_clause_in_cnf_drat_verifies() {
    let outcome = common::outcome("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert_eq!(
        outcome.stats.additions, 1,
        "one step, and it is the empty clause"
    );
    common::cli("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.drat").assert("s VERIFIED", 0);
}

// ------------------------------------------------------------------ negative

/// D1. The first addition dropped. Every later step written against it loses
/// its reason.
#[test]
fn d1_addition_dropped_is_rejected() {
    assert_rat_check_failed("real_rat_proof.cnf", "d01_addition_dropped.drat", 21, 79);
}

/// D2. The last load-bearing addition dropped, so the rejection happens
/// against a database the whole proof has been adding to and deleting from.
///
/// The message is asserted, not just the verdict: this is the one negative
/// whose expected failure is the RAT candidate loop itself, and a rejection
/// from anywhere else would be the right answer for the wrong reason.
#[test]
fn d2_needed_addition_dropped_fails_a_rat_candidate() {
    assert_rat_check_failed(
        "real_rat_proof.cnf",
        "d02_needed_addition_dropped.drat",
        -11,
        83,
    );
}

/// D3. One literal of one addition flipped — searched for, because most flips
/// leave a valid proof.
#[test]
fn d3_literal_flipped_is_rejected() {
    assert_rat_check_failed("real_rat_proof.cnf", "d03_literal_flipped.drat", 21, 46);
}

/// D4. Two adjacent additions transposed.
#[test]
fn d4_additions_swapped_is_rejected() {
    assert_rat_check_failed("real_rat_proof.cnf", "d04_additions_swapped.drat", -21, 46);
}

/// D5. A lemma deleted on the line after it was added, and used later.
///
/// The control on where the candidate set comes from: a checker that
/// enumerates deleted clauses, or that treats deletion as advisory, verifies
/// this.
#[test]
fn d5_deleted_then_used_is_rejected() {
    assert_rat_check_failed("real_rat_proof.cnf", "d05_deleted_then_used.drat", 21, 80);
}

/// D6. Truncated. Nothing was derived, so rejection is a theorem.
#[test]
fn d6_truncated_has_no_empty_clause() {
    let verdict = common::verdict("real_rat_proof.cnf", "d06_truncated.drat");
    assert_eq!(rejection(verdict).reason, Reason::NoEmptyClause);
    common::cli("real_rat_proof.cnf", "d06_truncated.drat").assert("s NOT VERIFIED", 1);
}

/// D7. The final `0` removed — and the one place Refute is knowingly stricter
/// than `drat-trim -f`, which reports `s VERIFIED` on this file because it
/// adds the empty clause itself once the formula propagates to a conflict.
///
/// Nothing in the file derived it. A checker whose success condition is "the
/// formula went quiet" is a checker that never read the proof.
#[test]
fn d7_no_empty_clause_is_rejected() {
    let verdict = common::verdict("real_rat_proof.cnf", "d07_no_empty_clause.drat");
    assert_eq!(rejection(verdict).reason, Reason::NoEmptyClause);
}

/// D8. A valid proof against a **satisfiable** formula.
///
/// The control that matters most. A formula with a model has no refutation, so
/// `s VERIFIED` here is a defect whatever any other checker says, and a
/// pipeline that passes it certifies a false upper bound.
#[test]
fn d8_satisfiable_formula_is_rejected() {
    assert_rat_check_failed(
        "d08_satisfiable_formula.cnf",
        "d08_satisfiable_formula.drat",
        -9,
        3,
    );
}

/// D9. The trail must be taken back between candidates.
///
/// Milestone 1b shipped this rule with no test on it, and deleting the line
/// left 77 tests green while the checker printed `s VERIFIED` on a formula
/// `kissat` reports satisfiable. Here there is no file to disagree with, so
/// this fixture is the only thing standing between that bug and a false
/// refutation. It also fails a checker that stops at the first candidate that
/// passes: the first candidate here does pass.
#[test]
fn d9_trail_leak_between_candidates_is_rejected() {
    // Candidate 2, not candidate 1: the first candidate passes, and a checker
    // that stopped there, or that left its propagations on the trail, never
    // reaches the one that fails.
    assert_rat_check_failed(
        "d09_trail_leak_between_candidates.cnf",
        "d09_trail_leak_between_candidates.drat",
        1,
        2,
    );
}

/// D10. Deletion by literals removes exactly one copy.
///
/// A store keyed by literal set cannot hold two copies at all, and one that
/// removes every identifier under the key loses both. Either way the surviving
/// candidate disappears and this satisfiable formula is refuted.
#[test]
fn d10_duplicate_clause_deleted_once_is_rejected() {
    // The surviving copy is candidate 2. A store that removed both, or one
    // keyed by literal set that never held two, has no candidate 2 at all.
    assert_rat_check_failed(
        "d10_duplicate_clause_deleted_once.cnf",
        "d10_duplicate_clause_deleted_once.drat",
        1,
        2,
    );
}

// ------------------------------------------------------------------ boundary

/// B23. An empty proof file, auto-detected. Identical to B1, and it says so.
///
/// With no first line there is nothing to classify, so the format is
/// unobservable — and both readings agree, because neither derived the empty
/// clause. Green before this milestone as well as after it, which is the point
/// of it: detection must not move a verdict that was already right.
#[test]
fn b23_empty_proof_is_the_same_verdict_under_either_reading() {
    let verdict = common::verdict("b01_empty_proof.cnf", "b01_empty_proof.lrat");
    assert_eq!(rejection(verdict).reason, Reason::NoEmptyClause);
}

/// B24. A deletion naming a clause that is not present.
///
/// Counted, not rejected — part 1's rule, unchanged. Deletion only ever
/// removes tools from the checker, so it can cause a later rejection and never
/// a false `VERIFIED`, and being strict here would refuse other producers'
/// output for no safety gain.
#[test]
fn b24_unknown_deletion_is_counted_not_rejected() {
    let outcome = inline("p cnf 3 3\n1 3 0\n-1 0\n-3 0\n", "d 2 0\n1 3 0\n0\n");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert_eq!(outcome.stats.unknown_deletions, 1);
}

/// B25. A deletion naming a live **unit** clause is honoured.
///
/// The documented difference from `drat-trim`, which ignores such a deletion
/// to protect a root-level trail it keeps across steps. Refute keeps no such
/// trail — 87 unit lemmas over 31,195 steps on the A217058 a(4) certificate
/// made the saving not worth the retraction machinery — so honouring it costs
/// nothing and is the stricter reading.
///
/// Without clause `(1)` the formula has a model, so the empty clause does not
/// follow. Ignoring the deletion verifies this proof.
#[test]
fn b25_deleting_a_unit_clause_is_honoured() {
    let outcome = inline("p cnf 2 3\n1 0\n-1 2 0\n-2 0\n", "d 1 0\n0\n");
    assert_eq!(rejection(outcome.verdict).reason, Reason::NoConflict);
}

/// B26. A proof line past `max_line_bytes`, reported before it is decoded.
#[test]
fn b26_a_long_drat_line_is_bounded_before_it_is_decoded() {
    let limits = Limits {
        max_line_bytes: 16,
        ..Limits::default()
    };
    let proof = format!("{} 0\n0\n", "1 ".repeat(64));
    let outcome = inline_with("p cnf 3 3\n1 3 0\n-1 0\n-3 0\n", &proof, &limits);
    match rejection(outcome.verdict).reason {
        Reason::Parse(err) => assert_eq!(
            err.kind,
            refute::ParseErrorKind::LineTooLong { limit: 16 },
            "wrong parse error"
        ),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

/// B27. A repeated literal in a lemma is the same literal.
///
/// The pivot is the first literal as written, and assigning it twice is
/// idempotent. A checker that counts free literals rather than distinct ones
/// rejects a proof `drat-trim` verifies.
#[test]
fn b27_a_repeated_literal_in_a_lemma_verifies() {
    let outcome = inline("p cnf 3 3\n1 3 0\n-1 0\n-3 0\n", "1 1 3 0\n0\n");
    assert_eq!(outcome.verdict, Verdict::Verified);
}

/// B28. A text DRAT proof whose first line is a deletion is not binary.
///
/// Milestone 1b's sniff — first byte `a` or `d` — was exactly right while LRAT
/// was the only text format, because a text LRAT line begins with a decimal
/// identifier. A text DRAT deletion line begins `d `, and under the old rule
/// this file is reported as a binary proof and never read at all.
#[test]
fn b28_a_leading_deletion_line_is_not_a_binary_proof() {
    let verdict = common::verdict("real_rat_proof.cnf", "b29_deletion_first.drat");
    assert!(
        !matches!(verdict, Verdict::Unsupported(_)),
        "a text proof was reported as binary: {verdict:?}"
    );
    let run = common::cli("real_rat_proof.cnf", "b29_deletion_first.drat");
    assert_ne!(run.code, 2, "stderr was {:?}", run.stderr);
}

/// B29. A CRLF DRAT proof parses. `kissat` writes the platform's line endings,
/// so a proof generated on Windows is CRLF throughout.
#[test]
fn b29_a_crlf_drat_proof_parses() {
    assert_eq!(
        common::verdict("tiny_unsat.cnf", "b30_crlf.drat"),
        Verdict::Verified
    );
}

/// B30. The binary proof keeps its verdict under the widened sniff.
///
/// Binary DRAT terminates every record with 0x00, and `b17` satisfies both
/// clauses of the new rule, so widening it to admit a leading `d ` cannot have
/// cost this case.
#[test]
fn b30_a_binary_proof_is_still_unsupported() {
    assert!(matches!(
        common::verdict("b17_binary_proof.cnf", "b17_binary_proof.lrat"),
        Verdict::Unsupported(_)
    ));
    common::cli("b17_binary_proof.cnf", "b17_binary_proof.lrat").assert("s UNSUPPORTED", 2);
}

/// B31. A DRAT file forced with `--lrat` is rejected, and asserted **not** to
/// exit 0.
///
/// The user made a claim about the file and the file contradicted it. That is
/// a rejection, not a usage error: the exit code is the verdict.
#[test]
fn b31_a_drat_file_forced_as_lrat_is_rejected() {
    let run = common::cli_args(&[
        "--lrat".to_owned(),
        common::fixture("real_rat_proof.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("real_rat_proof.drat")
            .to_string_lossy()
            .into_owned(),
    ]);
    run.assert("s NOT VERIFIED", 1);
}

/// B32. An LRAT file forced with `--drat` is rejected, and asserted **not** to
/// exit 0. The other half of B31, and the test that dies if detection is
/// bypassed by forcing one format everywhere.
#[test]
fn b32_an_lrat_file_forced_as_drat_is_rejected() {
    let run = common::cli_args(&[
        "--drat".to_owned(),
        common::fixture("real_rat_proof.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("real_rat_proof.lrat")
            .to_string_lossy()
            .into_owned(),
    ]);
    run.assert("s NOT VERIFIED", 1);
}

/// B33. The regression guard on detection's default arm.
///
/// `hostile_escape_proof.lrat` is accepted by neither grammar — its first line
/// is a terminal escape sequence — so it falls to the default arm, which is
/// the incumbent LRAT reader. Unchanged verdict, unchanged reason, unchanged
/// escaped message. If detection ever starts guessing, this is what says so.
#[test]
fn b33_a_file_neither_grammar_accepts_keeps_milestone_1s_message() {
    let run = common::cli("hostile_escape_proof.cnf", "hostile_escape_proof.lrat");
    run.assert("s NOT VERIFIED", 1);
    assert!(
        run.stderr.contains("\\x1b"),
        "the escape was not escaped: {:?}",
        run.stderr
    );
    assert!(
        !run.stderr.contains('\u{1b}'),
        "a raw escape byte reached stderr"
    );
}

/// B34. A comment line in a DRAT proof is a parse error.
///
/// `kissat` writes none, measured at zero occurrences, so this fails closed on
/// a file nobody has been observed to write rather than guessing at what a
/// leading `c` means in a format that has no comments.
#[test]
fn b34_a_comment_line_in_a_drat_proof_is_a_parse_error() {
    let outcome = inline("p cnf 3 3\n1 3 0\n-1 0\n-3 0\n", "1 3 0\nc a comment\n0\n");
    match rejection(outcome.verdict).reason {
        Reason::Parse(err) => assert_eq!(
            err.kind,
            refute::ParseErrorKind::NotAnInteger("c".to_owned())
        ),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

/// B35. A lemma naming a variable the formula never mentions.
///
/// Every per-variable and per-literal vector is sized from the formula and
/// grown on demand, never from the `p` line and never from `Limits::max_var`.
/// Part 1 learned that on the assignment vector, where nineteen bytes of
/// header bought a 64 MB allocation; there are three more such vectors now,
/// two of them vectors of vectors.
#[test]
fn b35_a_lemma_over_an_unseen_variable_grows_the_vectors() {
    let outcome = inline("p cnf 1 2\n1 0\n-1 0\n", "5 0\n0\n");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert!(
        outcome.stats.assignment_slots > 5,
        "the assignment vector did not grow: {} slots",
        outcome.stats.assignment_slots
    );
}

// ----------------------------------------------------------------- CLI shape

/// The `check` verb is accepted and means exactly what its absence means.
///
/// `refute <cnf> <proof>` is a documented two-positional contract that twelve
/// CLI tests assert, so the verb is additive: accepted only when there are
/// exactly three positional arguments and the first is `check`.
#[test]
fn the_optional_check_verb_changes_nothing() {
    let bare = common::cli("tiny_unsat.cnf", "tiny_unsat.drat");
    let verb = common::cli_args(&[
        "check".to_owned(),
        common::fixture("tiny_unsat.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("tiny_unsat.drat")
            .to_string_lossy()
            .into_owned(),
    ]);
    bare.assert("s VERIFIED", 0);
    verb.assert("s VERIFIED", 0);
    assert_eq!(bare.stderr, verb.stderr);
}

/// `--stats` prints the DRAT counter line only when the DRAT checker ran.
///
/// A counter block full of zeroes teaches a reader that the numbers do not
/// mean anything, which is the opposite of why they exist.
#[test]
fn the_drat_counter_line_is_printed_only_for_a_drat_run() {
    let drat = common::cli_args(&[
        "--stats".to_owned(),
        common::fixture("real_rat_proof.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("real_rat_proof.drat")
            .to_string_lossy()
            .into_owned(),
    ]);
    let lrat = common::cli_args(&[
        "--stats".to_owned(),
        common::fixture("real_rat_proof.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("real_rat_proof.lrat")
            .to_string_lossy()
            .into_owned(),
    ]);
    drat.assert("s VERIFIED", 0);
    lrat.assert("s VERIFIED", 0);
    assert!(
        drat.stderr.contains("occurrence updates"),
        "no DRAT counter line: {:?}",
        drat.stderr
    );
    assert!(
        !lrat.stderr.contains("occurrence updates"),
        "the DRAT counter line was printed for an LRAT run: {:?}",
        lrat.stderr
    );
    // The bet, made observable on a reader's own proof: 593 index slot
    // updates against the scan this file would have cost, which is 20 RAT
    // additions times a database averaging some forty live clauses.
    assert!(
        drat.stderr.contains("593 occurrence updates"),
        "the occurrence count moved: {:?}",
        drat.stderr
    );
}
