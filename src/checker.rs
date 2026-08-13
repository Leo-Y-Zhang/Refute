//! The forward LRAT checker.

use std::io::BufRead;

use crate::cnf::Cnf;
use crate::limits::Limits;
use crate::lrat::LratReader;
use crate::verdict::Verdict;

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
