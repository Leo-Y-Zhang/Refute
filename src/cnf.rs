//! DIMACS CNF parsing.

use std::io::BufRead;

use crate::limits::Limits;
use crate::lit::{Clause, Lit};
use crate::parse::{scan_i64, scan_lit, strip_byte_order_mark, ParseError, ParseErrorKind, Source};

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
/// DIMACS is whitespace-delimited, not line-oriented: a clause may span lines
/// and comments may sit between its literals, whatever a given generator
/// happens to emit. The header is treated as advisory, because a formula whose
/// header undercounts its variables is still a formula.
///
/// # Errors
///
/// Returns [`ParseError`] for malformed input, for a literal beyond
/// [`Limits::max_var`], and for a read failure on `reader`.
pub fn parse_dimacs<R: BufRead>(mut reader: R, limits: &Limits) -> Result<Cnf, ParseError> {
    let mut clauses: Vec<Clause> = Vec::new();
    let mut pending: Vec<Lit> = Vec::new();
    let mut warnings = Vec::new();
    let mut header: Option<(u32, usize)> = None;
    let mut max_var: u32 = 0;
    let mut line_no: u64 = 0;
    let mut buffer = String::new();
    let mut clause_started_at: u64 = 0;

    loop {
        buffer.clear();
        // `line_no` counts the lines already read, so the read that just failed
        // was of the next one. Reporting it a line early sends a reader to a
        // line that is fine.
        let read = reader.read_line(&mut buffer).map_err(|err| {
            ParseError::new(Source::Formula, line_no.saturating_add(1), io_kind(&err))
        })?;
        if read == 0 {
            break;
        }
        line_no = line_no.saturating_add(1);
        let line = strip_byte_order_mark(buffer.trim(), line_no);

        let mut tokens = line.split_ascii_whitespace();
        let first = match tokens.next() {
            Some(token) => token,
            None => continue,
        };
        match first {
            "c" => continue,
            // SATLIB files end with a '%' line followed by a bare '0'. Read as
            // clauses, that '0' is the empty clause, and a formula containing
            // the empty clause is refutable in one step by anyone who asks. It
            // is a terminator, not a clause.
            "%" => break,
            "p" => {
                if header.is_some() {
                    return Err(ParseError::new(
                        Source::Formula,
                        line_no,
                        ParseErrorKind::DuplicateHeader,
                    ));
                }
                header = Some(parse_header(line, line_no)?);
                continue;
            }
            _ => {}
        }

        for token in line.split_ascii_whitespace() {
            if token == "0" {
                clauses.push(pending.clone().into_boxed_slice());
                pending.clear();
                continue;
            }
            if pending.is_empty() {
                clause_started_at = line_no;
            }
            let lit = scan_lit(token, limits)
                .map_err(|kind| ParseError::new(Source::Formula, line_no, kind))?;
            if pending.len() >= limits.max_clause_len {
                return Err(ParseError::new(
                    Source::Formula,
                    line_no,
                    ParseErrorKind::ListTooLong {
                        limit: limits.max_clause_len,
                    },
                ));
            }
            max_var = max_var.max(lit.var());
            pending.push(lit);
        }
    }

    if !pending.is_empty() {
        return Err(ParseError::new(
            Source::Formula,
            clause_started_at,
            ParseErrorKind::UnexpectedEof,
        ));
    }

    match header {
        None => warnings.push(Warning::MissingHeader),
        Some((declared_vars, declared_clauses)) => {
            if declared_vars < max_var {
                warnings.push(Warning::HeaderVarUndercount {
                    declared: declared_vars,
                    found: max_var,
                });
            }
            if declared_clauses != clauses.len() {
                warnings.push(Warning::HeaderClauseMismatch {
                    declared: declared_clauses,
                    found: clauses.len(),
                });
            }
            max_var = max_var.max(declared_vars);
        }
    }

    Ok(Cnf {
        num_vars: max_var,
        clauses,
        warnings,
    })
}

fn parse_header(line: &str, line_no: u64) -> Result<(u32, usize), ParseError> {
    let bad = |line: &str| {
        ParseError::new(
            Source::Formula,
            line_no,
            ParseErrorKind::BadHeader(line.to_owned()),
        )
    };
    let mut tokens = line.split_ascii_whitespace();
    if tokens.next() != Some("p") || tokens.next() != Some("cnf") {
        return Err(bad(line));
    }
    let mut counts = [0i64; 2];
    for slot in &mut counts {
        let token = tokens.next().ok_or_else(|| bad(line))?;
        let value = scan_i64(token).map_err(|_| bad(line))?;
        if value < 0 {
            return Err(bad(line));
        }
        *slot = value;
    }
    if tokens.next().is_some() {
        return Err(bad(line));
    }
    let vars = u32::try_from(counts.first().copied().unwrap_or(0)).map_err(|_| bad(line))?;
    let clause_count =
        usize::try_from(counts.get(1).copied().unwrap_or(0)).map_err(|_| bad(line))?;
    Ok((vars, clause_count))
}

pub(crate) fn io_kind(err: &std::io::Error) -> ParseErrorKind {
    ParseErrorKind::Io(err.to_string())
}
