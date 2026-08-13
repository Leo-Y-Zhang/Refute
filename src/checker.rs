//! The forward LRAT checker.

use std::io::BufRead;

use crate::cnf::{parse_dimacs, Cnf, Warning};
use crate::limits::Limits;
use crate::lrat::LratReader;
use crate::verdict::{Reason, Rejection, Verdict};

/// Checks `proof` against `cnf`.
///
/// Total: returns a verdict for every input, including garbage. Never panics,
/// never allocates unboundedly, never reads past the first failing step.
pub fn check<R: BufRead>(cnf: &Cnf, proof: LratReader<R>, limits: &Limits) -> Verdict {
    // STUB — build order step 7 implements this. It verifies everything, which
    // is precisely what makes every negative test of step 3 discriminating.
    let _ = (cnf, proof, limits);
    Verdict::Verified
}

/// Parses a formula and checks a proof against it, in one call.
///
/// The mapping "a formula we cannot read is a proof we cannot accept" lives
/// here rather than in `main`, so that it is covered by the test suite and so
/// that the milestone-4 WASM entry point cannot accidentally differ from the
/// CLI. Warnings are returned for the caller to print; the library never does.
pub fn check_readers<F: BufRead, P: BufRead>(
    formula: F,
    proof: P,
    limits: &Limits,
) -> (Verdict, Vec<Warning>) {
    let cnf = match parse_dimacs(formula, limits) {
        Ok(cnf) => cnf,
        Err(err) => {
            let rejection = Rejection {
                step: None,
                line: 0,
                reason: Reason::Parse(err),
            };
            return (Verdict::NotVerified(rejection), Vec::new());
        }
    };
    let warnings = cnf.warnings.clone();
    let verdict = check(&cnf, LratReader::new(proof, limits), limits);
    (verdict, warnings)
}
