//! The command line contract.
//!
//! Exit codes and output strings are the interface a CI step depends on, so
//! they are asserted literally. Anything grepping for `VERIFIED` alone would
//! also match `NOT VERIFIED`; the documented test is on the exit code, and both
//! are checked here.

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

/// Nothing was checked, so the exit code is 3 and not 1. Conflating "no proof"
/// with "bad proof" hides a typo in a CI script for as long as it takes someone
/// to notice their gate has been passing on a missing file.
#[test]
fn missing_file_is_distinct_from_a_bad_proof() {
    let run = common::cli_args(&[
        common::fixture("tiny_unsat.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("no_such_file.lrat")
            .to_string_lossy()
            .into_owned(),
    ]);
    assert_eq!(run.code, 3, "stderr was {:?}", run.stderr);
    assert!(run.stdout.is_empty(), "stdout was {:?}", run.stdout);
    assert!(
        run.stderr.contains("cannot open"),
        "stderr was {:?}",
        run.stderr
    );
}

#[test]
fn no_arguments_prints_usage_and_exits_3() {
    let run = common::cli_args(&[]);
    assert_eq!(run.code, 3);
    assert!(run.stderr.starts_with("usage: refute"), "{:?}", run.stderr);
}

#[test]
fn one_argument_prints_usage_and_exits_3() {
    let run = common::cli_args(&["only.cnf".to_owned()]);
    assert_eq!(run.code, 3);
    assert!(run.stderr.starts_with("usage: refute"), "{:?}", run.stderr);
}

/// Asking for help, and nothing else, is not an error.
#[test]
fn version_and_help_alone_exit_0() {
    let version = common::cli_args(&["--version".to_owned()]);
    assert_eq!(version.code, 0);
    assert!(
        version.stdout.starts_with("refute "),
        "{:?}",
        version.stdout
    );

    let help = common::cli_args(&["--help".to_owned()]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("usage: refute"), "{:?}", help.stdout);
}

/// The documented contract is "trust the exit code". A `--help` anywhere in
/// argv used to short-circuit to 0, so `refute bad.cnf bad.lrat --help` — a
/// proof this suite proves is bad — reported success for a proof never read.
/// One stray argument in a CI script is all that takes.
#[test]
fn help_or_version_beside_other_arguments_never_exits_0() {
    let bad_formula = common::fixture("n05_no_empty_clause.cnf")
        .to_string_lossy()
        .into_owned();
    let bad_proof = common::fixture("n05_no_empty_clause.lrat")
        .to_string_lossy()
        .into_owned();

    for flag in ["--help", "-h", "--version", "-V"] {
        let run = common::cli_args(&[bad_formula.clone(), bad_proof.clone(), flag.to_owned()]);
        assert_ne!(run.code, 0, "{flag} beside a bad proof exited 0");
        assert_eq!(run.code, 3, "{flag}: stderr was {:?}", run.stderr);
        assert!(
            run.stdout.is_empty(),
            "{flag} printed a verdict: {:?}",
            run.stdout
        );
        assert!(
            run.stderr.starts_with("usage: refute"),
            "{flag}: stderr was {:?}",
            run.stderr
        );
    }
}

/// `--` ends the flags, so a file really called `--help` can still be checked.
/// Without a terminator the only way to name one is to rename it.
#[test]
fn a_double_dash_ends_the_flags() {
    let run = common::cli_args(&[
        "--".to_owned(),
        common::fixture("tiny_unsat.cnf")
            .to_string_lossy()
            .into_owned(),
        common::fixture("tiny_unsat.lrat")
            .to_string_lossy()
            .into_owned(),
    ]);
    run.assert("s VERIFIED", 0);

    let as_a_path =
        common::cli_args(&["--".to_owned(), "--help".to_owned(), "--version".to_owned()]);
    assert_eq!(as_a_path.code, 3, "stderr was {:?}", as_a_path.stderr);
    assert!(
        as_a_path.stderr.contains("cannot open '--help'"),
        "after -- a flag is a path; stderr was {:?}",
        as_a_path.stderr
    );
}

/// A verdict must survive `refute a.cnf b.lrat > log.txt`, and it must survive
/// a hostile file.
///
/// The first four cases are benign, and on their own they were decoration: no
/// fixture in the suite contained a byte that could have failed them. The last
/// two carry a real payload — `ESC [ 1 A ESC [ 2 K s VERIFIED`, which moves
/// the cursor up a line, clears it, and writes `s VERIFIED` over the top —
/// once in the formula and once in the proof, because both files are quoted
/// back by name when a token will not parse.
///
/// The assertion is on the bytes rather than on the attack: every byte out is
/// printable ASCII or a line break. `is_ascii()` alone would pass ESC, which
/// is 0x1b and entirely ASCII.
#[test]
fn output_is_plain_ascii_in_every_verdict() {
    let cases = [
        ("tiny_unsat.cnf", "tiny_unsat.lrat"),
        ("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat"),
        ("real_rat_proof.cnf", "real_rat_proof.lrat"),
        ("b06_var_over_limit.cnf", "b06_var_over_limit.lrat"),
        ("b17_binary_proof.cnf", "b17_binary_proof.lrat"),
        ("hostile_escape_formula.cnf", "hostile_escape_formula.lrat"),
        ("hostile_escape_proof.cnf", "hostile_escape_proof.lrat"),
    ];
    for (cnf, proof) in cases {
        let run = common::cli(cnf, proof);
        for (stream, text) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            for byte in text.bytes() {
                assert!(
                    matches!(byte, 0x20..=0x7e | b'\n' | b'\r'),
                    "{stream} carried byte {byte:#04x} for {cnf}: {text:?}"
                );
            }
        }
    }
}

/// The escaping is visible, not silent: the bytes are shown as `\xNN` so a
/// reader can still see what the file actually contained.
#[test]
fn an_unreadable_token_is_quoted_with_its_bytes_escaped() {
    let run = common::cli("hostile_escape_proof.cnf", "hostile_escape_proof.lrat");
    run.assert("s NOT VERIFIED", 1);
    assert!(
        run.stderr.contains("\\x1b[1A\\x1b[2Ks"),
        "the token must still be quoted, escaped; stderr was {:?}",
        run.stderr
    );
}

/// The three verdict tokens, each with its exit code, in one place. If one of
/// these strings ever changes, this is the test that says so.
#[test]
fn the_three_verdicts_and_their_exit_codes() {
    common::cli("tiny_unsat.cnf", "tiny_unsat.lrat").assert("s VERIFIED", 0);
    common::cli("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat").assert("s NOT VERIFIED", 1);
    // The third verdict's only remaining producer. It was `real_rat_proof`
    // until milestone 1b, which is the whole point: that file now verifies.
    common::cli("b17_binary_proof.cnf", "b17_binary_proof.lrat").assert("s UNSUPPORTED", 2);
}

/// `UNSUPPORTED` names the command that fixes it, not the milestone that
/// would have. A reader who is told their proof is unsupported and nothing
/// else concludes the file is fine and stops.
#[test]
fn a_binary_proof_is_told_how_to_produce_a_text_one() {
    let run = common::cli("b17_binary_proof.cnf", "b17_binary_proof.lrat");
    run.assert("s UNSUPPORTED", 2);
    assert!(
        run.stderr.contains("--no-binary"),
        "the message must name the fix; stderr was {:?}",
        run.stderr
    );
}

/// A RAT rejection carries the resolvent block it died on, because that is the
/// number a person needs in order to find the line — the step id and the proof
/// line get them to a hint list of forty tokens, and no further.
#[test]
fn a_rat_rejection_names_the_resolvent_block() {
    let run = common::cli("r02_block_dropped.cnf", "r02_block_dropped.lrat");
    run.assert("s NOT VERIFIED", 1);
    assert!(
        run.stderr.contains("resolvent block"),
        "stderr was {:?}",
        run.stderr
    );
    assert!(run.stderr.contains("step"), "stderr was {:?}", run.stderr);
    assert!(run.stderr.contains("line"), "stderr was {:?}", run.stderr);
}

/// A rejection names the step, the line and the reason, or it is not
/// actionable. `rustc`'s "expected X, found Y" is the model.
#[test]
fn a_rejection_names_where_and_why() {
    let run = common::cli(
        "n03_hint_deleted_clause.cnf",
        "n03_hint_deleted_clause.lrat",
    );
    assert_eq!(run.code, 1);
    assert!(run.stderr.starts_with("refute: "), "{:?}", run.stderr);
    assert!(run.stderr.contains("line"), "{:?}", run.stderr);
    assert!(run.stderr.contains("hint"), "{:?}", run.stderr);
}
