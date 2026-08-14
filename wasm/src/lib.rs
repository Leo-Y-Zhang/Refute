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
/// An input was larger than this module will hold. Not a verdict.
pub const REFUSED: u32 = 3;

/// The largest formula or proof this module will hold: 32 MiB.
///
/// Peak linear memory is roughly the proof plus the formula plus the store plus
/// a megabyte, and the proof is the larger term on every rung measured above
/// the smallest. 32 MiB of proof puts peak at about 48 MB, which is
/// comfortable in a desktop tab and honest on a phone.
///
/// It is not a claim about `WebAssembly.Memory` maxima, which differ per
/// browser, per platform and per amount of free RAM, and which this project has
/// not measured on a phone. It is set from this project's own ladder and
/// deliberately far below any published engine limit, so that the module is
/// wrong in the direction of refusing something it could have checked.
///
/// **It applies to the formula as well as to the proof**, where `docs/TDD.md`
/// part 5 states the refusal on proof size alone. The budget it states names
/// both terms, there are two files, and a user who drops a 500 MB formula with
/// a one-line proof would otherwise reach the ceiling by dying at it. Refusing
/// one more thing than the design asked for is the safe direction; accepting
/// one more is not.
pub const MAX_INPUT_BYTES: usize = 33_554_432;

/// The offset a refused reserve returns.
///
/// Zero is unambiguous: a live `Vec<u8>` never has address zero, and an empty
/// one is dangling-but-aligned at 1 rather than null.
/// `a_zero_length_reserve_still_returns_a_usable_offset` is the test that keeps
/// that true, because the whole refusal contract rests on it.
const REFUSED_OFFSET: usize = 0;

/// One of the two inputs, and whether it was refused.
///
/// The flag travels with the buffer rather than in a third `thread_local`, so
/// that a good formula cannot clear a refused proof's flag by being reserved
/// after it.
#[derive(Default)]
struct Input {
    /// What JavaScript writes into, sized by a reserve call.
    bytes: Vec<u8>,
    /// Set when the last reserve for this input was over the ceiling.
    refused: bool,
}

thread_local! {
    /// The formula, written by JavaScript at the offset [`cnf_reserve`] returns.
    static FORMULA: Cell<Input> = const { Cell::new(Input { bytes: Vec::new(), refused: false }) };
    /// The proof, written by JavaScript at the offset [`proof_reserve`] returns.
    static PROOF: Cell<Input> = const { Cell::new(Input { bytes: Vec::new(), refused: false }) };
}

/// Sizes one buffer and returns the offset JavaScript should write at, or
/// [`REFUSED_OFFSET`] if `len` is over [`MAX_INPUT_BYTES`].
///
/// [`Cell::take`] and [`Cell::set`] rather than a `RefCell`: a `RefCell` borrow
/// panics if it overlaps another, and a panic here is a trap, and a trap is a
/// blank page. Moving the `Vec` out and back cannot fail and does not move the
/// heap allocation the returned offset points at.
///
/// A refusal frees the buffer rather than keeping one nobody may write to, and
/// it clears the previous contents either way. The alternative — leaving the
/// last input in place — would let a refused reserve be followed by a `check()`
/// that quietly re-checked something the user had replaced.
fn reserve(slot: &'static LocalKey<Cell<Input>>, len: usize) -> usize {
    slot.with(|cell| {
        let mut input = cell.take();
        input.bytes.clear();
        if len > MAX_INPUT_BYTES {
            input.refused = true;
            input.bytes.shrink_to_fit();
            cell.set(input);
            return REFUSED_OFFSET;
        }
        input.refused = false;
        input.bytes.resize(len, 0);
        // Safe: casting a pointer to an integer only reads its address. The
        // dereference happens in JavaScript, against the same linear memory.
        let offset = input.bytes.as_ptr() as usize;
        cell.set(input);
        offset
    })
}

/// Reserves `len` bytes for the formula and returns the offset to write at.
///
/// Read `instance.exports.memory.buffer` **after** this call, never before:
/// see the glue contract in the crate documentation. A return of `0` is a
/// refusal, not an offset: write nothing and call [`check`], which will return
/// [`REFUSED`].
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
/// see the glue contract in the crate documentation. A return of `0` is a
/// refusal, not an offset: write nothing and call [`check`], which will return
/// [`REFUSED`].
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
/// [`REFUSED`] if either input was over [`MAX_INPUT_BYTES`] when it was
/// reserved. That is not a fourth verdict and must never be shown as one: it
/// says this module declined to hold the file, and the CLI has no such ceiling.
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

            // Before anything is read. A refused input has no bytes, and
            // checking an empty formula against an empty proof would report
            // NOT VERIFIED — a verdict, about a file this module never held.
            let code = if formula.refused || proof.refused {
                REFUSED
            } else {
                // The same entry point the CLI uses, under the same default
                // limits. "A formula we cannot read is a proof we cannot
                // accept" lives inside it rather than here, which is what makes
                // the module's verdict the checker's verdict rather than a
                // second opinion about it.
                let outcome = refute::check_readers(
                    formula.bytes.as_slice(),
                    proof.bytes.as_slice(),
                    &Limits::default(),
                );

                // Exhaustive, and asserted so from the checker crate's test
                // suite. A wildcard arm here is the one defect that would let
                // this module report a verdict the checker never gave.
                match outcome.verdict {
                    Verdict::Verified => VERIFIED,
                    Verdict::NotVerified(_) => NOT_VERIFIED,
                    Verdict::Unsupported(_) => UNSUPPORTED,
                }
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

    use super::{
        check, cnf_reserve, proof_reserve, Input, MAX_INPUT_BYTES, NOT_VERIFIED, REFUSED,
        REFUSED_OFFSET, UNSUPPORTED, VERIFIED,
    };

    /// Writes `bytes` into the buffer the way the glue does.
    ///
    /// The host build has no linear memory to index, so this reaches the same
    /// `Vec` through the same `thread_local` instead. What it exercises is the
    /// contract — reserve, write, check — and not the JavaScript side of it,
    /// which is the Node harness's job.
    fn write(slot: &'static std::thread::LocalKey<std::cell::Cell<Input>>, bytes: &[u8]) {
        slot.with(|cell| {
            let mut input = cell.take();
            input.bytes.clear();
            input.bytes.extend_from_slice(bytes);
            cell.set(input);
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
                let input = cell.take();
                let seen = input.bytes.as_ptr() as usize;
                cell.set(input);
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

    // ---------------------------------------------------------------------
    // The size refusal. W5 of the test plan, plus the assumption it rests on.

    /// The sentinel the whole refusal contract depends on.
    ///
    /// If a legitimate reserve could ever return zero, the glue could not tell
    /// a refusal from an offset, and a page would write the user's proof over
    /// the bottom of linear memory. An empty `Vec<u8>` is dangling-but-aligned
    /// at 1, not null, and this is the test that says so out loud.
    #[test]
    fn a_zero_length_reserve_still_returns_a_usable_offset() {
        assert_ne!(cnf_reserve(0), REFUSED_OFFSET);
        assert_ne!(proof_reserve(0), REFUSED_OFFSET);
    }

    #[test]
    fn a_proof_one_byte_over_the_ceiling_is_refused() {
        let _ = cnf_reserve(TINY_CNF.len());
        write(&super::FORMULA, TINY_CNF);
        // No allocation happens, which is the point: the refusal is a decision
        // taken with the length in hand, not a failure discovered by trying.
        assert_eq!(proof_reserve(MAX_INPUT_BYTES + 1), REFUSED_OFFSET);
        assert_eq!(check(), REFUSED);
    }

    #[test]
    fn a_formula_one_byte_over_the_ceiling_is_refused_too() {
        // The design states the refusal on proof size. There are two files.
        assert_eq!(cnf_reserve(MAX_INPUT_BYTES + 1), REFUSED_OFFSET);
        let _ = proof_reserve(TINY_PROOF.len());
        write(&super::PROOF, TINY_PROOF);
        assert_eq!(check(), REFUSED);
    }

    #[test]
    fn the_ceiling_itself_is_accepted() {
        // A boundary written `>=` instead of `>` would refuse a file it can
        // hold, and nothing else in this suite would notice.
        assert_ne!(proof_reserve(MAX_INPUT_BYTES), REFUSED_OFFSET);
    }

    /// A refusal must not leave the previous input checkable.
    ///
    /// This is the one that would have been a wrong answer rather than a
    /// missing one: verify a proof, then reserve one too large, and a module
    /// that kept the old bytes would report `VERIFIED` for a file the user had
    /// already replaced.
    #[test]
    fn a_refusal_discards_what_was_there_before() {
        load(TINY_CNF, TINY_PROOF);
        assert_eq!(check(), VERIFIED);
        assert_eq!(proof_reserve(MAX_INPUT_BYTES + 1), REFUSED_OFFSET);
        assert_eq!(check(), REFUSED);
    }

    /// And a good reserve after a refused one clears the refusal.
    #[test]
    fn reserving_within_the_ceiling_again_clears_the_refusal() {
        assert_eq!(proof_reserve(MAX_INPUT_BYTES + 1), REFUSED_OFFSET);
        load(TINY_CNF, TINY_PROOF);
        assert_eq!(check(), VERIFIED);
    }
}
