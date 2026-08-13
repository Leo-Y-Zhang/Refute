//! Shared fixture plumbing.
//!
//! Two routes into the same code: `verdict` goes through the library, so a test
//! can assert an exact [`Reason`]; `cli` runs the built binary, so a test can
//! assert an exit code and the literal strings a downstream script will grep
//! for. Anything that matters is asserted on both.

#![allow(dead_code)]

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

use refute::cnf::Warning;
use refute::{Limits, Verdict};

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Checks a fixture pair through the library.
pub fn verdict(cnf: &str, proof: &str) -> Verdict {
    checked(cnf, proof).0
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
