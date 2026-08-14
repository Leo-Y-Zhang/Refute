//! The WebAssembly export boundary for [`refute`].
//!
//! This crate exists because of one measurement. `unsafe_code = "forbid"` in
//! the checker's manifest blocks every form a WebAssembly export can take —
//! `#[no_mangle]`, `#[export_name]` and `unsafe extern` alike, all three tried
//! and all three refused. The two obvious ways out are both bad: relaxing the
//! lint weakens a property the README states about the checker, and taking
//! `wasm-bindgen` breaks the no-dependency rule and ships its own `unsafe`
//! besides. Neither is necessary. The exports go in a second crate, and the
//! checker is not edited at all.
//!
//! The wrapper needs no `unsafe` of its own either. It hands JavaScript an
//! offset into linear memory and JavaScript dereferences it, which is
//! JavaScript's business; nothing here takes a pointer *in*, so there is no
//! pointer to trust. `tests/trust_boundary.rs` in the checker crate asserts
//! both halves of that: no `unsafe` block here, and `forbid` still there.
//!
//! # The glue contract
//!
//! Three rules, and the probe that led to this design broke the first one:
//!
//! 1. Call the reserve export, **then** read `instance.exports.memory.buffer`.
//!    Growing linear memory detaches every `ArrayBuffer` view of it, and
//!    JavaScript evaluates arguments left to right, so
//!    `new Uint8Array(ex.memory.buffer, ex.proof_reserve(n), n)` hands
//!    `Uint8Array` a detached buffer and throws on a line that looks correct.
//! 2. Never cache a `Uint8Array` across an export call, for the same reason.
//! 3. One instance per check. `memory.grow` has no inverse, so linear memory is
//!    a high-water mark and only a dropped instance gives it back.
//!
//! See `docs/TDD.md` part 5 for the measurements all three come from.

use std::cell::Cell;
use std::thread::LocalKey;

use refute::{Limits, Verdict};

/// A checked sequence of steps derived the empty clause.
pub const VERIFIED: u32 = 0;
/// The proof was read and found wanting.
pub const NOT_VERIFIED: u32 = 1;
/// The proof uses a construct this checker does not check. Not a pass.
pub const UNSUPPORTED: u32 = 2;

thread_local! {
    /// The formula, written by JavaScript at the offset [`cnf_reserve`] returns.
    static FORMULA: Cell<Vec<u8>> = const { Cell::new(Vec::new()) };
    /// The proof, written by JavaScript at the offset [`proof_reserve`] returns.
    static PROOF: Cell<Vec<u8>> = const { Cell::new(Vec::new()) };
}

/// Sizes one buffer and returns the offset JavaScript should write at.
///
/// [`Cell::take`] and [`Cell::set`] rather than a `RefCell`: a `RefCell` borrow
/// panics if it overlaps another, and a panic here is a trap, and a trap is a
/// blank page. Moving the `Vec` out and back cannot fail and does not move the
/// heap allocation the returned offset points at.
fn reserve(slot: &'static LocalKey<Cell<Vec<u8>>>, len: usize) -> usize {
    slot.with(|cell| {
        let mut buffer = cell.take();
        buffer.clear();
        buffer.resize(len, 0);
        // Safe: casting a pointer to an integer only reads its address. The
        // dereference happens in JavaScript, against the same linear memory.
        let offset = buffer.as_ptr() as usize;
        cell.set(buffer);
        offset
    })
}

/// Reserves `len` bytes for the formula and returns the offset to write at.
///
/// Read `instance.exports.memory.buffer` **after** this call, never before:
/// see the glue contract in the crate documentation.
// `#[no_mangle]` is `unsafe_code` to the lint, and without it this function has
// no exported symbol. Allowed here rather than in the manifest so that the
// exception is one item wide and visible in a diff.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn cnf_reserve(len: usize) -> usize {
    reserve(&FORMULA, len)
}

/// Reserves `len` bytes for the proof and returns the offset to write at.
///
/// Read `instance.exports.memory.buffer` **after** this call, never before:
/// see the glue contract in the crate documentation.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn proof_reserve(len: usize) -> usize {
    reserve(&PROOF, len)
}

/// Checks the reserved proof against the reserved formula.
///
/// Returns [`VERIFIED`], [`NOT_VERIFIED`] or [`UNSUPPORTED`]. Nothing richer:
/// the failing step, the line and the reason arrive in a later commit, by the
/// same route the CLI's do. A page that reports a verdict correctly and says
/// nothing else is useful; a page that reports one *incorrectly* is worse than
/// no page at all, so the verdict ships first and alone.
///
/// The buffers are put back, so calling this twice on one instance checks the
/// same input twice and gives the same answer. That is a deliberate choice
/// against the alternative — freeing them — which would have made a second call
/// silently check nothing at all and report `NOT VERIFIED` for the wrong
/// reason. It is not licence to reuse an instance: rule 3 of the glue contract
/// is about memory, and it still holds.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn check() -> u32 {
    FORMULA.with(|formula_cell| {
        PROOF.with(|proof_cell| {
            let formula = formula_cell.take();
            let proof = proof_cell.take();

            // The same entry point the CLI uses, under the same default limits.
            // "A formula we cannot read is a proof we cannot accept" lives
            // inside it rather than here, which is what makes the module's
            // verdict the checker's verdict rather than a second opinion about
            // it.
            let outcome =
                refute::check_readers(formula.as_slice(), proof.as_slice(), &Limits::default());

            // Exhaustive, and asserted so from the checker crate's test suite.
            // A wildcard arm here is the one defect that would let this module
            // report a verdict the checker never gave.
            let code = match outcome.verdict {
                Verdict::Verified => VERIFIED,
                Verdict::NotVerified(_) => NOT_VERIFIED,
                Verdict::Unsupported(_) => UNSUPPORTED,
            };

            formula_cell.set(formula);
            proof_cell.set(proof);
            code
        })
    })
}

#[cfg(test)]
mod tests {
    // A test asserts by panicking. The package's panic floor is there for the
    // module, where a panic is a trap; here it would only make the failure
    // report worse.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{check, cnf_reserve, proof_reserve, NOT_VERIFIED, UNSUPPORTED, VERIFIED};

    /// Writes `bytes` into the buffer at `offset`, the way the glue does.
    ///
    /// The host build has no linear memory to index, so this reaches the same
    /// `Vec` through the same `thread_local` instead. What it exercises is the
    /// contract — reserve, write, check — and not the JavaScript side of it,
    /// which is the Node harness's job.
    fn write(slot: &'static std::thread::LocalKey<std::cell::Cell<Vec<u8>>>, bytes: &[u8]) {
        slot.with(|cell| {
            let mut buffer = cell.take();
            buffer.clear();
            buffer.extend_from_slice(bytes);
            cell.set(buffer);
        });
    }

    fn load(formula: &[u8], proof: &[u8]) {
        let _ = cnf_reserve(formula.len());
        let _ = proof_reserve(proof.len());
        write(&super::FORMULA, formula);
        write(&super::PROOF, proof);
    }

    /// Clause 1 is `x`, clause 2 is `not x`. The smallest formula that has a
    /// refutation, which is what a boundary test wants.
    const TINY_CNF: &[u8] = b"p cnf 1 2\n1 0\n-1 0\n";
    /// Step 3 is the empty clause, resolving clauses 1 and 2.
    const TINY_PROOF: &[u8] = b"3 0 1 2 0\n";

    #[test]
    fn a_verified_proof_reports_zero() {
        load(TINY_CNF, TINY_PROOF);
        assert_eq!(check(), VERIFIED);
    }

    #[test]
    fn a_proof_that_never_derives_the_empty_clause_reports_one() {
        // A sound step — clause 1 conflicts with the negated lemma — and then
        // the proof stops. Sound and incomplete is still not a refutation.
        load(TINY_CNF, b"3 1 0 1 0\n");
        assert_eq!(check(), NOT_VERIFIED);
    }

    #[test]
    fn a_binary_proof_reports_two_and_not_one() {
        // The three verdicts must stay three. A module that collapsed
        // UNSUPPORTED into NOT VERIFIED would be a different tool wearing this
        // one's name, and every assertion above would still pass.
        load(TINY_CNF, b"a\x02\x04\x00\x00");
        assert_eq!(check(), UNSUPPORTED);
    }

    #[test]
    fn reserving_returns_an_offset_that_stays_put_across_the_check() {
        let offset = cnf_reserve(TINY_CNF.len());
        write(&super::FORMULA, TINY_CNF);
        let _ = proof_reserve(0);
        assert_eq!(
            super::FORMULA.with(|cell| {
                let buffer = cell.take();
                let seen = buffer.as_ptr() as usize;
                cell.set(buffer);
                seen
            }),
            offset,
            "reserving the proof moved the formula's buffer"
        );
    }

    #[test]
    fn checking_twice_on_one_instance_gives_the_same_answer() {
        // The buffers are put back deliberately. If they were dropped, this
        // would report NOT VERIFIED the second time — a wrong answer that no
        // caller could distinguish from a real one.
        load(TINY_CNF, TINY_PROOF);
        assert_eq!(check(), VERIFIED);
        assert_eq!(check(), VERIFIED);
    }
}
