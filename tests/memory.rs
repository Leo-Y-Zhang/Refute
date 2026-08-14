//! The milestone-3 counter controls: P21, B37, B38 and the store's own size.
//!
//! Written against the binary as it stands, which is `docs/TDD.md` part 4's
//! build order step 3. A memory rule cannot be pinned by a verdict — every
//! store variant part 4 measured returned the same verdict on every artefact —
//! so these assert counters, and the counters are the only control a memory
//! regression moves.
//!
//! Two of the tests here are green today and say so in their own doc comment.
//! The rest are red, and each names which rule it is red for: the counter that
//! is not computed yet, or the reclamation rule that does not exist yet. A test
//! that goes green in two steps is recorded as such rather than re-aimed at
//! whichever step lands first.

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

use refute::Verdict;

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
    let outcome = common::outcome("tiny_unsat.cnf", "tiny_unsat.drat");
    assert_eq!(outcome.verdict, Verdict::Verified);
    assert_eq!(outcome.stats.deletions, 0, "the fixture began deleting");
    assert_eq!(
        outcome.stats.compactions, 0,
        "a proof that deletes nothing compacted an arena with nothing dead in it"
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
