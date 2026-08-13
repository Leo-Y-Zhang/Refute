//! Literals, clauses and clause identifiers.

use core::fmt;

/// A DIMACS literal: a non-zero signed variable index.
///
/// The sign convention of the input file is preserved exactly, so error
/// messages can quote the literal the user wrote. `i32::MIN` is rejected at
/// construction, which is what makes [`Lit::negate`] total.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Lit(i32);

impl Lit {
    /// Builds a literal, or `None` if `raw` is zero or `i32::MIN`.
    ///
    /// Zero is the DIMACS terminator and is never a literal. `i32::MIN` is
    /// excluded because it has no negation in `i32`; excluding it here is the
    /// reason no arithmetic below can overflow.
    #[must_use]
    pub fn new(raw: i32) -> Option<Self> {
        if raw == 0 || raw == i32::MIN {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// The literal as written in the file.
    #[must_use]
    pub fn get(self) -> i32 {
        self.0
    }

    /// The underlying variable index, always at least 1.
    #[must_use]
    pub fn var(self) -> u32 {
        self.0.unsigned_abs()
    }

    /// True when the literal is negated in the file (`-7`).
    #[must_use]
    pub fn is_negated(self) -> bool {
        self.0 < 0
    }

    /// The complement of this literal. Total, because `i32::MIN` cannot exist.
    #[must_use]
    pub fn negate(self) -> Self {
        match self.0.checked_neg() {
            Some(n) => Self(n),
            // Unreachable: `new` rejects `i32::MIN`. Returning `self` rather
            // than panicking keeps the library's no-panic property structural.
            None => self,
        }
    }
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An immutable clause. Boxed because clauses are never resized once stored.
pub type Clause = Box<[Lit]>;

/// An LRAT clause identifier.
///
/// Identifiers are strictly increasing but sparse: a measured `drat-trim -L`
/// run produced 2,873 lemmas spanning ids 205 to 3571, which is why the clause
/// database is a map and not a vector.
pub type ClauseId = u64;
