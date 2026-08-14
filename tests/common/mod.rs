//! Shared fixture plumbing.
//!
//! Two routes into the same code: `verdict` goes through the library, so a test
//! can assert an exact [`Reason`]; `cli` runs the built binary, so a test can
//! assert an exit code and the literal strings a downstream script will grep
//! for. Anything that matters is asserted on both.

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
// Each test crate uses a different part of this module.
#![allow(dead_code)]

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

use refute::cnf::Warning;
use refute::{Limits, Verdict};

pub fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

/// The committed corpus itself, for the tests that are about the corpus
/// rather than about one file in it.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Checks a fixture pair through the library.
pub fn verdict(cnf: &str, proof: &str) -> Verdict {
    checked(cnf, proof).0
}

/// Checks a fixture pair through the library, keeping everything it produced.
pub fn outcome(cnf: &str, proof: &str) -> refute::Outcome {
    outcome_with_limits(cnf, proof, &Limits::default())
}

/// The same, under limits the caller chooses.
pub fn outcome_with_limits(cnf: &str, proof: &str, limits: &Limits) -> refute::Outcome {
    let formula = File::open(fixture(cnf)).unwrap();
    let proof = File::open(fixture(proof)).unwrap();
    refute::check_readers(BufReader::new(formula), BufReader::new(proof), limits)
}

/// Checks a fixture pair through the library, keeping the formula's warnings.
pub fn checked(cnf: &str, proof: &str) -> (Verdict, Vec<Warning>) {
    let formula = File::open(fixture(cnf)).unwrap();
    let proof = File::open(fixture(proof)).unwrap();
    let outcome = refute::check_readers(
        BufReader::new(formula),
        BufReader::new(proof),
        &Limits::default(),
    );
    (outcome.verdict, outcome.warnings)
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the built binary on a fixture pair.
pub fn cli(cnf: &str, proof: &str) -> Run {
    cli_args(&[
        fixture(cnf).to_string_lossy().into_owned(),
        fixture(proof).to_string_lossy().into_owned(),
    ])
}

/// Runs the built binary with arbitrary arguments.
pub fn cli_args(args: &[String]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_refute"))
        .args(args)
        .output()
        .unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

impl Run {
    /// Asserts the exact contract a caller depends on: the verdict token on
    /// stdout and the exit code, together.
    pub fn assert(&self, token: &str, code: i32) {
        assert_eq!(
            self.stdout.trim_end(),
            token,
            "stdout was {:?}, stderr {:?}",
            self.stdout,
            self.stderr
        );
        assert_eq!(self.code, code, "stderr was {:?}", self.stderr);
    }
}
