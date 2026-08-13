//! The command line contract.
//!
//! Exit codes and output strings are the interface a CI step depends on, so
//! they are asserted literally. Anything grepping for `VERIFIED` alone would
//! also match `NOT VERIFIED`; the documented test is on the exit code, and both
//! are checked here.

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

#[test]
fn version_and_help_exit_0() {
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

/// A verdict must survive `refute a.cnf b.lrat > log.txt`. No colour, no
/// Unicode, no emoji, no escape sequences — checked on the bytes, across all
/// three verdicts, because the register of this tool is part of its design.
#[test]
fn output_is_plain_ascii_in_every_verdict() {
    let cases = [
        ("tiny_unsat.cnf", "tiny_unsat.lrat"),
        ("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat"),
        ("real_rat_proof.cnf", "real_rat_proof.lrat"),
        ("b06_var_over_limit.cnf", "b06_var_over_limit.lrat"),
    ];
    for (cnf, proof) in cases {
        let run = common::cli(cnf, proof);
        for (stream, text) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            assert!(
                text.is_ascii(),
                "{stream} was not ASCII for {proof}: {text:?}"
            );
            assert!(
                !text.contains('\u{1b}'),
                "{stream} contained an escape sequence for {proof}"
            );
        }
    }
}

/// The three verdict tokens, each with its exit code, in one place. If one of
/// these strings ever changes, this is the test that says so.
#[test]
fn the_three_verdicts_and_their_exit_codes() {
    common::cli("tiny_unsat.cnf", "tiny_unsat.lrat").assert("s VERIFIED", 0);
    common::cli("n05_no_empty_clause.cnf", "n05_no_empty_clause.lrat").assert("s NOT VERIFIED", 1);
    common::cli("real_rat_proof.cnf", "real_rat_proof.lrat").assert("s UNSUPPORTED", 2);
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
