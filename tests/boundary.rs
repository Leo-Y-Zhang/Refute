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
use refute::{Limits, ParseError, ParseErrorKind, Reason, Source, Verdict};

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

/// A reader that hands over its lines one at a time and then fails.
///
/// `BufReader::read_line` refills from the inner reader only when its own
/// buffer is empty, so one line per `read` puts the failure on a line of the
/// test's choosing. A disk or a network share failing halfway through a 200 MB
/// proof is the case being modelled, and it is the case nobody can reproduce
/// on demand.
struct FailsAfter {
    lines: Vec<&'static str>,
    next: usize,
}

impl std::io::Read for FailsAfter {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.lines.get(self.next) {
            Some(line) => {
                self.next = self.next.saturating_add(1);
                let bytes = line.as_bytes();
                buf[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            None => Err(std::io::Error::other("the device was disconnected")),
        }
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
            resolvent: None,
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

/// B14. A read that fails part-way through names the line it failed on.
///
/// Four lines are handed over and the fifth read fails, in each file in turn.
/// The line counter is incremented after a successful read, so the error used
/// to be reported one line early — a reader sent to line 4 of a file whose
/// line 4 is fine has been told something false about a 200 MB file.
#[test]
fn b14_an_io_error_names_the_line_that_failed() {
    let formula = std::io::BufReader::new(FailsAfter {
        lines: vec!["p cnf 2 2\n", "1 2 0\n", "-1 0\n", "1 0\n"],
        next: 0,
    });
    let outcome = refute::check_readers(formula, Cursor::new(&b""[..]), &Limits::default());
    match outcome.verdict {
        Verdict::NotVerified(rejection) => match rejection.reason {
            Reason::Parse(err) => {
                assert_eq!(err.source, Source::Formula);
                assert!(matches!(err.kind, ParseErrorKind::Io(_)), "{err:?}");
                assert_eq!(err.line, 5, "the failing read was of line 5");
            }
            other => panic!("expected a parse error, got {other:?}"),
        },
        other => panic!("expected a rejection, got {other:?}"),
    }

    let proof = std::io::BufReader::new(FailsAfter {
        lines: vec!["3 d 0\n"; 4],
        next: 0,
    });
    let outcome = refute::check_readers(
        Cursor::new("p cnf 2 2\n1 2 0\n-1 0\n".as_bytes()),
        proof,
        &Limits::default(),
    );
    match outcome.verdict {
        Verdict::NotVerified(rejection) => match rejection.reason {
            Reason::Parse(err) => {
                assert_eq!(err.source, Source::Proof);
                assert!(matches!(err.kind, ParseErrorKind::Io(_)), "{err:?}");
                assert_eq!(err.line, 5, "the failing read was of line 5");
            }
            other => panic!("expected a parse error, got {other:?}"),
        },
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// B15. A UTF-8 byte order mark at the start of either file.
///
/// Windows editors write one whenever a file is opened and saved, and the
/// fixtures for this project are generated on Windows. Neither format says
/// anything about it, so the decision is ours: skip it once, at the very
/// start.
///
/// Skipping cannot cause a false `VERIFIED` — the mark carries no clause, no
/// hint and no identifier — while rejecting fails a file that is otherwise
/// exactly right, and the message a reader got instead named a token they
/// could not see. A mark anywhere else is not an encoding artefact, and is
/// still an error.
#[test]
fn b15_a_leading_byte_order_mark_is_skipped() {
    const MARK: &str = "\u{feff}";
    let formula = std::fs::read_to_string(common::fixture("tiny_unsat.cnf")).unwrap();
    let proof = std::fs::read_to_string(common::fixture("tiny_unsat.lrat")).unwrap();

    for (cnf, lrat) in [
        (format!("{MARK}{formula}"), proof.clone()),
        (formula.clone(), format!("{MARK}{proof}")),
        (format!("{MARK}{formula}"), format!("{MARK}{proof}")),
    ] {
        let outcome = refute::check_readers(
            Cursor::new(cnf.as_bytes()),
            Cursor::new(lrat.as_bytes()),
            &Limits::default(),
        );
        assert_eq!(outcome.verdict, Verdict::Verified, "cnf was {cnf:?}");
    }

    let mut lines: Vec<String> = formula.lines().map(str::to_owned).collect();
    lines[1] = format!("{MARK}{}", lines[1]);
    let err = parse_error(&(lines.join("\n") + "\n"), &proof);
    assert_eq!(err.source, Source::Formula);
    assert!(
        matches!(err.kind, ParseErrorKind::NotAnInteger(_)),
        "a mark on line 2 is a corruption, not an encoding: {err:?}"
    );
    assert_eq!(err.line, 2);
}

/// B16. Nineteen bytes claiming four billion variables.
///
/// `p cnf 4294967295 1` is the whole attack. The assignment vector used to be
/// sized from the header, capped only by `Limits::max_var`, so those nineteen
/// bytes bought a 64 MB allocation before a single clause was checked — and in
/// the milestone-4 WASM target that is the heap.
///
/// The header is advisory everywhere else in this parser; it is advisory here
/// too. The vector is sized from the largest variable the formula actually
/// mentions and grows on demand, which B3 — a header that *understates* the
/// count — covers from the other side.
#[test]
fn b16_the_header_does_not_size_the_assignment_vector() {
    let outcome = refute::check_readers(
        Cursor::new("p cnf 4294967295 1\n1 0\n".as_bytes()),
        Cursor::new(&b""[..]),
        &Limits::default(),
    );
    assert!(
        matches!(outcome.verdict, Verdict::NotVerified(_)),
        "{:?}",
        outcome.verdict
    );
    assert!(
        outcome.stats.assignment_slots <= 8,
        "a formula mentioning variable 1 bought {} assignment slots",
        outcome.stats.assignment_slots
    );
}

/// B12. A whole real `drat-trim` proof containing RAT blocks.
///
/// Milestone 1 stopped on its *empty hint list*, on line 2, and printed
/// `s UNSUPPORTED`: in every instance measured — pigeonhole 5x4 through 8x7 —
/// the empty hint list comes first, because the RAT blocks resolve against
/// exactly those lemmas. Both are checked now. P9 asserts the verdict; this
/// keeps the boundary entry pointing at what changed.
#[test]
fn b12_real_proof_with_rat_verifies() {
    common::cli("real_rat_proof.cnf", "real_rat_proof.lrat").assert("s VERIFIED", 0);
    assert_eq!(
        common::verdict("real_rat_proof.cnf", "real_rat_proof.lrat"),
        Verdict::Verified
    );
}

/// B12b. A single RAT line, copied verbatim out of that same proof and stood
/// on its own against the same formula.
///
/// It was the only fixture reaching the `RatHints` path in milestone 1. It is
/// worth keeping for a better reason: its blocks name clauses 46 and 47, which
/// are lemmas of the proof it came from and do not exist when the line stands
/// alone. A RAT step is checked against the database it is in, not the one it
/// was written for — so this is a rejection, not a pass.
#[test]
fn b12b_a_rat_line_out_of_context_is_rejected() {
    let run = common::cli("b12b_rat_hints.cnf", "b12b_rat_hints.lrat");
    assert_ne!(run.code, 0, "a RAT line out of context exited 0");
    run.assert("s NOT VERIFIED", 1);
    assert!(matches!(
        common::verdict("b12b_rat_hints.cnf", "b12b_rat_hints.lrat"),
        Verdict::NotVerified(_)
    ));
}

/// B17. A real binary proof: 64 bytes of `kissat`'s binary DRAT.
///
/// The mistake is forgetting `--no-binary`, and milestone 1 reported it as a
/// corrupt proof — `expected an integer, found 'a*\x13\x00...'` — which is a
/// tool failure dressed up as a bad certificate, exactly the confusion this
/// project exists to remove. It is the one construct left that Refute declines
/// to check, so it is what keeps the third verdict honest.
#[test]
fn b17_a_binary_proof_is_unsupported_not_verified() {
    let run = common::cli("b17_binary_proof.cnf", "b17_binary_proof.lrat");
    assert_ne!(run.code, 0, "a binary proof exited 0");
    run.assert("s UNSUPPORTED", 2);
    assert!(
        matches!(
            common::verdict("b17_binary_proof.cnf", "b17_binary_proof.lrat"),
            Verdict::Unsupported(_)
        ),
        "{:?}",
        common::verdict("b17_binary_proof.cnf", "b17_binary_proof.lrat")
    );
}

/// B17b. The guard on the one weakening in the crate.
///
/// Exactly one parse error kind becomes `Unsupported` and exit 2; every other
/// one stays a rejection and exit 1. The mapping is a rejection turned into a
/// non-rejection, so it is the place a corrupt proof could learn to look like
/// an unsupported one.
#[test]
fn b17b_only_a_binary_proof_becomes_unsupported() {
    let formula = "p cnf 2 2\n1 0\n-1 0\n";
    for proof in [
        "3 1 0 1\n",              // no terminator
        "3 1 0 1 0 9\n",          // trailing tokens
        "not-an-integer 0 0\n",   // not an integer
        "99999999999999999999\n", // overflow
        "-1 1 0 1 0\n",           // a step id that is not positive
        "3 100000000 0 1 0\n",    // a variable past the ceiling
    ] {
        let outcome = refute::check_readers(
            Cursor::new(formula.as_bytes()),
            Cursor::new(proof.as_bytes()),
            &Limits::default(),
        );
        assert!(
            matches!(outcome.verdict, Verdict::NotVerified(_)),
            "{proof:?} produced {:?}",
            outcome.verdict
        );
    }
}

/// B18. A hint list of resolvent block markers, longer than the ceiling.
///
/// The ceiling bounds the *whole* hint list — prefix, block markers and block
/// hints together — because a line of ten million one-hint blocks allocates
/// exactly as much as a ten-million-hint list does. Milestone 1 discarded the
/// negative tokens without counting them, so this line was unbounded.
#[test]
fn b18_block_markers_count_against_the_hint_ceiling() {
    let limits = Limits {
        max_clause_len: 4,
        ..Limits::default()
    };
    let outcome = refute::check_readers(
        Cursor::new("p cnf 3 3\n1 0\n-1 2 0\n-2 0\n".as_bytes()),
        Cursor::new("4 1 0 -1 -2 -3 -1 -2 0\n".as_bytes()),
        &limits,
    );
    match outcome.verdict {
        Verdict::NotVerified(rejection) => match rejection.reason {
            Reason::Parse(err) => assert_eq!(
                err.kind,
                ParseErrorKind::ListTooLong {
                    limit: limits.max_clause_len
                }
            ),
            other => panic!("expected a parse error, got {other:?}"),
        },
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// B19. A RAT lemma whose first literal is written twice: `1 1 3`.
///
/// The pivot is the first literal, and assigning a literal twice is
/// idempotent, so the repeat changes nothing. The formula and the hints are
/// `resolvent_propagates`'s, whose lemma sequence `drat-trim` verifies.
#[test]
fn b19_a_repeated_pivot_is_idempotent() {
    let formula = std::fs::read_to_string(common::fixture("resolvent_propagates.cnf")).unwrap();
    let outcome = refute::check_readers(
        Cursor::new(formula.as_bytes()),
        Cursor::new("7 1 1 3 0 -1 2 3 4 0\n8 0 5 6 7 1 0\n".as_bytes()),
        &Limits::default(),
    );
    assert_eq!(outcome.verdict, Verdict::Verified);
}

/// B20. A hint list that opens with a resolvent block: the prefix is empty.
///
/// Never observed in real output, which always propagates something before it
/// resolves, but it is legal and harmless — the block simply starts from the
/// negated lemma alone. Strictness here would forbid a shape no measurement
/// condemns. `resolvent_propagates` is already this shape, and the first
/// assertion is on its bytes so that a regenerated fixture cannot quietly stop
/// being the test.
#[test]
fn b20_a_rat_line_may_have_an_empty_prefix() {
    let proof = std::fs::read_to_string(common::fixture("resolvent_propagates.lrat")).unwrap();
    let rat_line = proof
        .lines()
        .find(|line| line.split_whitespace().any(|t| t.starts_with('-')))
        .expect("the fixture should carry a resolvent block");
    let first_hint = rat_line
        .split_whitespace()
        .skip_while(|t| *t != "0")
        .nth(1)
        .expect("the fixture's RAT line should have a hint list");
    assert!(
        first_hint.starts_with('-'),
        "the prefix is not empty: {rat_line:?}"
    );
    assert_eq!(
        common::verdict("resolvent_propagates.cnf", "resolvent_propagates.lrat"),
        Verdict::Verified
    );
}

/// B21. A resolvent block naming clause id 0.
///
/// It cannot be written. `-0` scans as zero, not as a negative, so it is read
/// as a hint identifier and rejected there — which is the fail-closed answer
/// and the one milestone 1 already gave. `docs/TDD.md` part 2 asks for
/// `NotAResolutionCandidate` in one row and for a parse error in another; the
/// parse error is what the grammar actually produces, and asserting the other
/// would be asserting something untrue.
#[test]
fn b21_a_block_cannot_name_clause_zero() {
    let err = parse_error("p cnf 2 2\n1 0\n-1 0\n", "3 1 0 -0 0\n");
    assert_eq!(err.source, Source::Proof);
    assert_eq!(
        err.kind,
        ParseErrorKind::NonPositiveClauseId("-0".to_owned())
    );
    assert_eq!(err.line, 1);
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
