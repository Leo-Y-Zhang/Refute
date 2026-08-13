//! The `refute` command line interface.
//!
//! Silence until the verdict is deliberate: this tool gets piped, and a
//! progress bar in a pipe is noise. Output is plain ASCII with no colour, so a
//! verdict survives `refute a.cnf b.lrat > log.txt`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use refute::{check_readers, Limits, Verdict};

const USAGE: &str = "usage: refute <formula.cnf> <proof.lrat> [--stats]";

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
            "--help" | "-h" | "--version" | "-V" => {
                eprintln!("{USAGE}");
                return EXIT_USAGE;
            }
            other => positional.push(other),
        }
    }

    let (formula_path, proof_path) = match positional.as_slice() {
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

    let outcome = check_readers(
        BufReader::new(formula_file),
        BufReader::new(proof_file),
        &Limits::default(),
    );

    for warning in &outcome.warnings {
        eprintln!("refute: warning: {warning}");
    }
    if stats {
        let counters = outcome.stats;
        eprintln!(
            "refute: {} additions, {} deletions, {} hints resolved, \
             {} peak live clauses, {} unknown deletions, \
             {} assignments, {} of them undone",
            counters.additions,
            counters.deletions,
            counters.hints_resolved,
            counters.peak_live_clauses,
            counters.unknown_deletions,
            counters.assignments,
            counters.assignments_undone
        );
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
