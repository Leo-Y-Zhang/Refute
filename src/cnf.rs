//! DIMACS CNF parsing.

use std::io::BufRead;

use crate::limits::Limits;
use crate::lit::Clause;
use crate::parse::ParseError;

/// Something odd but survivable about the formula file.
///
/// Warnings are returned, never printed: the library has no output surface, so
/// the milestone-4 WASM target does not have to route stderr anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// The `p` line understated the variable count.
    HeaderVarUndercount {
        /// The count the header declared.
        declared: u32,
        /// The largest variable actually seen.
        found: u32,
    },
    /// The `p` line disagreed with the number of clauses present.
    HeaderClauseMismatch {
        /// The count the header declared.
        declared: usize,
        /// The number of clauses actually read.
        found: usize,
    },
    /// There was no `p` line at all.
    MissingHeader,
}

impl core::fmt::Display for Warning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::HeaderVarUndercount { declared, found } => write!(
                f,
                "header declares {declared} variables but the formula uses {found}"
            ),
            Self::HeaderClauseMismatch { declared, found } => write!(
                f,
                "header declares {declared} clauses but the formula has {found}"
            ),
            Self::MissingHeader => write!(f, "formula has no 'p cnf' header line"),
        }
    }
}

/// A parsed formula. Clause identifiers are the one-based positions of
/// `clauses`, which is the convention LRAT hints refer to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cnf {
    /// The largest variable index used, which may exceed the header's claim.
    pub num_vars: u32,
    /// The clauses, in file order.
    pub clauses: Vec<Clause>,
    /// Non-fatal oddities, for the caller to report.
    pub warnings: Vec<Warning>,
}

/// Parses a DIMACS CNF formula.
///
/// # Errors
///
/// Returns [`ParseError`] for malformed input, for a literal beyond
/// [`Limits::max_var`], and for a read failure on `reader`.
pub fn parse_dimacs<R: BufRead>(reader: R, limits: &Limits) -> Result<Cnf, ParseError> {
    // STUB — build order step 5 implements this. Present now so that the
    // negative tests of step 3 compile and can be observed failing.
    let _ = (reader, limits);
    Ok(Cnf {
        num_vars: 0,
        clauses: Vec::new(),
        warnings: Vec::new(),
    })
}
