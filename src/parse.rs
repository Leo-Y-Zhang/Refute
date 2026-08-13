//! Parse errors and the shared integer scanner.
//!
//! Both parsers read the same untrusted token shape, so the scanning of a
//! signed integer — with checked arithmetic, never an `as` cast — lives here
//! once rather than twice.

use core::fmt;

use crate::limits::Limits;

/// Which file a parse error came from. The CLI prints this word, so a user
/// with two paths on the command line knows which one to look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The DIMACS formula.
    Formula,
    /// The LRAT proof.
    Proof,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Formula => write!(f, "formula"),
            Self::Proof => write!(f, "proof"),
        }
    }
}

/// What went wrong, in `rustc`'s "expected X, found Y" register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The underlying reader failed. The string is the OS message.
    Io(String),
    /// The file ended in the middle of a clause, step or list.
    UnexpectedEof,
    /// A line ended before its `0` terminator.
    MissingTerminator,
    /// A token was not an integer.
    NotAnInteger(String),
    /// A token was an integer too large for the checker's own arithmetic.
    IntegerOverflow(String),
    /// A variable index beyond [`Limits::max_var`].
    VarExceedsLimit {
        /// The variable that was asked for.
        var: u64,
        /// The ceiling in force.
        limit: u32,
    },
    /// A clause, hint list or deletion list beyond [`Limits::max_clause_len`].
    ListTooLong {
        /// The ceiling in force.
        limit: usize,
    },
    /// A `p` line that was not `p cnf <vars> <clauses>`.
    BadHeader(String),
    /// A second `p` line.
    DuplicateHeader,
    /// A clause identifier that was not strictly positive.
    NonPositiveClauseId(String),
    /// Tokens after the terminator that ends a step.
    TrailingTokens(String),
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "read failed: {msg}"),
            Self::UnexpectedEof => write!(f, "expected 0 terminator, found end of file"),
            Self::MissingTerminator => write!(f, "expected 0 terminator, found end of line"),
            Self::NotAnInteger(tok) => write!(f, "expected an integer, found '{tok}'"),
            Self::IntegerOverflow(tok) => write!(f, "integer '{tok}' is out of range"),
            Self::VarExceedsLimit { var, limit } => {
                write!(f, "variable {var} exceeds limit {limit}")
            }
            Self::ListTooLong { limit } => write!(f, "list longer than limit {limit}"),
            Self::BadHeader(line) => {
                write!(f, "expected 'p cnf <vars> <clauses>', found '{line}'")
            }
            Self::DuplicateHeader => write!(f, "a second 'p' header line"),
            Self::NonPositiveClauseId(tok) => {
                write!(f, "expected a positive clause id, found '{tok}'")
            }
            Self::TrailingTokens(tok) => write!(f, "unexpected token '{tok}' after 0 terminator"),
        }
    }
}

/// A parse failure, located.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Which of the two files.
    pub source: Source,
    /// One-based line number.
    pub line: u64,
    /// What went wrong.
    pub kind: ParseErrorKind,
}

impl ParseError {
    /// Builds a located parse error.
    #[must_use]
    pub fn new(source: Source, line: u64, kind: ParseErrorKind) -> Self {
        Self { source, line, kind }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} line {}: {}", self.source, self.line, self.kind)
    }
}

/// Scans a signed decimal integer with checked arithmetic.
///
/// Returns the value as `i64` so that the caller can range-check against its
/// own ceiling and report the offending number, rather than silently wrapping.
/// `99999999999999999999` fails here, before anything is allocated.
pub(crate) fn scan_i64(tok: &str) -> Result<i64, ParseErrorKind> {
    let (negative, digits) = match tok.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, tok.strip_prefix('+').unwrap_or(tok)),
    };
    if digits.is_empty() {
        return Err(ParseErrorKind::NotAnInteger(tok.to_owned()));
    }
    let mut acc: i64 = 0;
    for byte in digits.bytes() {
        let digit = match byte {
            b'0'..=b'9' => i64::from(byte.wrapping_sub(b'0')),
            _ => return Err(ParseErrorKind::NotAnInteger(tok.to_owned())),
        };
        acc = match acc
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(digit))
        {
            Some(next) => next,
            None => return Err(ParseErrorKind::IntegerOverflow(tok.to_owned())),
        };
    }
    if negative {
        acc = match acc.checked_neg() {
            Some(negated) => negated,
            None => return Err(ParseErrorKind::IntegerOverflow(tok.to_owned())),
        };
    }
    Ok(acc)
}

/// Scans a literal and range-checks its variable against `limits`.
pub(crate) fn scan_lit(tok: &str, limits: &Limits) -> Result<crate::lit::Lit, ParseErrorKind> {
    let raw = scan_i64(tok)?;
    let magnitude = raw.unsigned_abs();
    if magnitude > u64::from(limits.max_var) {
        return Err(ParseErrorKind::VarExceedsLimit {
            var: magnitude,
            limit: limits.max_var,
        });
    }
    // In range for `i32` because `max_var` cannot exceed `u32::MAX` and the
    // default is 2^26; the conversion is checked rather than cast regardless.
    let narrowed =
        i32::try_from(raw).map_err(|_| ParseErrorKind::IntegerOverflow(tok.to_owned()))?;
    crate::lit::Lit::new(narrowed).ok_or(ParseErrorKind::NotAnInteger(tok.to_owned()))
}

/// Scans a clause identifier: strictly positive, within `u64`.
pub(crate) fn scan_id(tok: &str) -> Result<crate::lit::ClauseId, ParseErrorKind> {
    let raw = scan_i64(tok)?;
    if raw <= 0 {
        return Err(ParseErrorKind::NonPositiveClauseId(tok.to_owned()));
    }
    u64::try_from(raw).map_err(|_| ParseErrorKind::IntegerOverflow(tok.to_owned()))
}
