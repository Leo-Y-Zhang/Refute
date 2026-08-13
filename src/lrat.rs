//! Streaming LRAT proof parsing.
//!
//! The reader yields one step at a time and never holds the file. A 200 MB
//! proof is read in constant memory; only the clause database grows.
//!
//! Parsing is line-oriented, unlike the formula parser. LRAT is
//! whitespace-delimited on paper, but a step that runs off the end of its line
//! would then quietly absorb the next step's identifier as a literal, and a
//! truncated proof would mis-parse rather than fail. One step per line, and a
//! line that does not terminate is an error.

use std::io::BufRead;

use crate::cnf::io_kind;
use crate::limits::Limits;
use crate::lit::{ClauseId, Lit};
use crate::parse::{scan_i64, scan_id, scan_lit, ParseError, ParseErrorKind, Source};

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
    ///
    /// The leading number on a deletion line is the identifier of the most
    /// recent addition, not a clause to delete, and is discarded.
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
    buffer: String,
}

impl<R: BufRead> LratReader<R> {
    /// Wraps a reader.
    pub fn new(reader: R, limits: &Limits) -> Self {
        Self {
            reader,
            limits: *limits,
            line: 0,
            finished: false,
            buffer: String::new(),
        }
    }

    fn fail(&mut self, kind: ParseErrorKind) -> Option<Result<Step, ParseError>> {
        self.finished = true;
        Some(Err(ParseError::new(Source::Proof, self.line, kind)))
    }
}

impl<R: BufRead> Iterator for LratReader<R> {
    type Item = Result<Step, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.finished {
                return None;
            }
            self.buffer.clear();
            match self.reader.read_line(&mut self.buffer) {
                Ok(0) => {
                    self.finished = true;
                    return None;
                }
                Ok(_) => {}
                Err(err) => {
                    let kind = io_kind(&err);
                    return self.fail(kind);
                }
            }
            self.line = self.line.saturating_add(1);
            let line = core::mem::take(&mut self.buffer);
            if line.trim().is_empty() {
                self.buffer = line;
                continue;
            }
            let parsed = parse_step(line.trim(), self.line, &self.limits);
            self.buffer = line;
            return match parsed {
                Ok(step) => Some(Ok(step)),
                Err(kind) => self.fail(kind),
            };
        }
    }
}

fn parse_step(line: &str, line_no: u64, limits: &Limits) -> Result<Step, ParseErrorKind> {
    let mut tokens = line.split_ascii_whitespace();
    let id_token = tokens.next().ok_or(ParseErrorKind::MissingTerminator)?;
    let id = scan_id(id_token)?;

    let mut lookahead = tokens.clone();
    if lookahead.next() == Some("d") {
        let mut ids = Vec::new();
        let mut terminated = false;
        for token in lookahead.by_ref() {
            if token == "0" {
                terminated = true;
                break;
            }
            push_bounded(&mut ids, scan_id(token)?, limits)?;
        }
        if !terminated {
            return Err(ParseErrorKind::MissingTerminator);
        }
        // Deletion is permissive about *which* identifiers it is handed, which
        // is sound because deleting only removes tools from the checker. The
        // shape of the line is a different question: an addition has always
        // rejected tokens after its terminator, and a parser that disagrees
        // with itself about where a step ends is reading a file nobody wrote.
        if let Some(extra) = lookahead.next() {
            return Err(ParseErrorKind::TrailingTokens(extra.to_owned()));
        }
        return Ok(Step::Delete { ids, line: line_no });
    }

    let mut lits: Vec<Lit> = Vec::new();
    let mut terminated = false;
    for token in tokens.by_ref() {
        if token == "0" {
            terminated = true;
            break;
        }
        push_bounded(&mut lits, scan_lit(token, limits)?, limits)?;
    }
    if !terminated {
        return Err(ParseErrorKind::MissingTerminator);
    }

    let mut positive: Vec<ClauseId> = Vec::new();
    let mut any_negative = false;
    terminated = false;
    for token in tokens.by_ref() {
        if token == "0" {
            terminated = true;
            break;
        }
        let value = scan_i64(token)?;
        if value < 0 {
            // A RAT resolvent block. The remaining hints are still scanned for
            // well-formedness, but their values are not kept: milestone 1 does
            // not check RAT, and a half-understood hint list is worse than none.
            any_negative = true;
        } else {
            push_bounded(&mut positive, scan_id(token)?, limits)?;
        }
    }
    if !terminated {
        return Err(ParseErrorKind::MissingTerminator);
    }
    if let Some(extra) = tokens.next() {
        return Err(ParseErrorKind::TrailingTokens(extra.to_owned()));
    }

    let hints = if any_negative {
        Hints::Rat
    } else if positive.is_empty() {
        Hints::Empty
    } else {
        Hints::Rup(positive)
    };
    Ok(Step::Add {
        id,
        lits,
        hints,
        line: line_no,
    })
}

fn push_bounded<T>(target: &mut Vec<T>, value: T, limits: &Limits) -> Result<(), ParseErrorKind> {
    if target.len() >= limits.max_clause_len {
        return Err(ParseErrorKind::ListTooLong {
            limit: limits.max_clause_len,
        });
    }
    target.push(value);
    Ok(())
}
