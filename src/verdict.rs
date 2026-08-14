//! The three-way verdict and the reasons behind it.
//!
//! The verdict is three-way on evidence, not on taste. In milestone 1 the
//! third answer was for the RAT step, which that milestone did not check;
//! milestone 1b checks it, and the third answer would have become decoration
//! had nothing real been left for it. Something is: `kissat` writes binary
//! DRAT unless it is told otherwise, and a binary file handed to a text
//! checker is neither a pass nor a bad certificate. It is the wrong file, and
//! saying so is the whole reason this project exists.
//!
//! [`Verdict::Verified`] has no `Default`, no `From<bool>` and no public
//! constructor. Until milestone 2 the guard on that was "`checker.rs` is the
//! only file that names it"; there are now two checkers with a legitimate
//! claim to have derived the empty clause, and **two checkers must not mean
//! two doors**. So the variant is constructed in exactly one place — [`verified`],
//! below — and what moved to two places is the *evidence* it demands.
//! `tests/trust_boundary.rs` counts both, and fails the build if either count
//! changes.

use core::fmt;

use crate::lit::{ClauseId, Lit};
use crate::parse::ParseError;

/// Evidence that a checked step derived the empty clause.
///
/// Carries nothing. Its whole value is that it cannot be conjured: the field
/// is `()` so there is no data to invent, and every construction of it is a
/// literal `EmptyClauseDerived(())` that a grep over the library finds. A
/// checker builds one immediately after the step that added the empty clause
/// returned `Ok`, and hands it to [`verified`], which is the only function in
/// this crate that produces [`Verdict::Verified`].
///
/// Deliberately not `Clone` and not `Copy`: a witness that could be duplicated
/// could be kept from an earlier run and presented again.
pub(crate) struct EmptyClauseDerived(pub(crate) ());

/// The one function in the library that produces [`Verdict::Verified`].
///
/// Takes the witness by value and drops it. There is no other route to the
/// variant, in either checker, at any milestone. `Verdict` is itself
/// `#[must_use]`, so this function inherits it.
pub(crate) fn verified(_: EmptyClauseDerived) -> Verdict {
    Verdict::Verified
}

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
    /// The resolvent block being checked, when the rejection happened inside
    /// one. A RAT step's hint list can be forty tokens long, and the step id
    /// and the line number get a reader to the line and no further.
    pub resolvent: Option<ClauseId>,
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
    /// A RAT-shaped step whose lemma has no literals.
    ///
    /// The pivot is the lemma's first literal, so an empty lemma has none, so
    /// the RAT condition cannot be evaluated at all. Fail closed: a checker
    /// that accepts `9999 0 0` accepts a bare empty clause on no evidence.
    RatWithoutPivot,
    /// A live clause holding the negated pivot that no resolvent block covers.
    ///
    /// The candidate set is computed by the checker from its own database and
    /// never read from the file, so this is the file failing to account for
    /// something the checker found, not the other way round.
    MissingResolvent {
        /// The pivot, as written in the file.
        pivot: Lit,
    },
    /// A resolvent block naming something that is not an uncovered live clause
    /// holding the negated pivot: a deleted clause, a clause the pivot has
    /// nothing to do with, or one an earlier block already covered.
    NotAResolutionCandidate {
        /// The pivot, as written in the file.
        pivot: Lit,
    },
    /// A resolvent block carrying hints that can never be reached, because the
    /// negation of its own resolvent already refutes it. Padding, and real
    /// output never does it — the same argument as [`Reason::EarlyConflict`].
    ResolventFalsifiedEarly,
    /// The hint prefix of a RAT step reached a conflict on its own, so the
    /// lemma is RUP and the blocks that follow are unreachable.
    ///
    /// Sound to accept — a RUP lemma is a fine lemma — and rejected on the
    /// same evidence and the same reasoning as [`Reason::EarlyConflict`]: real
    /// `drat-trim` output never does it, on 439 measured lines carrying
    /// blocks. It is the one new rule with a plausible false-rejection risk
    /// against a different producer, which is why it has a code of its own.
    RatLemmaIsRup(ClauseId),
    /// A live clause holding the negated pivot whose resolvent with the lemma
    /// is not implied by unit propagation. The DRAT path's one new rejection.
    ///
    /// The candidate is named by [`Rejection::resolvent`], in the checker's own
    /// numbering: originals are `1..n` in file order and lemmas continue from
    /// `n + 1`, which is exactly the LRAT numbering a reader already knows.
    ///
    /// There is one of these and not four. Milestone 1b needed
    /// [`Reason::MissingResolvent`], [`Reason::NotAResolutionCandidate`],
    /// [`Reason::ResolventFalsifiedEarly`] and [`Reason::RatLemmaIsRup`] to
    /// police a hint list that claimed to name the candidate set exactly. Here
    /// there is no claim to police: the checker enumerates the candidates
    /// itself, so the enumeration *is* the check, and the four strictness
    /// rules that carry a false-rejection risk against another producer do not
    /// exist on this path at all.
    RatCheckFailed {
        /// The pivot: the lemma's first literal, as written in the file.
        pivot: Lit,
    },
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
            Self::RatWithoutPivot => write!(f, "a lemma with no literals has no pivot"),
            Self::MissingResolvent { pivot } => write!(
                f,
                "no resolvent block for a live clause holding {}, the negation of pivot {pivot}",
                pivot.negate()
            ),
            Self::NotAResolutionCandidate { pivot } => write!(
                f,
                "the block names no uncovered live clause holding {}, \
                 the negation of pivot {pivot}",
                pivot.negate()
            ),
            Self::ResolventFalsifiedEarly => write!(
                f,
                "the resolvent is refuted by its own negation, so its hints are unreachable"
            ),
            Self::RatLemmaIsRup(id) => write!(
                f,
                "hint {id} conflicts before the resolvent blocks, so the lemma is RUP"
            ),
            Self::RatCheckFailed { pivot } => write!(
                f,
                "the resolvent on pivot {pivot} is not implied by unit propagation"
            ),
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
    /// The proof file is binary, not text LRAT.
    ///
    /// `kissat` writes binary DRAT unless it is told `--no-binary`, so this is
    /// a mistake a user makes rather than a construct nobody meets. Reporting
    /// it as a corrupt proof — which is what happened before — is a tool
    /// failure dressed up as a bad certificate.
    BinaryProof {
        /// One-based line number in the proof. Always 1.
        line: u64,
    },
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinaryProof { line } => write!(
                f,
                "proof line {line}: this is a binary proof; refute reads text LRAT. \
                 Re-run kissat with --no-binary, then drat-trim with -L"
            ),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.step, self.line) {
            (Some(step), 0) => write!(f, "step {step}")?,
            (Some(step), line) => write!(f, "step {step}, proof line {line}")?,
            (None, 0) => return write!(f, "{}", self.reason),
            (None, line) => write!(f, "proof line {line}")?,
        }
        if let Some(resolvent) = self.resolvent {
            write!(f, ", resolvent block {resolvent}")?;
        }
        write!(f, ": {}", self.reason)
    }
}
