//! The three-way verdict and the reasons behind it.
//!
//! The verdict is three-way on evidence, not on taste. A measured `drat-trim -L`
//! file contained 56 addition lines with an empty hint list — valid RAT lemmas
//! whose pivot had no resolution candidates. Treating "no hints" as acceptance
//! would accept anything; running RUP on them would reject a valid proof. Both
//! are lies, so there is a third answer.
//!
//! [`Verdict::Verified`] has no `Default`, no `From<bool>` and no public
//! constructor: `checker.rs` is the only file in the library that names it, and
//! `tests/trust_boundary.rs` fails the build if that stops being true.

use core::fmt;

use crate::lit::ClauseId;
use crate::parse::ParseError;

/// The outcome of checking one proof against one formula.
///
/// `#[must_use]`, because a dropped verdict is a check that never happened.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A checked sequence of steps derived the empty clause. The only success.
    Verified,
    /// The proof was read and found wanting.
    NotVerified(Rejection),
    /// The proof uses a construct milestone 1 does not check. Not a pass.
    Unsupported(Unsupported),
}

/// Where a rejection happened and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// The step being added, when the rejection is attributable to one.
    pub step: Option<ClauseId>,
    /// One-based line number in the proof, or 0 when no line applies.
    pub line: u64,
    /// The reason.
    pub reason: Reason,
}

/// Why a proof was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Either file could not be read as its format. Fail closed.
    Parse(ParseError),
    /// A hint named a clause that is not in the database.
    MissingHint(ClauseId),
    /// A hint clause was already satisfied when it was used. In a well-formed
    /// derivation this never happens; it is what catches a valid proof of a
    /// different formula.
    HintSatisfied(ClauseId),
    /// A hint clause had two or more unassigned literals, so it is not a unit.
    HintNotUnit(ClauseId),
    /// A hint falsified the assignment before the last hint, meaning the hint
    /// list was reordered or padded.
    EarlyConflict(ClauseId),
    /// The hint list ran out without reaching a conflict.
    NoConflict,
    /// Step identifiers must strictly increase.
    ///
    /// This is also what forbids reusing an identifier: everything in the
    /// clause database is at most the last identifier added, so an id that
    /// passes this test is larger than every one present. There is no
    /// `DuplicateId` beside it, because nothing could ever produce one.
    NonMonotonicId {
        /// The identifier just read.
        got: ClauseId,
        /// The largest identifier added before it.
        previous: ClauseId,
    },
    /// The proof ended without deriving the empty clause.
    NoEmptyClause,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::MissingHint(id) => {
                write!(f, "hint {id} names a clause that is not in the database")
            }
            Self::HintSatisfied(id) => write!(f, "hint {id} is already satisfied"),
            Self::HintNotUnit(id) => write!(f, "hint {id} is not unit under the assignment"),
            Self::EarlyConflict(id) => {
                write!(f, "hint {id} conflicts before the end of the hint list")
            }
            Self::NoConflict => write!(f, "hints ran out without reaching a conflict"),
            Self::NonMonotonicId { got, previous } => {
                write!(
                    f,
                    "step id {got} does not exceed the previous id {previous}"
                )
            }
            Self::NoEmptyClause => write!(f, "proof contains no empty clause"),
        }
    }
}

/// A construct this milestone declines to check.
///
/// Reported with exit code 2 and never confusable with success. The message
/// names the next step rather than only the limitation, because a reader can
/// otherwise conclude their proof is fine and stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// A hint list containing a negative identifier: a RAT resolvent block.
    /// Measured at 2.4 % of addition lines in a real pigeonhole proof.
    RatHints {
        /// One-based line number in the proof.
        line: u64,
    },
    /// An addition with an empty hint list, such as `205 57 -29 0 0`.
    /// Measured at 2.0 % of addition lines. Neither a pass nor a corruption.
    EmptyHints {
        /// One-based line number in the proof.
        line: u64,
    },
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RatHints { line } => write!(
                f,
                "proof line {line}: RAT hint block; milestone 1 checks RUP hints only. \
                 Use drat-trim for RAT proofs until milestone 1b"
            ),
            Self::EmptyHints { line } => write!(
                f,
                "proof line {line}: addition with an empty hint list; milestone 1 checks \
                 RUP hints only. Use drat-trim for RAT proofs until milestone 1b"
            ),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.step, self.line) {
            (Some(step), 0) => write!(f, "step {step}: {}", self.reason),
            (Some(step), line) => write!(f, "step {step}, proof line {line}: {}", self.reason),
            (None, 0) => write!(f, "{}", self.reason),
            (None, line) => write!(f, "proof line {line}: {}", self.reason),
        }
    }
}
