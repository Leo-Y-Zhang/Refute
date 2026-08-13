//! The `refute` command line interface.
//!
//! Silence until the verdict is deliberate: this tool gets piped, and a
//! progress bar in a pipe is noise. Output is plain ASCII with no colour, so a
//! verdict survives `refute a.cnf b.lrat > log.txt`.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::ExitCode;

use refute::{check, parse_dimacs, Limits, LratReader, Verdict};

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

    let mut positional: Vec<&str> = Vec::new();
    let mut stats = false;
    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return EXIT_VERIFIED;
            }
            "--version" | "-V" => {
                println!("refute {}", env!("CARGO_PKG_VERSION"));
                return EXIT_VERIFIED;
            }
            "--stats" => stats = true,
            other => positional.push(other),
        }
    }
    let _ = stats;

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

    let limits = Limits::default();

    let cnf = match parse_dimacs(BufReader::new(formula_file), &limits) {
        Ok(cnf) => cnf,
        Err(err) => {
            // A formula we cannot read is a proof we cannot accept.
            return report_not_verified(&err.to_string());
        }
    };
    for warning in &cnf.warnings {
        eprintln!("refute: warning: {warning}");
    }

    let proof = LratReader::new(BufReader::new(proof_file), &limits);
    match check(&cnf, proof, &limits) {
        Verdict::Verified => {
            println!("s VERIFIED");
            EXIT_VERIFIED
        }
        Verdict::NotVerified(rejection) => report_not_verified(&rejection.to_string()),
        Verdict::Unsupported(unsupported) => {
            println!("s UNSUPPORTED");
            eprintln!("refute: {unsupported}");
            EXIT_UNSUPPORTED
        }
    }
}

fn report_not_verified(detail: &str) -> u8 {
    println!("s NOT VERIFIED");
    eprintln!("refute: {detail}");
    let _ = std::io::stdout().flush();
    EXIT_NOT_VERIFIED
}
