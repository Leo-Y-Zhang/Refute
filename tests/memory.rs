//! The milestone-3 controls: what the store holds, and what it must still
//! decide while holding less of it.
//!
//! `docs/TDD.md` part 4's build order step 3, in two commits because half of
//! it could not be written until `max_dead_arena_lits` existed to write it
//! against. Two kinds of control live here and they are red for different
//! reasons, which each one's own doc comment states:
//!
//! - **Counter controls.** A memory rule cannot be pinned by a verdict — every
//!   store variant part 4 measured returned the same verdict on every artefact,
//!   including a change that moved peak working set by a factor of five and
//!   left all 128 tests green. So these assert counters, and a counter is the
//!   only control a memory regression moves.
//! - **Safety controls.** Hand-built proofs over a formula with a model, each
//!   aimed at one way the reclamation could quietly corrupt the database.
//!   Several are green today *for the wrong reason*: nothing compacts yet, so
//!   they are the default store run twice. They say so, and the commit that
//!   adds compaction is the one that makes them mean anything. Their real
//!   evidence is the mutation-kill table, which is mandatory content of the
//!   milestone.
//!
//! A test that goes green in two steps is recorded as such rather than
//! re-aimed at whichever step lands first.

// A test asserts by panicking: `unwrap` on a fixture that must open, `panic!`
// on a verdict that must not happen. The package's panic floor in Cargo.toml is
// there for the library and the binary, where a panic on input-derived data is
// a denial of service. Here it would only make the failure report worse.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use std::io::Cursor;

use refute::limits::DEFAULT_MAX_LINE_BYTES;
use refute::{Limits, Reason, Stats, Verdict};

/// A formula with a model. Every hand-built control below runs against it.
///
/// `kissat 4.0.1` reports `s SATISFIABLE` with the model `1 2 3 4`, and a
/// reader can check that by eye: `1` satisfies the first two clauses, `3` the
/// third, `4` the fourth. That is what makes each rejection below a
/// requirement rather than a preference. A formula with a model has no
/// refutation, so `s VERIFIED` on any of these is a false accept whatever
/// reason string the checker would otherwise have printed, and every one of
/// them ends in `0` so that a checker which accepted the step before it would
/// have to say so out loud.
const SATISFIABLE: &str = "p cnf 4 4\n1 2 0\n1 -2 0\n-1 3 0\n-1 -3 4 0\n";

/// The lemma `1`, which every hand-built proof below opens or follows with.
///
/// RUP against [`SATISFIABLE`]: assume `-1`, the first clause forces `2`, and
/// the second is then falsified. Once it is live it forces `3` and `4` too,
/// which is what makes [`churn`] cheap to write.
const UNIT_LEMMA: &str = "1 0\n";

/// The compaction floor set to zero: compact on any deletion that leaves a
/// dead literal behind and a dead half larger than the live one.
fn forced() -> Limits {
    Limits {
        max_dead_arena_lits: 0,
        ..Limits::default()
    }
}

/// Checks text held here rather than in a fixture.
///
/// A fixture earns its place by carrying provenance — a solver ran, and these
/// bytes came out. These proofs are hand-built to reach one code path each and
/// have none to carry, and the corpus is at 496 KB of its 500 KB budget.
fn inline(cnf: &str, proof: &str, limits: &Limits) -> refute::Outcome {
    refute::check_readers(
        Cursor::new(cnf.as_bytes()),
        Cursor::new(proof.as_bytes()),
        limits,
    )
}

/// The same input under both settings of the compaction floor.
///
/// Returned as a pair because this milestone's whole safety argument is an
/// equality between them: compaction changes what the store holds and nothing
/// about what it decides.
fn under_both(cnf: &str, proof: &str) -> (refute::Outcome, refute::Outcome) {
    (
        inline(cnf, proof, &Limits::default()),
        inline(cnf, proof, &forced()),
    )
}

fn rejection(verdict: Verdict) -> refute::Rejection {
    match verdict {
        Verdict::NotVerified(rejection) => rejection,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// `count` two-literal lemmas over fresh variables from `first_var`, each
/// added and then deleted.
///
/// Every one is RUP once [`UNIT_LEMMA`] is live: `1` forces `3` through the
/// third clause, so a clause holding `3` is refuted by its own negation.
/// Deleting them again is the point — they exist to fill the arena with dead
/// literals, and at a floor of zero nothing smaller than eight of them reaches
/// the compaction trigger, because the trigger is a comparison against the
/// live half and the live half is this formula.
fn churn(first_var: i32, count: i32) -> String {
    let mut text = String::new();
    for step in 0..count {
        let var = first_var.saturating_add(step);
        text.push_str(&format!("3 {var} 0\n"));
    }
    for step in 0..count {
        let var = first_var.saturating_add(step);
        text.push_str(&format!("d 3 {var} 0\n"));
    }
    text
}

/// Every counter that describes what the run *checked*, as against what it
/// held.
///
/// The split is the milestone's safety argument made assertable. Compaction,
/// the `bykey` prune and the lazy occurrence index each change the second
/// group and must leave the first alone, to the unit — not "the verdict is the
/// same", which the four experiments in `docs/TDD.md` part 4 showed is true of
/// a store that has been quietly broken as well as of one that has not.
fn assert_same_check(default: &Stats, forced: &Stats, what: &str) {
    assert_eq!(default.additions, forced.additions, "{what}: additions");
    assert_eq!(default.deletions, forced.deletions, "{what}: deletions");
    assert_eq!(
        default.unknown_deletions, forced.unknown_deletions,
        "{what}: unknown deletions"
    );
    assert_eq!(
        default.peak_live_clauses, forced.peak_live_clauses,
        "{what}: peak live clauses"
    );
    assert_eq!(
        default.assignments, forced.assignments,
        "{what}: assignments"
    );
    assert_eq!(
        default.assignments_undone, forced.assignments_undone,
        "{what}: assignments undone"
    );
    assert_eq!(
        default.propagations, forced.propagations,
        "{what}: propagations"
    );
    assert_eq!(
        default.watch_visits, forced.watch_visits,
        "{what}: watch visits"
    );
    assert_eq!(
        default.rup_additions, forced.rup_additions,
        "{what}: RUP additions"
    );
    assert_eq!(
        default.rat_additions, forced.rat_additions,
        "{what}: RAT additions"
    );
    assert_eq!(
        default.tautological_additions, forced.tautological_additions,
        "{what}: tautological additions"
    );
    assert_eq!(
        default.rat_candidates_checked, forced.rat_candidates_checked,
        "{what}: RAT candidates checked"
    );
}

/// The store reports the bytes it holds, on a DRAT run.
///
/// Red: `store_bytes` and `live_arena_bytes` are declared and never computed,
/// so both are zero on every run. The arena of a proof that adds six clauses to
/// a formula of eight cannot be empty.
#[test]
fn the_drat_store_reports_the_bytes_it_holds() {
    let stats = common::outcome("tiny_unsat.cnf", "tiny_unsat.drat").stats;
    assert!(
        stats.store_bytes > 0,
        "a run with {} additions reported a store of zero bytes",
        stats.additions
    );
    assert!(
        stats.live_arena_bytes > 0,
        "a run with {} peak live clauses reported an empty live arena",
        stats.peak_live_clauses
    );
    let arena = stats
        .live_arena_bytes
        .checked_add(stats.dead_arena_bytes)
        .unwrap();
    assert!(
        arena <= stats.store_bytes,
        "the arena is {arena} bytes of a store that says it holds {}",
        stats.store_bytes
    );
}

/// P21. The deletion index holds one entry per distinct *live* body.
///
/// Red twice over, and the second is the defect the counter exists to expose:
/// `bykey` keeps a key whose identifier list has been emptied, so it holds an
/// entry per distinct body ever added — 1,111 of them on this artefact, against
/// 478 clauses still live — which is a second copy of the whole proof.
///
/// The bound is `peak_live_clauses` rather than a literal, because the live
/// count at the end is never above the peak and the peak is already counted.
#[test]
fn p21_the_deletion_index_holds_live_bodies_not_every_body_ever_added() {
    let outcome = common::outcome("vdw_a217058_n21.cnf", "vdw_a217058_n21.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    let stats = outcome.stats;
    assert!(
        stats.deletion_index_entries > 0,
        "a run with {} additions reported an empty deletion index",
        stats.additions
    );
    assert!(
        stats.deletion_index_entries <= stats.peak_live_clauses,
        "the deletion index holds {} entries against {} peak live clauses, \
         so it is keeping bodies that are no longer live",
        stats.deletion_index_entries,
        stats.peak_live_clauses
    );
}

/// B38. The dead arena is bounded by the live one.
///
/// Red for the counter today; still red when the counters land, because
/// nothing reclaims the arena and this artefact deletes 633 of the 1,111
/// clauses it ever holds. It goes green on the commit that adds compaction,
/// which is the only commit in part 4 that may turn it.
#[test]
fn b38_the_dead_arena_is_bounded_by_the_live_arena() {
    let outcome = common::outcome("vdw_a217058_n21.cnf", "vdw_a217058_n21.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    let stats = outcome.stats;
    assert!(
        stats.store_bytes > 0,
        "the store reported zero bytes after {} additions and {} deletions",
        stats.additions,
        stats.deletions
    );
    assert!(
        stats.dead_arena_bytes <= stats.live_arena_bytes,
        "{} dead arena bytes against {} live, after {} deletions and {} compactions",
        stats.dead_arena_bytes,
        stats.live_arena_bytes,
        stats.deletions,
        stats.compactions
    );
}

/// The candidate queries report what they filtered.
///
/// Red: `occurrence_entries_filtered` is declared and never computed. The
/// artefact makes 72 RAT additions and checks 108 candidates, so the queries
/// walked something.
#[test]
fn the_candidate_queries_report_the_entries_they_filtered() {
    let stats = common::outcome("rat_pigeonhole.cnf", "rat_pigeonhole.drat").stats;
    assert!(
        stats.rat_candidates_checked > 0,
        "the fixture stopped exercising the RAT path"
    );
    assert!(
        stats.occurrence_entries_filtered >= stats.rat_candidates_checked,
        "{} occurrence entries filtered to answer queries that returned {} candidates",
        stats.occurrence_entries_filtered,
        stats.rat_candidates_checked
    );
}

/// B37. No deletion, no compaction.
///
/// **Green today, and for the wrong reason**: there is no compaction to run, so
/// the counter it would move is zero however the run went. It is here because
/// it is the control that must stay green through the commit that adds
/// compaction, and a control written afterwards proves nothing about it.
#[test]
fn b37_a_proof_with_no_deletions_never_compacts() {
    // The second pair is the one the mutation pass asked for. `tiny_unsat`
    // has no deletions and no RAT step either, so it says nothing about
    // *where* compaction is reachable from; `d09` has no deletions and 24 RAT
    // candidates, so a compaction called from the candidate loop shows up
    // here and nowhere else. That mutation killed nothing until this line
    // existed, and the design calls the trail-empty precondition
    // load-bearing.
    for (cnf, proof) in [
        ("tiny_unsat.cnf", "tiny_unsat.drat"),
        (
            "d09_trail_leak_between_candidates.cnf",
            "d09_trail_leak_between_candidates.drat",
        ),
    ] {
        for (limits, floor) in [(Limits::default(), "default"), (forced(), "forced")] {
            let stats = common::outcome_with_limits(cnf, proof, &limits).stats;
            assert_eq!(stats.deletions, 0, "{proof} began deleting");
            assert_eq!(
                stats.compactions, 0,
                "{proof} at the {floor} floor compacted an arena with nothing dead in it"
            );
        }
    }
    // Not a scene-setter: without a RAT step there is no candidate loop for a
    // compaction to be misplaced into.
    let rat = common::outcome(
        "d09_trail_leak_between_candidates.cnf",
        "d09_trail_leak_between_candidates.drat",
    );
    assert!(
        rat.stats.rat_candidates_checked > 0,
        "the fixture stopped exercising the candidate loop"
    );
}

/// The six counters are DRAT-only, so the LRAT path leaves every one at zero.
///
/// **Green today, and for the wrong reason**: they are zero everywhere. It is
/// the boundary case the doc comments claim — "DRAT only" — and the LRAT
/// checker has no store to report, so this is the one that must not move when
/// the DRAT ones start reporting.
#[test]
fn the_store_counters_stay_zero_on_the_lrat_path() {
    let outcome = common::outcome("rat_pigeonhole.cnf", "rat_pigeonhole.lrat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    let stats = outcome.stats;
    assert_eq!(stats.store_bytes, 0, "the LRAT path reported a store");
    assert_eq!(stats.live_arena_bytes, 0);
    assert_eq!(stats.dead_arena_bytes, 0);
    assert_eq!(stats.deletion_index_entries, 0);
    assert_eq!(stats.compactions, 0);
    assert_eq!(stats.occurrence_entries_filtered, 0);
}

/// `--stats` prints the store line on a DRAT run.
///
/// Red: part 4's build order step 2 is the counters *and* the line, because a
/// counter a reader cannot see on their own proof is not the control the
/// milestone is buying. The three lines that exist are unchanged.
#[test]
fn the_stats_output_reports_the_store_on_a_drat_run() {
    let run = common::cli_args(&[
        common::fixture("rat_pigeonhole.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("rat_pigeonhole.drat")
            .to_string_lossy()
            .into_owned(),
        "--stats".to_owned(),
    ]);
    run.assert("s VERIFIED", 0);
    assert!(
        run.stderr.contains("held") && run.stderr.contains("dead arena"),
        "the stats block has no store line: {:?}",
        run.stderr
    );
}

// ------------------------------------------------------- forcing the trigger

/// The forced setting really forces something.
///
/// **Red until compaction exists**, and it is the control that stops every
/// test below it from being a test of the default store run twice. The flag
/// reaches `Limits` — `tests/cli.rs` pins that — but a floor nothing reads is
/// a floor nothing obeys, and the harness that depends on it hardest,
/// `tools/fuzz.py --force-compaction`, would report the same summary either
/// way.
///
/// The fixture is the largest RAT-carrying proof in the corpus, at 487
/// deletions, so there is no question of it having too little to reclaim.
#[test]
fn forced_compaction_actually_compacts() {
    let forced =
        common::outcome_with_limits("rat_pigeonhole.cnf", "rat_pigeonhole.drat", &forced());
    assert_eq!(forced.verdict, Verdict::Verified);
    assert!(
        forced.stats.compactions > 0,
        "a floor of zero compacted nothing across {} deletions",
        forced.stats.deletions
    );
}

/// P22. Forcing compaction changes what is held and nothing that is checked.
///
/// `docs/TDD.md` part 4 numbers this P19; P19 through P21 were taken by
/// milestone 2 and by the counter controls above, so it lands here. The
/// content is the document's: the same fixture under both floors, and every
/// counter that describes the check asserted equal to the unit.
///
/// This is the whole safety argument for compaction, and it is an equality
/// rather than a verdict on purpose. All four store variants part 4 measured
/// returned the same verdict on every artefact in the ladder, including the
/// ones that were wrong about what they held.
#[test]
fn p22_forced_compaction_changes_no_counter_that_describes_the_check() {
    let default = common::outcome("rat_pigeonhole.cnf", "rat_pigeonhole.drat");
    let forced =
        common::outcome_with_limits("rat_pigeonhole.cnf", "rat_pigeonhole.drat", &forced());
    assert_eq!(default.verdict, Verdict::Verified);
    assert_eq!(forced.verdict, Verdict::Verified);
    assert_same_check(&default.stats, &forced.stats, "rat_pigeonhole.drat");
}

/// P23. Every committed proof, under both floors, reaches the same verdict.
///
/// Cheap, and it is the regression net for all of milestones 2 and 3 at once:
/// eighteen files, three of which are real certificates and eleven of which
/// are mutants built to be rejected for a named reason.
///
/// The table is checked against the directory rather than trusted, so a
/// fixture added without a line here fails this test instead of silently
/// escaping it. That is the failure mode of every hand-maintained list, and
/// the corpus has grown in every milestone so far.
#[test]
fn p23_every_committed_proof_agrees_under_both_floors() {
    const PAIRS: [(&str, &str); 18] = [
        ("tiny_unsat.cnf", "tiny_unsat.drat"),
        ("deletes_originals.cnf", "deletes_originals.drat"),
        ("real_rat_proof.cnf", "real_rat_proof.drat"),
        ("rat_pigeonhole.cnf", "rat_pigeonhole.drat"),
        ("empty_clause_in_cnf.cnf", "empty_clause_in_cnf.drat"),
        ("vdw_a217058_n21.cnf", "vdw_a217058_n21.drat"),
        ("real_rat_proof.cnf", "d01_addition_dropped.drat"),
        ("real_rat_proof.cnf", "d02_needed_addition_dropped.drat"),
        ("real_rat_proof.cnf", "d03_literal_flipped.drat"),
        ("real_rat_proof.cnf", "d04_additions_swapped.drat"),
        ("real_rat_proof.cnf", "d05_deleted_then_used.drat"),
        ("real_rat_proof.cnf", "d06_truncated.drat"),
        ("real_rat_proof.cnf", "d07_no_empty_clause.drat"),
        (
            "d08_satisfiable_formula.cnf",
            "d08_satisfiable_formula.drat",
        ),
        (
            "d09_trail_leak_between_candidates.cnf",
            "d09_trail_leak_between_candidates.drat",
        ),
        (
            "d10_duplicate_clause_deleted_once.cnf",
            "d10_duplicate_clause_deleted_once.drat",
        ),
        ("real_rat_proof.cnf", "b29_deletion_first.drat"),
        ("tiny_unsat.cnf", "b30_crlf.drat"),
    ];

    let committed = std::fs::read_dir(common::fixtures_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "drat"))
        .count();
    assert_eq!(
        PAIRS.len(),
        committed,
        "the corpus holds {committed} .drat files and this table names {}",
        PAIRS.len()
    );

    for (cnf, proof) in PAIRS {
        let default = common::outcome(cnf, proof);
        let forced = common::outcome_with_limits(cnf, proof, &forced());
        assert_eq!(default.verdict, forced.verdict, "{proof}");
        assert_same_check(&default.stats, &forced.stats, proof);
    }
}

// ------------------------------------------------------------------ negative

/// D13. A compaction must not mix one clause's literals with another's.
///
/// The rejecting candidate is added *after* eight lemmas that are then
/// deleted, so it is the one live clause in the arena with dead literals in
/// front of it, and compaction has to move it. The step that rejects reads its
/// literals back: `-20` is refused because the resolvent against `20 21` is
/// `-20 or 21`, which nothing implies. A compaction that copied the wrong
/// slice, or rewrote `start` for only the first live clause, gives that step
/// some other clause's literals and there is no file to disagree with it.
///
/// **Green today for the wrong reason** — nothing compacts yet, so this is the
/// default store run twice. The commit that adds compaction is the one that
/// makes it mean anything, and its real evidence is the mutation-kill table.
#[test]
fn d13_a_compacted_clause_keeps_its_own_literals() {
    let proof = format!(
        "{UNIT_LEMMA}{}20 21 0\n{}-20 0\n0\n",
        churn(5, 8),
        churn(13, 8)
    );
    let (default, forced) = under_both(SATISFIABLE, &proof);
    for (outcome, floor) in [(default, "default"), (forced, "forced")] {
        let rejection = rejection(outcome.verdict);
        assert_eq!(
            rejection.reason,
            Reason::RatCheckFailed {
                pivot: refute::Lit::new(-20).unwrap()
            },
            "{floor} floor"
        );
        assert_eq!(rejection.resolvent, Some(14), "{floor} floor: candidate");
    }
}

/// D14. The occurrence purge keeps the entries of live clauses.
///
/// The mirror of D13. Here the rejecting candidate is the *first* clause the
/// proof adds, so compaction never moves it; what it has to survive is sixteen
/// deletions and every occurrence purge they trigger. If the purge dropped a
/// live entry, `-20` would have no candidate at all, would be vacuously RAT,
/// and would be accepted — after which this satisfiable formula's proof runs
/// to its last line.
///
/// The candidate is named, not just counted. A rejection for the right reason
/// against the wrong clause is a checker that found *a* candidate rather than
/// *the* candidate, which is the shape the milestone-1b review found twice.
#[test]
fn d14_a_live_candidate_survives_every_purge() {
    let proof = format!(
        "20 21 0\n{UNIT_LEMMA}{}{}-20 0\n0\n",
        churn(5, 8),
        churn(13, 8)
    );
    let (default, forced) = under_both(SATISFIABLE, &proof);
    for (outcome, floor) in [(default, "default"), (forced, "forced")] {
        let rejection = rejection(outcome.verdict);
        assert_eq!(
            rejection.reason,
            Reason::RatCheckFailed {
                pivot: refute::Lit::new(-20).unwrap()
            },
            "{floor} floor"
        );
        assert_eq!(
            rejection.resolvent,
            Some(5),
            "{floor} floor: the candidate is the clause added first"
        );
    }
}

/// D15. The deletion index drops a key only when its last copy has gone.
///
/// Three identical clauses, two deleted. The verdict does not discriminate
/// here and saying so is the point: an index that dropped the key on the first
/// deletion leaves *more* clauses live, not fewer, so the rejection below
/// happens either way and against the same clause. What moves is the
/// bookkeeping — the second and third deletions stop naming anything, and the
/// live arena holds three copies where it should hold one.
///
/// Both are asserted. The live arena figure is the counter this milestone
/// added earning its place: it is the only assertion here that a checker
/// cannot pass by rejecting for the right reason by accident.
#[test]
fn d15_deleting_one_of_three_copies_leaves_the_other_two() {
    let copies = "20 21 0\n".repeat(3);
    let proof = format!("{copies}d 20 21 0\nd 20 21 0\n-20 0\n0\n");
    let (default, forced) = under_both(SATISFIABLE, &proof);
    for (outcome, floor) in [(default, "default"), (forced, "forced")] {
        let stats = outcome.stats;
        let rejection = rejection(outcome.verdict);
        assert_eq!(
            rejection.reason,
            Reason::RatCheckFailed {
                pivot: refute::Lit::new(-20).unwrap()
            },
            "{floor} floor"
        );
        assert_eq!(rejection.resolvent, Some(5), "{floor} floor: candidate");
        assert_eq!(
            stats.unknown_deletions, 0,
            "{floor} floor: a deletion stopped finding the clause it named"
        );
        // Nine literals of formula and one surviving copy of `20 21`. Three
        // surviving copies is fifteen, which is what the mutation gives.
        assert_eq!(
            stats.live_arena_bytes,
            11usize.saturating_mul(std::mem::size_of::<refute::Lit>()),
            "{floor} floor: {} live arena bytes",
            stats.live_arena_bytes
        );
    }
}

// ------------------------------------------------------------------ boundary

/// B39. A deletion still matches whatever order it names its literals in,
/// after a compaction.
///
/// The deletion index is keyed by the normalised literal set and compaction
/// does not touch it, so this must go on working — but "does not touch it" is
/// a claim about code that is about to be written. If the reversed deletion
/// stopped matching, `20 21` would still be live, `-20` would be refused
/// against it, and the run would end four steps earlier with a different
/// reason. That is what the assertion discriminates.
#[test]
fn b39_a_reversed_deletion_matches_after_a_compaction() {
    let proof = format!("20 21 0\n{UNIT_LEMMA}{}d 21 20 0\n-20 0\n0\n", churn(5, 8));
    let (default, forced) = under_both(SATISFIABLE, &proof);
    for (outcome, floor) in [(default, "default"), (forced, "forced")] {
        let stats = outcome.stats;
        assert_eq!(
            rejection(outcome.verdict).reason,
            Reason::NoConflict,
            "{floor} floor: the reversed deletion was not honoured"
        );
        assert_eq!(stats.unknown_deletions, 0, "{floor} floor");
    }
}

/// B40. A unit clause deleted after a compaction is still honoured.
///
/// B25 established that Refute honours a deletion `drat-trim` must ignore, and
/// it is the one place where honouring a deletion can turn a refutation into a
/// satisfiable formula. The unit list holds identifiers and compaction does
/// not touch it; what compaction does touch is where the unit's literal lives,
/// and `propagate_from_scratch` reads that literal through the metadata every
/// step. A compaction that mis-rewrote it would assign some other literal, and
/// on this formula that is a conflict, and a conflict here is `s VERIFIED` on
/// a formula that has a model once `1` is gone.
///
/// The formula refutes by propagation alone, so every churn lemma is RUP
/// against it without needing anything said about its shape.
#[test]
fn b40_a_unit_deleted_after_a_compaction_is_honoured() {
    let mut proof = String::new();
    for step in 0..8i32 {
        let var = step.saturating_mul(2).saturating_add(5);
        proof.push_str(&format!("{var} {} 0\n", var.saturating_add(1)));
    }
    for step in 0..8i32 {
        let var = step.saturating_mul(2).saturating_add(5);
        proof.push_str(&format!("d {var} {} 0\n", var.saturating_add(1)));
    }
    proof.push_str("d 1 0\n0\n");
    let (default, forced) = under_both("p cnf 2 3\n1 0\n-1 2 0\n-2 0\n", &proof);
    for (outcome, floor) in [(default, "default"), (forced, "forced")] {
        assert_eq!(
            rejection(outcome.verdict).reason,
            Reason::NoConflict,
            "{floor} floor: deleting the unit clause was not honoured"
        );
    }
}

/// B41. The line ceiling costs nothing on any real file.
///
/// The measurement that kept `max_line_bytes` where it is. Part 4 considered
/// lowering it to save memory and measured instead: the reader keeps the
/// capacity of the longest line it has seen and not the ceiling, so the
/// ceiling is free, and lowering it could only begin rejecting a legitimate
/// proof with one very long clause in it.
///
/// Asserted as a ratio rather than as the exact 204 bytes measured, because a
/// fixture with a longer line is a normal thing for a later milestone to
/// commit and this test has no business failing for it. What it does catch is
/// a corpus that has drifted three orders of magnitude, which is the only
/// drift that would make the ceiling worth revisiting.
#[test]
fn b41_the_longest_line_in_the_corpus_is_nowhere_near_the_ceiling() {
    let mut longest = 0usize;
    let mut held_by = String::new();
    for entry in std::fs::read_dir(common::fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let is_proof = path
            .extension()
            .is_some_and(|ext| ext == "drat" || ext == "lrat");
        if !is_proof {
            continue;
        }
        for line in std::fs::read(&path).unwrap().split(|byte| *byte == b'\n') {
            if line.len() > longest {
                longest = line.len();
                held_by = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
        }
    }
    assert!(longest > 0, "no proof fixture was read at all");
    assert!(
        longest.saturating_mul(1000) < DEFAULT_MAX_LINE_BYTES,
        "the longest proof line in the corpus is {longest} bytes, in {held_by}, \
         against a ceiling of {DEFAULT_MAX_LINE_BYTES}"
    );
}
