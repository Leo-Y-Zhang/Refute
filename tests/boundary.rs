//! B1–B13: the edges.
//!
//! The null case in this project is the empty case, and there are four of them,
//! all of which occur in real files: an empty clause in the formula, an empty
//! deletion list, an empty hint list, and the empty clause as the final lemma.

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

use refute::cnf::Warning;
use refute::{Limits, ParseError, ParseErrorKind, Reason, Source, Unsupported, Verdict};

fn parse_error_kind(cnf: &str, proof: &str) -> ParseErrorKind {
    match common::verdict(cnf, proof) {
        Verdict::NotVerified(rejection) => match rejection.reason {
            Reason::Parse(err) => err.kind,
            other => panic!("expected a parse error, got {other:?}"),
        },
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// The same, on text held here rather than in a fixture. A fixture earns its
/// place by carrying provenance — a solver ran, and these bytes came out. Two
/// lines of malformed DIMACS have none to carry.
fn parse_error(cnf: &str, proof: &str) -> ParseError {
    let outcome = refute::check_readers(
        Cursor::new(cnf.as_bytes()),
        Cursor::new(proof.as_bytes()),
        &Limits::default(),
    );
    match outcome.verdict {
        Verdict::NotVerified(rejection) => match rejection.reason {
            Reason::Parse(err) => err,
            other => panic!("expected a parse error, got {other:?}"),
        },
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// B1. A proof file of zero bytes. Not a crash, not a pass.
#[test]
fn b1_empty_proof_file() {
    assert_eq!(
        common::verdict("b01_empty_proof.cnf", "b01_empty_proof.lrat"),
        Verdict::NotVerified(refute::Rejection {
            step: None,
            line: 0,
            reason: Reason::NoEmptyClause,
        })
    );
    common::cli("b01_empty_proof.cnf", "b01_empty_proof.lrat").assert("s NOT VERIFIED", 1);
}

/// B2. An empty formula and an empty proof. Nothing is derivable from nothing.
#[test]
fn b2_empty_cnf_and_empty_proof() {
    assert!(matches!(
        common::verdict("b02_empty_cnf.cnf", "b02_empty_cnf.lrat"),
        Verdict::NotVerified(r) if r.reason == Reason::NoEmptyClause
    ));
    common::cli("b02_empty_cnf.cnf", "b02_empty_cnf.lrat").assert("s NOT VERIFIED", 1);
}

/// B3. The header understates the variable count. Parse, grow, warn, and let
/// the verdict be unaffected — the header is a hint, not a contract.
#[test]
fn b3_header_understates_variables() {
    let (verdict, warnings) =
        common::checked("b03_header_undercount.cnf", "b03_header_undercount.lrat");
    assert_eq!(verdict, Verdict::Verified);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, Warning::HeaderVarUndercount { .. })),
        "no undercount warning: {warnings:?}"
    );
    let run = common::cli("b03_header_undercount.cnf", "b03_header_undercount.lrat");
    run.assert("s VERIFIED", 0);
    assert!(
        run.stderr.contains("warning"),
        "stderr was {:?}",
        run.stderr
    );
}

/// B4. The header overstates the clause count. Clause ids come from position,
/// so the count is advisory.
#[test]
fn b4_header_overstates_clauses() {
    let (verdict, warnings) =
        common::checked("b04_header_overcount.cnf", "b04_header_overcount.lrat");
    assert_eq!(verdict, Verdict::Verified);
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, Warning::HeaderClauseMismatch { .. })),
        "no clause-count warning: {warnings:?}"
    );
}

/// B5. An integer no machine word holds. A parse error, and specifically not a
/// panic and not an allocation.
#[test]
fn b5_literal_beyond_any_integer() {
    assert!(matches!(
        parse_error_kind("b05_huge_literal.cnf", "b05_huge_literal.lrat"),
        ParseErrorKind::IntegerOverflow(_)
    ));
    common::cli("b05_huge_literal.cnf", "b05_huge_literal.lrat").assert("s NOT VERIFIED", 1);
}

/// B6. A literal past the variable ceiling. Without this guard the file chooses
/// the checker's allocation size, and `2000000000` is a two-gigabyte one.
#[test]
fn b6_variable_beyond_limit_names_the_limit() {
    assert_eq!(
        parse_error_kind("b06_var_over_limit.cnf", "b06_var_over_limit.lrat"),
        ParseErrorKind::VarExceedsLimit {
            var: 100_000_000,
            limit: Limits::default().max_var,
        }
    );
    let run = common::cli("b06_var_over_limit.cnf", "b06_var_over_limit.lrat");
    run.assert("s NOT VERIFIED", 1);
    assert!(
        run.stderr.contains("67108864"),
        "the limit must be actionable; stderr was {:?}",
        run.stderr
    );
}

/// B7. One clause across five lines with comments interleaved. DIMACS is
/// whitespace-delimited, not line-oriented, whatever every generator happens
/// to emit.
#[test]
fn b7_clause_split_across_lines() {
    assert_eq!(
        common::verdict("b07_split_clause.cnf", "b07_split_clause.lrat"),
        Verdict::Verified
    );
}

/// B8. CRLF throughout both files. The fixtures are generated on Windows; the
/// alternative to this test is finding out from a stranger's bug report.
#[test]
fn b8_crlf_line_endings() {
    assert_eq!(
        common::verdict("b08_crlf.cnf", "b08_crlf.lrat"),
        Verdict::Verified
    );
    common::cli("b08_crlf.cnf", "b08_crlf.lrat").assert("s VERIFIED", 0);
}

/// B9. The terminator missing from the last step. Fail closed: a file we cannot
/// read is a proof we cannot accept.
#[test]
fn b9_missing_terminator() {
    let kind = parse_error_kind("b09_missing_terminator.cnf", "b09_missing_terminator.lrat");
    assert!(
        matches!(
            kind,
            ParseErrorKind::MissingTerminator | ParseErrorKind::UnexpectedEof
        ),
        "expected a terminator error, got {kind:?}"
    );
    common::cli("b09_missing_terminator.cnf", "b09_missing_terminator.lrat")
        .assert("s NOT VERIFIED", 1);
}

/// B9b. The *formula* ending in the middle of a clause, which B9 covers only
/// for the proof.
///
/// The parser already failed closed here; nothing asserted it, so deleting the
/// check that does it broke no test. A formula whose last clause has no
/// terminator is a truncated download, and half a clause is not a formula.
#[test]
fn b9b_formula_ending_mid_clause_is_rejected() {
    let err = parse_error("p cnf 2 2\n1 2 0\n1 2", "");
    assert_eq!(err.source, Source::Formula);
    assert_eq!(err.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(
        err.line, 3,
        "the error names the line the unterminated clause began on"
    );
}

/// B10. A deletion of an id never added. Accepted deliberately: deletion only
/// removes tools from the checker, so a spurious one can cause a later
/// `MissingHint` but can never cause a false `VERIFIED`. Being strict here
/// would reject other producers' output for no safety gain.
#[test]
fn b10_deletion_of_unknown_id_is_accepted() {
    assert_eq!(
        common::verdict("b10_unknown_deletion.cnf", "b10_unknown_deletion.lrat"),
        Verdict::Verified
    );
}

/// B11. The same id deleted twice. Same argument as B10.
#[test]
fn b11_double_deletion_is_accepted() {
    assert_eq!(
        common::verdict("b11_double_deletion.cnf", "b11_double_deletion.lrat"),
        Verdict::Verified
    );
}

/// B11b. Tokens after the `0` that ends a deletion line.
///
/// An addition has always rejected them. A deletion accepted anything after
/// its terminator, so `9 d 1 0 2` silently kept clause 2 alive — the parser
/// disagreeing with itself about what a step is. Deletion is permissive about
/// *which* identifiers it is given, deliberately and soundly; that is not a
/// reason to be permissive about the shape of the line carrying them.
#[test]
fn b11b_tokens_after_a_deletion_terminator_are_rejected() {
    let err = parse_error("p cnf 2 2\n1 0\n-1 0\n", "3 d 1 0 2\n");
    assert_eq!(err.source, Source::Proof);
    assert_eq!(err.kind, ParseErrorKind::TrailingTokens("2".to_owned()));
    assert_eq!(err.line, 1);
}

/// B12. A whole real `drat-trim` proof containing RAT blocks.
///
/// It reports the *empty hint list*, not the RAT block, because in every
/// instance measured — pigeonhole 5x4 through 8x7 — the first unsupported
/// construct is an empty hint list on line 2, and the RAT blocks resolve
/// against exactly those lemmas. What the test locks down is the part that
/// matters: exit 2, and explicitly not exit 0.
#[test]
fn b12_real_proof_with_rat_is_unsupported_not_verified() {
    let run = common::cli("real_rat_proof.cnf", "real_rat_proof.lrat");
    assert_ne!(run.code, 0, "a proof we cannot check exited 0");
    run.assert("s UNSUPPORTED", 2);
    assert!(matches!(
        common::verdict("real_rat_proof.cnf", "real_rat_proof.lrat"),
        Verdict::Unsupported(Unsupported::EmptyHints { line: 2 })
    ));
}

/// B12b. A single RAT resolvent block, copied verbatim from that same proof.
/// Without it the `RatHints` path is never reached by any real file.
#[test]
fn b12b_rat_hint_block_is_unsupported_not_verified() {
    let run = common::cli("b12b_rat_hints.cnf", "b12b_rat_hints.lrat");
    assert_ne!(run.code, 0, "a RAT block exited 0");
    run.assert("s UNSUPPORTED", 2);
    assert!(matches!(
        common::verdict("b12b_rat_hints.cnf", "b12b_rat_hints.lrat"),
        Verdict::Unsupported(Unsupported::RatHints { line: 1 })
    ));
    assert!(
        run.stderr.contains("drat-trim"),
        "the message must name the way forward; stderr was {:?}",
        run.stderr
    );
}

/// B13. 100,000 variables, 50,000 steps, generated here rather than committed.
///
/// The thing it catches is a trail unwound by clearing the assignment vector:
/// O(vars) per step, five billion byte writes for this input, where the
/// specified O(assigned) unwind touches about two entries per step.
///
/// **Corrected after the build.** The assertion used to be a wall clock —
/// under 20 seconds — which the defect it describes passes comfortably in a
/// release build and fails only in a debug one. A test whose verdict depends
/// on the profile is not a test. The counters are exact, identical in both
/// profiles, and say the two things that matter: every assignment was undone
/// one at a time, and the total work is proportional to the assignments rather
/// than to the variables.
#[test]
fn b13_large_formula_unwinds_in_proportion_to_assignments() {
    const CHAIN: u32 = 50_000;
    const UNUSED_VAR: u32 = 100_000;

    let mut formula = String::from("p cnf 100000 50002\n");
    formula.push_str("1 0\n");
    for i in 1..CHAIN {
        formula.push_str(&format!("-{} {} 0\n", i, i + 1));
    }
    formula.push_str(&format!("-{CHAIN} 0\n"));
    formula.push_str(&format!("{UNUSED_VAR} 0\n"));

    // Clause ids: the chain occupies 1..=50001, the unused variable 50002.
    let num_clauses: u64 = 50_002;
    let mut proof = String::new();
    let mut previous: u64 = 1;
    for k in 2..=u64::from(CHAIN) {
        let id = num_clauses + k - 1;
        proof.push_str(&format!("{id} {k} 0 {previous} {k} 0\n"));
        previous = id;
    }
    proof.push_str(&format!(
        "{} 0 {} {} 0\n",
        num_clauses + u64::from(CHAIN),
        previous,
        CHAIN + 1
    ));

    let outcome = refute::check_readers(
        Cursor::new(formula.as_bytes()),
        Cursor::new(proof.as_bytes()),
        &Limits::default(),
    );

    assert_eq!(outcome.verdict, Verdict::Verified);
    // Every assignment undone exactly once. An unwind that cleared the
    // assignment vector would undo far more than it ever assigned, or, if it
    // cleared without counting, nothing at all: either way, not this.
    assert_eq!(
        outcome.stats.assignments_undone, outcome.stats.assignments,
        "the trail was not unwound assignment by assignment"
    );
    // Two assignments per step, not one per variable. Clearing the vector
    // instead would be 50,000 x 100,001 writes.
    let ceiling = u64::from(CHAIN).saturating_mul(4);
    assert!(
        outcome.stats.assignments <= ceiling && outcome.stats.assignments > 0,
        "{} assignments over {CHAIN} steps is not O(assigned) per step",
        outcome.stats.assignments
    );
}
