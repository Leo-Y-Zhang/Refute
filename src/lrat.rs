//! Streaming LRAT proof parsing.
//!
//! The reader yields one step at a time and never holds the file. A 200 MB
//! proof is read in memory bounded by [`Limits::max_line_bytes`], one line at
//! a time; only the clause database grows.
//!
//! That bound is enforced rather than assumed. Until it existed the sentence
//! above said "in constant memory" and was false: `read_line` buffers a whole
//! line before any ceiling can apply to what is in it, so a 200 MB proof
//! written on one line peaked at 268.6 MB of working set — measured on the
//! release binary — and only then failed on `max_clause_len`.
//!
//! Parsing is line-oriented, unlike the formula parser. LRAT is
//! whitespace-delimited on paper, but a step that runs off the end of its line
//! would then quietly absorb the next step's identifier as a literal, and a
//! truncated proof would mis-parse rather than fail. One step per line, and a
//! line that does not terminate is an error.

use std::io::{BufRead, Read};

use crate::cnf::io_kind;
use crate::limits::Limits;
use crate::lit::{ClauseId, Lit};
use crate::parse::{
    scan_i64, scan_id, scan_lit, strip_byte_order_mark, ParseError, ParseErrorKind, Source,
};

/// One resolvent block: the clause resolved against, and its own hints.
///
/// Opened by a negative identifier. Every positive identifier after it, up to
/// the next negative one or the end of the list, belongs to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolventBlock {
    /// The clause the lemma is resolved against. Always positive; the sign in
    /// the file is the marker, not part of the identifier.
    pub clause: ClauseId,
    /// The hints for this block's resolvent. Empty in all 703 blocks measured
    /// for `docs/TDD.md` part 2, because the resolvent's own negation refutes
    /// it — which is exactly why the walk over them needs a built fixture.
    pub hints: Vec<ClauseId>,
}

/// The hint list of an addition step, classified before anything is checked.
///
/// Milestone 1 kept only the positive identifiers and threw the rest away, on
/// the ground that a half-understood hint list is worse than none. They are
/// fully understood now, so they are all kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hints {
    /// Every hint is a positive identifier: a RUP derivation. 96.0 % of
    /// addition lines in the measured corpus.
    Rup(Vec<ClauseId>),
    /// At least one negative identifier: a RAT step. 2.4 %.
    Rat {
        /// The positive identifiers before the first block, propagated before
        /// any resolvent is checked. Possibly empty.
        prefix: Vec<ClauseId>,
        /// The resolvent blocks, in file order. Never empty in this variant.
        blocks: Vec<ResolventBlock>,
    },
    /// No hints at all, as in `205 57 -29 0 0`. 2.0 %, and a claim rather than
    /// an absence: it says the lemma's pivot has no resolution candidate. The
    /// checker establishes that for itself before accepting it.
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
    sniffed: bool,
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
            sniffed: false,
            buffer: String::new(),
        }
    }

    /// Recognises a binary proof from its first byte, before any decoding.
    ///
    /// Binary DRAT and binary LRAT begin every record with `a` (0x61) or `d`
    /// (0x64); a text LRAT line always begins with a decimal step identifier,
    /// and a deletion is `<id> d ...`, so the first byte of a text proof is
    /// never either of these. `kissat` writes binary unless it is told
    /// `--no-binary`, which makes this the commonest way to hand a checker
    /// something it cannot read.
    ///
    /// Done on the raw bytes rather than on a decoded line because a binary
    /// proof need not be valid UTF-8, and a read that fails to decode would
    /// report an I/O error instead. It is the first byte of the *file*, which
    /// is narrower than `docs/TDD.md` part 2's "first non-empty line": the
    /// narrowing can only fail to recognise a binary proof, leaving milestone
    /// 1's parse error in place, and has no route to a false `VERIFIED`.
    fn is_binary(&mut self) -> bool {
        self.sniffed = true;
        match self.reader.fill_buf() {
            // A read error here is left to the read below, which reports it
            // with a line number.
            Ok(buffered) => matches!(buffered.first(), Some(b'a') | Some(b'd')),
            Err(_) => false,
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
            if !self.sniffed && self.is_binary() {
                self.line = 1;
                return self.fail(ParseErrorKind::BinaryProof);
            }
            self.buffer.clear();
            // One byte past the ceiling, so that a line of exactly the ceiling
            // and a line one byte longer are told apart by what was read
            // rather than by a second look at the reader. Everything the
            // parser does below happens to a line this call has already
            // bounded.
            let ceiling = self.limits.max_line_bytes;
            let take = u64::try_from(ceiling).unwrap_or(u64::MAX).saturating_add(1);
            match (&mut self.reader).take(take).read_line(&mut self.buffer) {
                Ok(0) => {
                    self.finished = true;
                    return None;
                }
                // Read the whole allowance and no newline in it: the line runs
                // past the ceiling. A line of exactly `ceiling` bytes that ends
                // the file reads `ceiling` bytes, not `ceiling + 1`, so it is
                // not this case and is accepted.
                //
                // A cut through a multi-byte character is reported by
                // `read_line` as an I/O error instead, which is what any
                // undecodable byte in the proof already gets. LRAT is ASCII.
                Ok(_) if self.buffer.len() > ceiling && !self.buffer.ends_with('\n') => {
                    self.line = self.line.saturating_add(1);
                    return self.fail(ParseErrorKind::LineTooLong { limit: ceiling });
                }
                Ok(_) => {}
                Err(err) => {
                    // `self.line` counts the lines already yielded, so the
                    // read that just failed was of the next one. The counter
                    // moves first, and the error is located on that line.
                    self.line = self.line.saturating_add(1);
                    let kind = io_kind(&err);
                    return self.fail(kind);
                }
            }
            self.line = self.line.saturating_add(1);
            let line = core::mem::take(&mut self.buffer);
            // A line holding nothing but a byte order mark is an empty line,
            // so the mark is stripped before the emptiness test rather than
            // after it.
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

    let mut prefix: Vec<ClauseId> = Vec::new();
    let mut blocks: Vec<ResolventBlock> = Vec::new();
    let mut hint_tokens: usize = 0;
    terminated = false;
    for token in tokens.by_ref() {
        if token == "0" {
            terminated = true;
            break;
        }
        // The ceiling bounds the hint list as a whole — prefix, block markers
        // and block hints together — rather than each list inside it. A line
        // of ten million one-hint blocks allocates what a ten-million-hint
        // list allocates, and milestone 1 counted only the positives, so that
        // line was unbounded.
        if hint_tokens >= limits.max_clause_len {
            return Err(ParseErrorKind::ListTooLong {
                limit: limits.max_clause_len,
            });
        }
        hint_tokens = hint_tokens.saturating_add(1);
        let value = scan_i64(token)?;
        if value < 0 {
            // A negative identifier opens a resolvent block. `unsigned_abs`
            // cannot overflow here: `scan_i64` rejects `i64::MIN` as out of
            // range, so no new arithmetic reaches the untrusted path. `-0`
            // scans as zero rather than as a negative, so it is read below as
            // a hint identifier and rejected there.
            blocks.push(ResolventBlock {
                clause: value.unsigned_abs(),
                hints: Vec::new(),
            });
        } else {
            let id = scan_id(token)?;
            match blocks.last_mut() {
                Some(block) => block.hints.push(id),
                None => prefix.push(id),
            }
        }
    }
    if !terminated {
        return Err(ParseErrorKind::MissingTerminator);
    }
    if let Some(extra) = tokens.next() {
        return Err(ParseErrorKind::TrailingTokens(extra.to_owned()));
    }

    let hints = if blocks.is_empty() {
        if prefix.is_empty() {
            Hints::Empty
        } else {
            Hints::Rup(prefix)
        }
    } else {
        Hints::Rat { prefix, blocks }
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
