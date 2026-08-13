//! Refute — an independent forward checker for LRAT unsatisfiability proofs.
//!
//! Milestone 1 checks RUP steps with hints. RAT hint blocks and empty hint
//! lists are reported as unsupported, never as verified. See `docs/PRD.md`.
//!
//! The library never exits the process, never prints, and never panics on
//! input-derived data; the `deny` list below is how that is enforced rather
//! than merely intended. A panic is a denial of service in the milestone-4
//! WASM target, so it is a build failure here.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#![warn(missing_docs)]
