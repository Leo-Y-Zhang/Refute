//! Which of the two proof formats a file is, decided by reading it.
//!
//! The CLI takes a proof path and no format. A user with both files in one
//! directory will hand over the wrong one, and a checker that reads an LRAT
//! file as DRAT — or the reverse — must not do so silently.
//!
//! **Never the file extension.** An extension is a claim by whoever named the
//! file, and this project's whole posture is that a claim is not evidence.
//!
//! Detection is exact rather than heuristic, because the two grammars are
//! disjoint on a per-line basis:
//!
//! ```text
//! DRAT step  := "d" lit* 0  |  lit* 0            ; one 0, and it ends the line
//! LRAT step  := id "d" id* 0  |  id lit* 0 id* 0 ; an addition has two groups
//! ```
//!
//! An LRAT addition needs two terminators and a DRAT addition rejects anything
//! after its one. An LRAT deletion's *second* token is `d`; a DRAT deletion's
//! first token is. An LRAT identifier is strictly positive, while a DRAT line
//! commonly opens with a negative literal. Measured over the 49 committed
//! `.lrat` proofs and the nine `.drat` proofs behind `docs/TDD.md` part 3, no
//! proof's first step is accepted by the other grammar and none by both.
//!
//! The acceptance test is not a third parser. It is the two readers' own
//! parsers, called on the line, so "the grammars are disjoint" is a statement
//! about the code that will actually read the file rather than about a
//! description of it that could drift.

use crate::limits::Limits;
use crate::parse::strip_byte_order_mark;

/// How many bytes of the proof detection is allowed to look at.
///
/// Enough for any first line any producer writes — the longest first line in
/// the corpus is 27 bytes — and small enough that peeking is free.
pub(crate) const PEEK_BYTES: usize = 1024;

/// The proof format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Text LRAT, as `drat-trim -L` writes it. Milestones 1 and 1b.
    Lrat,
    /// Text DRAT, as the solver writes it. Milestone 2.
    Drat,
}

/// Recognises a binary proof from the head of the file.
///
/// Milestone 1b's rule was "the first byte is `a` (0x61) or `d` (0x64)", which
/// was exactly right while LRAT was the only text format: a text LRAT line
/// always opens with a decimal identifier. **A text DRAT deletion line opens
/// `d `.** Left alone, that rule reports a perfectly good text proof as binary
/// whenever its first line is a deletion, and never reads it at all.
///
/// So the rule is widened. Binary DRAT terminates every record with a 0x00
/// byte and no text proof contains one, which makes the first clause decisive;
/// the second is a cheap belt for a file too short to have reached a
/// terminator yet.
///
/// Both clauses can only produce an unsupported verdict, which has no route to
/// `Verified`. The residual failure — a binary proof whose first record runs
/// past the peek and whose second byte happens to be a space — is a worse
/// message, not a worse verdict.
#[must_use]
pub(crate) fn looks_binary(head: &[u8]) -> bool {
    let peeked = head.get(..PEEK_BYTES).unwrap_or(head);
    if peeked.contains(&0x00) {
        return true;
    }
    match (peeked.first(), peeked.get(1)) {
        (Some(b'a' | b'd'), Some(b' ' | b'\t')) => false,
        (Some(b'a' | b'd'), _) => true,
        _ => false,
    }
}

/// Classifies a proof from the head of the file.
///
/// Total. Every input gets a format, and the one it gets when nothing is
/// certain is [`Format::Lrat`] — the incumbent — so that **nothing about
/// milestone 1's behaviour changes**. A file neither grammar accepts gets
/// exactly the parse error it got before, `hostile_escape_proof` keeps its
/// escaped message, and an empty file keeps `NoEmptyClause`.
///
/// A binary proof is one of those cases, deliberately. Both readers carry the
/// sniff themselves and answer [`Unsupported::BinaryProof`] with a line
/// number, after the formula has been parsed; declining to classify here is
/// what keeps a binary file out of the DRAT reader, where it would have earned
/// a parse error about a byte instead.
///
/// **Mis-routing cannot produce a false `VERIFIED`.** Each checker is sound for
/// its own grammar: if the DRAT checker verifies a file, that file *read as
/// DRAT* refutes the formula, whatever its author meant it to be. The cost of
/// a wrong guess is a confusing rejection. That is the only reason this
/// function is allowed a default arm at all.
#[must_use]
pub(crate) fn detect(head: &[u8], limits: &Limits) -> Format {
    if looks_binary(head) {
        return Format::Lrat;
    }
    match first_step(head) {
        Some(line) => match (
            crate::lrat::accepts(line, limits),
            crate::drat::accepts(line, limits),
        ) {
            (false, true) => Format::Drat,
            // True/false, neither, or — unobserved on the whole corpus — both.
            // No reason code is invented for a case nothing can produce.
            _ => Format::Lrat,
        },
        None => Format::Lrat,
    }
}

/// The first non-empty line of the head, if the head certainly contains a
/// whole one.
///
/// "Certainly" is the load-bearing word. A line cut in half by the peek can
/// parse as the other grammar — an LRAT addition truncated after its first `0`
/// is a well-formed DRAT addition — so a line that reaches the end of the head
/// without a newline is not classified at all unless the reader gave back less
/// than was asked for, which means it had no more to give. The cost of
/// declining is the default arm, which is milestone 1's behaviour.
fn first_step(head: &[u8]) -> Option<&str> {
    let text = match core::str::from_utf8(head) {
        Ok(text) => text,
        // A decoding failure past the first line is not this function's
        // business; the reader will report it with a line number. Everything
        // before the bad byte is still a valid prefix.
        Err(err) => core::str::from_utf8(head.get(..err.valid_up_to()).unwrap_or(&[])).ok()?,
    };
    let at_eof = head.len() < PEEK_BYTES;
    let mut line_no: u64 = 0;
    for line in text.split_inclusive('\n') {
        let complete = line.ends_with('\n');
        line_no = line_no.saturating_add(1);
        let content = strip_byte_order_mark(line.trim(), line_no);
        if content.is_empty() {
            if complete {
                continue;
            }
            return None;
        }
        return if complete || at_eof {
            Some(content)
        } else {
            None
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{detect, looks_binary, Format};
    use crate::limits::Limits;

    fn format_of(head: &str) -> Format {
        detect(head.as_bytes(), &Limits::default())
    }

    #[test]
    fn an_lrat_addition_needs_two_terminators() {
        assert_eq!(format_of("1 2 3 0 4 5 0\n"), Format::Lrat);
        assert_eq!(format_of("1 2 3 0\n"), Format::Drat);
    }

    #[test]
    fn a_deletion_is_told_apart_by_where_the_d_is() {
        assert_eq!(format_of("9 d 1 2 0\n"), Format::Lrat);
        assert_eq!(format_of("d 1 2 0\n"), Format::Drat);
    }

    #[test]
    fn a_negative_first_token_can_only_be_drat() {
        assert_eq!(format_of("-154 0\n"), Format::Drat);
    }

    #[test]
    fn neither_grammar_means_the_incumbent_reader() {
        assert_eq!(format_of("\u{1b}[1A 0 0\n"), Format::Lrat);
        assert_eq!(format_of(""), Format::Lrat);
        assert_eq!(format_of("\n\n"), Format::Lrat);
    }

    /// A fragment is not a line, and must not be classified as one.
    ///
    /// The guard is `complete || at_eof`, so exercising it needs a head with
    /// no newline anywhere in the peeked window AND at least `PEEK_BYTES` of
    /// it -- otherwise `at_eof` carries the test on its own and the mutant
    /// lives. The version this replaces ended its input in a newline, which
    /// made `complete` true: it passed with the guard and passed with the
    /// guard removed, which is to say it tested nothing.
    ///
    /// `-1` cannot open an LRAT step, so a classifier that read this
    /// fragment would call the file DRAT on the strength of half a line.
    #[test]
    fn a_half_line_is_not_classified() {
        // The peeked window has to be a clause the DRAT grammar ACCEPTS,
        // or the fragment is unclassifiable for a second reason and the
        // guard is still not what decided it.
        // Exactly a peek-full, with no newline: what the reader hands over
        // when the first line runs past the window. The window has to hold
        // a clause the DRAT grammar ACCEPTS, or the fragment is
        // unclassifiable for a second reason and the guard is still not
        // what decided it.
        let fragment = format!("{}0", "-1 ".repeat((super::PEEK_BYTES - 1) / 3));
        assert_eq!(fragment.len(), super::PEEK_BYTES);
        assert!(!fragment.contains('\n'));
        assert_eq!(format_of(&fragment), Format::Lrat);
    }

    #[test]
    fn a_binary_proof_is_never_classified_as_drat() {
        assert_eq!(format_of("a*\x13\x00"), Format::Lrat);
    }

    #[test]
    fn a_leading_deletion_line_is_not_binary() {
        assert!(!looks_binary(b"d 1 2 0\n"));
        assert!(!looks_binary(b"a 1 2 0\n"));
        assert!(looks_binary(b"a\x2a\x13\x00"));
        assert!(looks_binary(b"d\xff\xff"));
        assert!(!looks_binary(b"1 2 0\n"));
    }
}
