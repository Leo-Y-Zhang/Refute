//! The forward DRAT checker.
//!
//! One pass, forward, streaming, and — unlike milestone 1's — with no help at
//! all from the file. There is no hint list to walk and no claim to police, so
//! the enumeration *is* the check: for a lemma that unit propagation does not
//! refute, every live clause holding the negated pivot is found by this
//! checker, from its own database, and its resolvent has to follow.
//!
//! That is a real simplification and worth saying plainly. Milestone 1b needed
//! four strictness rules to keep a hint list honest, and each carried a
//! false-rejection risk against a producer that wrote its hints differently.
//! This path has **one** new rejection reason and no such risk. What it has
//! instead is a propagation engine, which is the first thing in this project
//! that could accept a bad proof with nothing in the file to contradict it.
//! Every control in `tests/drat.rs` that runs over a satisfiable formula is
//! there for that.

use std::io::BufRead;

use crate::checker::Stats;
use crate::cnf::Cnf;
use crate::drat::store::{Store, UNSET};
use crate::drat::{DratReader, DratStep};
use crate::limits::Limits;
use crate::lit::Lit;
use crate::parse::ParseErrorKind;
use crate::verdict::{EmptyClauseDerived, Reason, Rejection, Unsupported, Verdict};

/// The variable is assigned true. Kept beside the store's own encoding.
const VAR_TRUE: u8 = 1;

/// Checks a DRAT proof against a formula.
///
/// Total: a verdict for every input, no panic, no unbounded allocation, and no
/// read past the first failing step.
///
/// "No unbounded allocation" is a claim about the ceiling, not about the
/// constant. A variable here costs about ninety-six bytes -- an assignment
/// byte, and a 24-byte slot in each of `watches` and `occ` for each of its two
/// literals -- so the bound that matters on this path is
/// [`Limits::max_drat_var`], not [`Limits::max_var`], and both the formula and
/// the proof are held to it.
pub(crate) fn check_with_stats<R: BufRead>(
    cnf: &Cnf,
    proof: DratReader<R>,
    limits: &Limits,
) -> (Verdict, Stats) {
    let mut checker = Checker {
        store: Store::new(cnf, limits),
    };
    let verdict = checker.run(proof);
    (verdict, checker.stats())
}

struct Checker {
    store: Store,
}

/// A refusal on its way out of a step check: the reason, and the candidate it
/// happened against, if any.
struct Refusal {
    reason: Reason,
    candidate: Option<u32>,
}

impl Checker {
    /// The run's counters, with the store walked once at the end for the four
    /// that describe what it holds rather than what it did.
    ///
    /// `compactions` and `occurrence_entries_filtered` come through the store's
    /// own counters, because they count events and not bytes.
    fn stats(&self) -> Stats {
        let footprint = self.store.footprint();
        Stats {
            assignment_slots: self.store.assignment_slots(),
            store_bytes: footprint.store_bytes,
            live_arena_bytes: footprint.live_arena_bytes,
            dead_arena_bytes: footprint.dead_arena_bytes,
            deletion_index_entries: footprint.deletion_index_entries,
            ..self.store.stats
        }
    }

    fn run<R: BufRead>(&mut self, proof: DratReader<R>) -> Verdict {
        for step in proof {
            let step = match step {
                Ok(step) => step,
                // Fail closed: a proof we cannot read is a proof we cannot
                // accept. One kind is answered differently, and only one — a
                // binary proof is not a bad certificate, it is the wrong file.
                Err(err) => {
                    if matches!(err.kind, ParseErrorKind::BinaryProof) {
                        return Verdict::Unsupported(Unsupported::BinaryProof { line: err.line });
                    }
                    return Verdict::NotVerified(Rejection {
                        step: None,
                        line: 0,
                        resolvent: None,
                        reason: Reason::Parse(err),
                    });
                }
            };
            match step {
                DratStep::Delete { lits, .. } => {
                    self.store.stats.deletions = self.store.stats.deletions.saturating_add(1);
                    if !self.store.delete(&lits) {
                        // Permissive, and sound: deletion only ever removes
                        // tools from the checker. A spurious one can cause a
                        // later rejection but never a false `VERIFIED`.
                        self.store.stats.unknown_deletions =
                            self.store.stats.unknown_deletions.saturating_add(1);
                    }
                }
                DratStep::Add { lits, line } => {
                    if let Some(verdict) = self.add(&lits, line) {
                        return verdict;
                    }
                }
            }
        }
        Verdict::NotVerified(Rejection {
            step: None,
            line: 0,
            resolvent: None,
            reason: Reason::NoEmptyClause,
        })
    }

    /// Returns `Some` when the run is over, `None` to continue.
    fn add(&mut self, lits: &[Lit], line: u64) -> Option<Verdict> {
        self.store.stats.additions = self.store.stats.additions.saturating_add(1);
        // The identifier this lemma will take if it is accepted. Reported in
        // the rejection so that a reader can count to it, and it is the LRAT
        // numbering: originals `1..n`, lemmas from `n + 1`.
        let id = self.store.next_id();

        if let Err(refusal) = self.check(lits) {
            return Some(Verdict::NotVerified(Rejection {
                step: Some(id),
                line,
                resolvent: refusal.candidate.map(u64::from),
                reason: refusal.reason,
            }));
        }

        let derived_empty_clause = lits.is_empty();
        self.store.add(lits);
        if derived_empty_clause {
            // The one site in this checker that builds the evidence, reached
            // only after the step that added the empty clause returned `Ok`.
            return Some(crate::verdict::verified(EmptyClauseDerived(())));
        }
        None
    }

    /// The step check, exactly as specified in `docs/TDD.md` part 3.
    ///
    /// A clause `C` is RAT on a pivot `p` in `C` with respect to `F` when, for
    /// every clause `D` in `F` holding `-p`, the resolvent `C or (D \ {-p})`
    /// follows from `F` by unit propagation. Adding such a clause preserves
    /// satisfiability, so if `F + C` is unsatisfiable then `F` is. `F` is the
    /// *live* database, and a superset of an unsatisfiable set is
    /// unsatisfiable, so a step checked against the smaller formula still
    /// refutes the larger one.
    ///
    /// Three places a checker loses that argument:
    ///
    /// 1. **The candidate set must be complete.** Miss one clause holding `-p`
    ///    and the condition was never checked, so an arbitrary clause is added
    ///    on evidence that looks fine. It comes from the occurrence index,
    ///    which deletion maintains eagerly for exactly this reason.
    /// 2. **The vacuous case must be proved, not assumed.** A lemma whose
    ///    pivot has no live candidate is accepted after the enumeration
    ///    returns nothing — the same loop, zero iterations. There is no
    ///    separate path for it and so nothing for the two to drift apart on.
    /// 3. **Propagation must derive only what is implied.** New in this
    ///    milestone and with no analogue in the LRAT path, where the
    ///    producer's hints named every propagation. A bug that assigns a
    ///    literal no clause forces makes every check easier to pass, which is
    ///    why the satisfiable-formula fixtures are the controls that matter.
    fn check(&mut self, lits: &[Lit]) -> Result<(), Refusal> {
        // The trail is empty here: every step unwinds itself completely.
        if self.assume_negated(lits) {
            self.store.unwind(0);
            self.store.stats.tautological_additions =
                self.store.stats.tautological_additions.saturating_add(1);
            return Ok(());
        }
        if self.store.propagate_from_scratch() {
            self.store.unwind(0);
            self.store.stats.rup_additions = self.store.stats.rup_additions.saturating_add(1);
            return Ok(());
        }

        // The empty clause has no first literal, so it has no pivot, so the
        // RAT condition cannot be evaluated at all. A checker that treats the
        // last line as a formality accepts every file that ends in `0`, which
        // is every file.
        let pivot = match lits.first() {
            Some(pivot) => *pivot,
            None => {
                self.store.unwind(0);
                return Err(Refusal {
                    reason: Reason::NoConflict,
                    candidate: None,
                });
            }
        };
        self.store.stats.rat_additions = self.store.stats.rat_additions.saturating_add(1);

        // Every candidate starts from here: the negated lemma *and* everything
        // unit propagation already derived from it. Dropping the second half
        // reads perfectly well and rejects every real proof.
        let base = self.store.trail_len();
        let candidates = self.store.resolution_candidates(pivot);
        for candidate in candidates {
            self.store.stats.rat_candidates_checked =
                self.store.stats.rat_candidates_checked.saturating_add(1);
            let refuted = self.check_resolvent(candidate, pivot, base);
            // EVERY candidate starts from `base`. Milestone 1b shipped this
            // line with no test pinning it, and deleting it left 77 tests
            // green while the checker printed `s VERIFIED` on a formula
            // `kissat` reports satisfiable. Here it is worse, because there is
            // no file to disagree with.
            self.store.unwind(base);
            if !refuted {
                self.store.unwind(0);
                return Err(Refusal {
                    reason: Reason::RatCheckFailed { pivot },
                    candidate: Some(candidate),
                });
            }
        }

        // No early exit above, and none here. The loop visits every live
        // clause holding the negated pivot, because RAT is a claim about all
        // of them, and a candidate whose resolvent is trivially refuted is
        // still a candidate that was checked.
        self.store.unwind(0);
        Ok(())
    }

    /// One candidate: assume the negation of the resolvent and propagate.
    ///
    /// Returns `true` when the resolvent is refuted, which is what the RAT
    /// condition asks for.
    fn check_resolvent(&mut self, candidate: u32, pivot: Lit, base: usize) -> bool {
        let negated_pivot = pivot.negate();
        for lit in self.store.clause(candidate) {
            if lit == negated_pivot {
                // Resolved away: it is not part of the resolvent.
                continue;
            }
            match self.store.value(lit) {
                // Already true, so negating it conflicts at once: the
                // resolvent is refuted by its own negation alone.
                VAR_TRUE => return true,
                UNSET => self.store.assign_true(lit.negate()),
                _ => {}
            }
        }
        self.store.propagate(base)
    }

    /// Puts the negation of a lemma on the trail.
    ///
    /// Returns `true` when the lemma is a tautology, which the caller accepts:
    /// adding `x or not-x` preserves satisfiability and it can never be the
    /// empty clause. This is the one permissive rule on an addition, and
    /// rejecting instead would be a false rejection with no safety benefit.
    ///
    /// A repeated literal is *not* a tautology. `-2 -2` is the clause `-2`,
    /// and assigning it twice is idempotent; a checker that reads the repeat
    /// as `x or not-x` accepts a lemma before anything is checked at all.
    fn assume_negated(&mut self, lits: &[Lit]) -> bool {
        for lit in lits {
            match self.store.value(*lit) {
                VAR_TRUE => return true,
                UNSET => self.store.assign_true(lit.negate()),
                // A repeated literal. Assigning it again is idempotent.
                _ => {}
            }
        }
        false
    }
}
