//! The `refute` command line interface.
//!
//! Silence until the verdict is deliberate: this tool gets piped, and a
//! progress bar in a pipe is noise. Output is plain ASCII with no colour, so a
//! verdict survives `refute a.cnf b.lrat > log.txt`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use refute::{check_readers, check_readers_with_format, Format, Limits, Verdict};

const USAGE: &str =
    "usage: refute [check] <formula.cnf> <proof.lrat|proof.drat> [--drat|--lrat] [--stats]";

/// The one flag `USAGE` does not list, and deliberately.
///
/// It sets `Limits::max_dead_arena_lits`, which decides when the DRAT store
/// compacts its arena. It exists so that `tools/fuzz.py --force-compaction`
/// can drive ten thousand random proofs through code that a random proof would
/// otherwise almost never reach: they are small and they delete little. It
/// changes no verdict on any input — that is the property the fuzz run is
/// there to test — so documenting it in the usage line would advertise a knob
/// with nothing for a user to gain by turning it.
const DEAD_ARENA_FLAG: &str = "--max-dead-arena-lits=";

/// Verified. The only success.
const EXIT_VERIFIED: u8 = 0;
/// Read and found wanting, including anything that failed to parse.
const EXIT_NOT_VERIFIED: u8 = 1;
/// A construct this milestone does not check. Never confusable with success.
const EXIT_UNSUPPORTED: u8 = 2;
/// Nothing was checked: bad arguments, or a file that would not open.
/// Distinct from 1 on purpose, so a typo in CI does not read as a bad proof.
const EXIT_USAGE: u8 = 3;

fn main() -> ExitCode {
    ExitCode::from(run())
}

/// Bytes as kilobytes, truncating, but never `0` for something that is there.
///
/// The figures this prints are megabytes on the proofs the counter exists for,
/// so kilobytes is the right unit and truncation costs nothing. On a small
/// proof it costs something real: `0 KB live arena` on a database that holds
/// forty clauses reads as "nothing is live", which is the opposite of true.
/// `<1` says the same thing about the size and nothing untrue about the
/// contents.
///
/// Division is a method call rather than `/` because the package denies
/// arithmetic operators and a literal divisor is not an exception the lint
/// makes.
fn kb(bytes: usize) -> String {
    match bytes {
        0 => "0".to_owned(),
        n if n < 1024 => "<1".to_owned(),
        n => n.saturating_div(1024).to_string(),
    }
}

fn run() -> u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Help and version are answered only when they are the whole command line.
    // Honouring them from anywhere in argv means `refute a.cnf a.lrat --help`
    // exits 0 with nothing checked, and the documented contract is that the
    // exit code is the verdict: one stray argument in a CI script would read
    // as a pass for a proof that was never opened.
    if let [only] = args.as_slice() {
        match only.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return EXIT_VERIFIED;
            }
            "--version" | "-V" => {
                println!("refute {}", env!("CARGO_PKG_VERSION"));
                return EXIT_VERIFIED;
            }
            _ => {}
        }
    }

    let mut positional: Vec<&str> = Vec::new();
    let mut stats = false;
    let mut flags_ended = false;
    let mut forced: Option<Format> = None;
    let mut dead_arena_lits: Option<usize> = None;
    for arg in &args {
        if flags_ended {
            positional.push(arg);
            continue;
        }
        match arg.as_str() {
            // Everything after `--` is a path, so a file really called
            // `--help` can still be checked.
            "--" => flags_ended = true,
            "--stats" => stats = true,
            // Skip detection entirely. A file that then fails to parse is a
            // rejection and not a usage error: the user made a claim about the
            // file and the file contradicted it, which is a verdict.
            "--drat" => forced = Some(Format::Drat),
            "--lrat" => forced = Some(Format::Lrat),
            "--help" | "-h" | "--version" | "-V" => {
                eprintln!("{USAGE}");
                return EXIT_USAGE;
            }
            // A bad value is a usage error and not a verdict, because nothing
            // about the proof was in question. Same treatment as a missing
            // path: exit 3, so a typo cannot read as a pass.
            other if other.starts_with(DEAD_ARENA_FLAG) => {
                match other
                    .get(DEAD_ARENA_FLAG.len()..)
                    .and_then(|value| value.parse::<usize>().ok())
                {
                    Some(lits) => dead_arena_lits = Some(lits),
                    None => {
                        eprintln!("refute: '{other}' needs a non-negative number");
                        return EXIT_USAGE;
                    }
                }
            }
            other => positional.push(other),
        }
    }

    // `refute check <cnf> <proof>` is the same command as `refute <cnf>
    // <proof>`. The two-positional form is a documented contract that twelve
    // tests assert, so the verb is additive: accepted only when there are
    // exactly three positional arguments and the first is `check`. A file
    // genuinely called `check` is still reachable, as `refute -- check b.drat`
    // or by any path with a separator in it.
    let paths: &[&str] = match positional.as_slice() {
        [verb, rest @ ..] if *verb == "check" && rest.len() == 2 && !flags_ended => rest,
        other => other,
    };
    let (formula_path, proof_path) = match paths {
        [formula, proof] => (Path::new(formula), Path::new(proof)),
        _ => {
            eprintln!("{USAGE}");
            return EXIT_USAGE;
        }
    };

    let formula_file = match File::open(formula_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("refute: cannot open '{}': {err}", formula_path.display());
            return EXIT_USAGE;
        }
    };
    let proof_file = match File::open(proof_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("refute: cannot open '{}': {err}", proof_path.display());
            return EXIT_USAGE;
        }
    };

    let formula = BufReader::new(formula_file);
    let proof = BufReader::new(proof_file);
    let mut limits = Limits::default();
    if let Some(lits) = dead_arena_lits {
        limits.max_dead_arena_lits = lits;
    }
    let outcome = match forced {
        Some(format) => check_readers_with_format(formula, proof, &limits, format),
        None => check_readers(formula, proof, &limits),
    };

    for warning in &outcome.warnings {
        eprintln!("refute: warning: {warning}");
    }
    if stats {
        let counters = outcome.stats;
        eprintln!(
            "refute: {} additions, {} deletions, {} hints resolved, \
             {} peak live clauses, {} unknown deletions, \
             {} assignments, {} of them undone, {} assignment slots",
            counters.additions,
            counters.deletions,
            counters.hints_resolved,
            counters.peak_live_clauses,
            counters.unknown_deletions,
            counters.assignments,
            counters.assignments_undone,
            counters.assignment_slots
        );
        // The second line is the RAT side, and `candidates examined` is on it
        // deliberately: the candidate scan is the one performance bet in the
        // design, and this is what makes it observable on a reader's own proof
        // instead of re-argued from the design's measurements.
        eprintln!(
            "refute: {} RAT additions, {} vacuous, {} resolvent blocks, \
             {} candidate scans, {} candidates examined, {} candidates found",
            counters.rat_additions,
            counters.vacuous_rat_additions,
            counters.resolvent_blocks,
            counters.candidate_scans,
            counters.candidates_examined,
            counters.resolution_candidates
        );
        // Only when the DRAT checker ran, so the block is never a wall of
        // zeroes. `occurrence updates` is the one performance bet in milestone
        // 2, made observable on a reader's own proof rather than re-argued
        // from the design's measurements.
        if outcome.format == Format::Drat {
            eprintln!(
                "refute: {} RUP additions, {} tautological, {} candidates checked, \
                 {} propagations, {} watch visits, {} occurrence updates",
                counters.rup_additions,
                counters.tautological_additions,
                counters.rat_candidates_checked,
                counters.propagations,
                counters.watch_visits,
                counters.occurrence_updates
            );
            // What the store holds, beside what it did. A memory rule cannot
            // be pinned by a verdict — every store variant `docs/TDD.md`
            // part 4 measured returned the same verdict on every artefact —
            // so it is pinned by these, and a counter a reader cannot see on
            // their own proof is not the control the milestone is buying.
            // Kilobytes truncate: a fixture small enough to report 0 KB is a
            // fixture whose store was never the question.
            eprintln!(
                "refute: {} KB held, {} KB live arena, {} KB dead arena, \
                 {} compactions, {} deletion index entries, \
                 {} occurrence entries filtered",
                kb(counters.store_bytes),
                kb(counters.live_arena_bytes),
                kb(counters.dead_arena_bytes),
                counters.compactions,
                counters.deletion_index_entries,
                counters.occurrence_entries_filtered
            );
        }
    }

    // One match, no wildcard arm: a new verdict variant must be handled here
    // rather than falling silently into the success branch.
    match outcome.verdict {
        Verdict::Verified => {
            println!("s VERIFIED");
            EXIT_VERIFIED
        }
        Verdict::NotVerified(rejection) => {
            println!("s NOT VERIFIED");
            eprintln!("refute: {rejection}");
            EXIT_NOT_VERIFIED
        }
        Verdict::Unsupported(unsupported) => {
            println!("s UNSUPPORTED");
            eprintln!("refute: {unsupported}");
            EXIT_UNSUPPORTED
        }
    }
}
