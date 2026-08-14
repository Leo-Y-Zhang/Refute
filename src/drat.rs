//! Streaming DRAT proof parsing.
//!
//! The format the solver writes, and the shortest grammar in this project: a
//! step is a clause, `d` opens a deletion, and there is no identifier and no
//! hint list. Everything milestone 1's reader does to police a hint list has no
//! counterpart here, because there is nothing to police — which is also why the
//! checker on the other side of it has to do so much more work.
//!
//! The reader yields one step at a time and never holds the file, under the
//! same [`Limits::max_line_bytes`] ceiling and for the same measured reason:
//! `read_line` buffers a whole line before any ceiling can apply to what is in
//! it, so a proof written on one line is bounded by nothing else. Raw DRAT is
//! an order of magnitude larger than the trimmed LRAT of the same refutation —
//! 2.5 MB against 372 KB on the A217058 a(4) rung — so the promise to stream
//! matters more here, not less.
//!
//! Line-oriented, for milestone 1's reason: a step that ran off the end of its
//! line would absorb the next step's literals, and a truncated proof would
//! mis-parse rather than fail. It is also half of what makes the two grammars
//! disjoint, so relaxing it would weaken format detection as well.

pub(crate) mod checker;

use std::io::{BufRead, Read};

use crate::cnf::io_kind;
use crate::limits::Limits;
use crate::lit::Lit;
use crate::parse::{
    push_bounded, scan_lit, strip_byte_order_mark, ParseError, ParseErrorKind, Source,
};

/// One line of a DRAT proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DratStep {
    /// An addition. Empty means the empty clause, which ends the proof.
    Add {
        /// The lemma's literals, in file order. The first is the pivot.
        lits: Vec<Lit>,
        /// One-based line number.
        line: u64,
    },
    /// A deletion, naming a clause by its literals rather than by an
    /// identifier. The set is matched, not the order.
    Delete {
        /// The literals of the clause to remove.
        lits: Vec<Lit>,
        /// One-based line number.
        line: u64,
    },
}

/// A streaming reader over a text DRAT proof.
pub struct DratReader<R: BufRead> {
    reader: R,
    limits: Limits,
    line: u64,
    finished: bool,
    sniffed: bool,
    buffer: String,
}

impl<R: BufRead> DratReader<R> {
    /// Wraps a reader.
    pub fn new(reader: R, limits: &Limits) -> Self {
        Self {
            reader,
            limits: *limits,
            line: 0,
            finished: false,
            sniffed: false,
            buffer: String::new(),
        }
    }

    /// The same widened sniff format detection uses.
    ///
    /// Detection has normally run already and would have stopped a binary
    /// proof before this reader existed. It is repeated here so that
    /// `--drat`, which skips detection by design, still answers a binary file
    /// with `s UNSUPPORTED` rather than with a parse error about a byte.
    fn is_binary(&mut self) -> bool {
        self.sniffed = true;
        match self.reader.fill_buf() {
            // A read error here is left to the read below, which reports it
            // with a line number.
            Ok(buffered) => crate::format::looks_binary(buffered),
            Err(_) => false,
        }
    }

    fn fail(&mut self, kind: ParseErrorKind) -> Option<Result<DratStep, ParseError>> {
        self.finished = true;
        Some(Err(ParseError::new(Source::Proof, self.line, kind)))
    }
}

impl<R: BufRead> Iterator for DratReader<R> {
    type Item = Result<DratStep, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.finished {
                return None;
            }
            if !self.sniffed && self.is_binary() {
                self.line = 1;
                return self.fail(ParseErrorKind::BinaryProof);
            }
            self.buffer.clear();
            // One byte past the ceiling, so that a line of exactly the ceiling
            // and a line one byte longer are told apart by what was read
            // rather than by a second look at the reader.
            let ceiling = self.limits.max_line_bytes;
            let take = u64::try_from(ceiling).unwrap_or(u64::MAX).saturating_add(1);
            match (&mut self.reader).take(take).read_line(&mut self.buffer) {
                Ok(0) => {
                    self.finished = true;
                    return None;
                }
                Ok(_) if self.buffer.len() > ceiling && !self.buffer.ends_with('\n') => {
                    self.line = self.line.saturating_add(1);
                    return self.fail(ParseErrorKind::LineTooLong { limit: ceiling });
                }
                Ok(_) => {}
                Err(err) => {
                    // `self.line` counts the lines already yielded, so the read
                    // that just failed was of the next one.
                    self.line = self.line.saturating_add(1);
                    let kind = io_kind(&err);
                    return self.fail(kind);
                }
            }
            self.line = self.line.saturating_add(1);
            let line = core::mem::take(&mut self.buffer);
            let parsed = {
                let content = strip_byte_order_mark(line.trim(), self.line);
                if content.is_empty() {
                    None
                } else {
                    Some(parse_step(content, self.line, &self.limits))
                }
            };
            self.buffer = line;
            return match parsed {
                None => continue,
                Some(Ok(step)) => Some(Ok(step)),
                Some(Err(kind)) => self.fail(kind),
            };
        }
    }
}

/// True when this line is a well-formed DRAT step. The acceptance half of
/// [`crate::format::detect`], and the parser itself rather than a description
/// of it.
pub(crate) fn accepts(line: &str, limits: &Limits) -> bool {
    parse_step(line, 1, limits).is_ok()
}

fn parse_step(line: &str, line_no: u64, limits: &Limits) -> Result<DratStep, ParseErrorKind> {
    let mut tokens = line.split_ascii_whitespace();
    let mut deletion = false;
    let mut lookahead = tokens.clone();
    if lookahead.next() == Some("d") {
        deletion = true;
        tokens = lookahead;
    }

    let mut lits: Vec<Lit> = Vec::new();
    let mut terminated = false;
    for token in tokens.by_ref() {
        if token == "0" {
            terminated = true;
            break;
        }
        // A comment line, or anything else that is not an integer, dies here.
        // DRAT has no comments; `kissat` writes none, measured at zero
        // occurrences, so this fails closed on a file nobody has been observed
        // to write rather than guessing what a leading `c` was meant to mean.
        push_bounded(&mut lits, scan_lit(token, limits)?, limits)?;
    }
    if !terminated {
        return Err(ParseErrorKind::MissingTerminator);
    }
    // Milestone 1's rule, and the reason an LRAT addition cannot be mistaken
    // for a DRAT one: exactly one terminator, and it ends the line.
    if let Some(extra) = tokens.next() {
        return Err(ParseErrorKind::TrailingTokens(extra.to_owned()));
    }

    Ok(if deletion {
        DratStep::Delete {
            lits,
            line: line_no,
        }
    } else {
        DratStep::Add {
            lits,
            line: line_no,
        }
    })
}
