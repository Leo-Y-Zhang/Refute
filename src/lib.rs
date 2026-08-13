//! Refute — an independent forward checker for LRAT unsatisfiability proofs.
//!
//! A SAT solver answering UNSAT is an assertion, not a proof. Refute exists to
//! be a second, independently written opinion on the certificate.
//!
//! Milestone 1 checks RUP steps with hints. RAT hint blocks and empty hint
//! lists are reported as [`verdict::Unsupported`], never as verified; on a
//! measured pigeonhole proof that was 4.4 % of addition lines. See
//! `docs/PRD.md` for the measurement and what it constrains.
//!
//! The library never exits the process, never prints, and never panics on
//! input-derived data. The `deny` list below is how that is enforced rather
//! than merely intended: a panic is a denial of service in the milestone-4
//! WASM target, so it is a build failure here.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#![warn(missing_docs)]

pub mod checker;
pub mod cnf;
pub mod limits;
pub mod lit;
pub mod lrat;
pub mod parse;
pub mod verdict;

pub use checker::check;
pub use cnf::{parse_dimacs, Cnf};
pub use limits::Limits;
pub use lit::{Clause, ClauseId, Lit};
pub use lrat::{Hints, LratReader, Step};
pub use parse::{ParseError, ParseErrorKind, Source};
pub use verdict::{Reason, Rejection, Unsupported, Verdict};
