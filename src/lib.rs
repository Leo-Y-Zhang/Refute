//! Refute — an independent forward checker for LRAT unsatisfiability proofs.
//!
//! A SAT solver answering UNSAT is an assertion, not a proof. Refute exists to
//! be a second, independently written opinion on the certificate.
//!
//! Milestone 1b checks every addition line a text `drat-trim -L` file
//! contains: RUP steps with hints, RAT steps with resolvent blocks, and the
//! empty hint list -- which is a claim that the lemma's pivot has no
//! resolution candidate, and is accepted only after this checker has
//! established that for itself. A binary proof is reported as
//! [`verdict::Unsupported`], never as verified. See `docs/PRD.md` for the
//! measurement and what it constrains.
//!
//! The library never exits the process, never prints, and never panics on
//! input-derived data. That is enforced rather than merely intended: the
//! `[lints]` tables in `Cargo.toml` deny `unwrap`, `expect`, indexing, `panic`
//! and unchecked arithmetic, and forbid `unsafe`. They live in the manifest so
//! that the binary and the test targets inherit the same floor — as a `deny`
//! block here they covered this crate alone, and an `unwrap` in the CLI passed
//! the gate. A panic is a denial of service in the milestone-4 WASM target, so
//! it is a build failure everywhere.

pub mod checker;
pub mod cnf;
pub mod limits;
pub mod lit;
pub mod lrat;
pub mod parse;
pub mod verdict;

pub use checker::{check, check_readers, check_with_stats, Outcome, Stats};
pub use cnf::{parse_dimacs, Cnf};
pub use limits::Limits;
pub use lit::{Clause, ClauseId, Lit};
pub use lrat::{Hints, LratReader, ResolventBlock, Step};
pub use parse::{ParseError, ParseErrorKind, Source};
pub use verdict::{Reason, Rejection, Unsupported, Verdict};
