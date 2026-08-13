//! Streaming LRAT proof parsing.
//!
//! The reader yields one step at a time and never holds the file. A 200 MB
//! proof is read in constant memory; only the clause database grows.

use std::io::BufRead;

use crate::limits::Limits;
use crate::lit::{ClauseId, Lit};
use crate::parse::ParseError;

/// The hint list of an addition step, classified before anything is checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hints {
    /// Every hint is a positive identifier: a RUP derivation. 96.0 % of
    /// addition lines in the measured corpus.
    Rup(Vec<ClauseId>),
    /// At least one negative identifier: a RAT resolvent block. 2.4 %.
    Rat,
    /// No hints at all, as in `205 57 -29 0 0`. 2.0 %, and neither a pass nor
    /// a corruption — see [`crate::verdict::Unsupported`].
    Empty,
}

/// One line of an LRAT proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// An addition: a lemma, its identifier and its hints.
    Add {
        /// The lemma's identifier.
        id: ClauseId,
        /// The lemma's literals. Empty means the empty clause.
        lits: Vec<Lit>,
        /// The hint list.
        hints: Hints,
        /// One-based line number.
        line: u64,
    },
    /// A deletion. An empty list is legal and occurs in real files.
    Delete {
        /// The identifiers to remove.
        ids: Vec<ClauseId>,
        /// One-based line number.
        line: u64,
    },
}

/// A streaming reader over an LRAT proof.
pub struct LratReader<R: BufRead> {
    reader: R,
    limits: Limits,
    line: u64,
    finished: bool,
}

impl<R: BufRead> LratReader<R> {
    /// Wraps a reader.
    pub fn new(reader: R, limits: &Limits) -> Self {
        Self {
            reader,
            limits: *limits,
            line: 0,
            finished: false,
        }
    }
}

impl<R: BufRead> Iterator for LratReader<R> {
    type Item = Result<Step, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        // STUB — build order step 6 implements this. Present now so that the
        // negative tests of step 3 compile and can be observed failing.
        let _ = (&mut self.reader, &self.limits, self.line);
        self.finished = true;
        None
    }
}
