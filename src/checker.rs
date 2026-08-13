//! The forward LRAT checker.
//!
//! One pass, forward, streaming. No watched literals, no occurrence lists, no
//! propagation engine. That is the whole reason to do LRAT before DRAT: with
//! hints, checking a step is a bounded walk over a list, and the soundness
//! argument fits in a paragraph.

use std::collections::HashMap;
use std::io::BufRead;

use crate::cnf::{parse_dimacs, Cnf, Warning};
use crate::limits::Limits;
use crate::lit::{Clause, ClauseId, Lit};
use crate::lrat::{Hints, LratReader, Step};
use crate::parse::ParseErrorKind;
use crate::verdict::{Reason, Rejection, Unsupported, Verdict};

/// Unassigned.
const UNSET: u8 = 0;
/// The variable is assigned true.
const VAR_TRUE: u8 = 1;
/// The variable is assigned false.
const VAR_FALSE: u8 = 2;

/// Counters for `--stats`. Cheap enough to keep unconditionally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Addition steps checked.
    pub additions: u64,
    /// Deletion steps processed.
    pub deletions: u64,
    /// Hint lookups resolved against the clause database.
    pub hints_resolved: u64,
    /// Largest number of clauses alive at once. The number that decides
    /// whether a proof fits in a browser tab.
    pub peak_live_clauses: usize,
    /// Deletions naming an identifier that was not present. Counted, not
    /// rejected: see the note on `Delete` handling below.
    pub unknown_deletions: u64,
    /// Literals assigned over the whole run, one per push onto the trail.
    pub assignments: u64,
    /// Assignments undone while unwinding the trail.
    ///
    /// Equal to `assignments` at the end of every completed run, because the
    /// trail is emptied after each step. The pair is what makes "the unwind is
    /// O(assigned) and not O(vars)" a measurement rather than a claim: an
    /// unwind that cleared the whole assignment vector would undo far more
    /// than was ever assigned, or — clearing without counting — nothing at
    /// all. `b13_large_formula_unwinds_in_proportion_to_assignments` asserts
    /// both directions.
    pub assignments_undone: u64,
    /// Slots in the assignment vector, one per variable it can hold.
    ///
    /// Sized from the largest variable the formula actually mentions and grown
    /// on demand after that — never from the count the `p` line declares.
    /// `p cnf 4294967295 1` is nineteen bytes, and taking its word for it buys
    /// a 64 MB allocation, capped only by `Limits::max_var`.
    pub assignment_slots: usize,
}

/// Everything one run produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The verdict.
    pub verdict: Verdict,
    /// Non-fatal oddities in the formula, for the caller to print.
    pub warnings: Vec<Warning>,
    /// Counters.
    pub stats: Stats,
}

/// Checks `proof` against `cnf`.
///
/// Total: returns a verdict for every input, including garbage. Never panics,
/// never allocates unboundedly, never reads past the first failing step.
pub fn check<R: BufRead>(cnf: &Cnf, proof: LratReader<R>, limits: &Limits) -> Verdict {
    check_with_stats(cnf, proof, limits).0
}

/// Parses a formula and checks a proof against it, in one call.
///
/// The mapping "a formula we cannot read is a proof we cannot accept" lives
/// here rather than in `main`, so that it is covered by the test suite and so
/// that the milestone-4 WASM entry point cannot accidentally differ from the
/// CLI. Warnings are returned for the caller to print; the library never does.
pub fn check_readers<F: BufRead, P: BufRead>(formula: F, proof: P, limits: &Limits) -> Outcome {
    let cnf = match parse_dimacs(formula, limits) {
        Ok(cnf) => cnf,
        Err(err) => {
            return Outcome {
                verdict: Verdict::NotVerified(Rejection {
                    step: None,
                    line: 0,
                    reason: Reason::Parse(err),
                }),
                warnings: Vec::new(),
                stats: Stats::default(),
            }
        }
    };
    let warnings = cnf.warnings.clone();
    let (verdict, stats) = check_with_stats(&cnf, LratReader::new(proof, limits), limits);
    Outcome {
        verdict,
        warnings,
        stats,
    }
}

/// [`check`], with the counters.
pub fn check_with_stats<R: BufRead>(
    cnf: &Cnf,
    proof: LratReader<R>,
    limits: &Limits,
) -> (Verdict, Stats) {
    let mut state = Checker::new(cnf, limits);
    let verdict = state.run(proof);
    (verdict, state.stats())
}

/// Every clause enters the database duplicate-free.
///
/// A repeated literal is the same literal: `1 2 -3 -3` is the clause `1 2 -3`.
/// Counting the repeat as a second free literal calls that clause non-unit
/// under an assignment that leaves only it free, and rejects a proof
/// `drat-trim` verifies — measured, and pinned by `p7_repeated_literal_in_a_
/// formula_clause_verifies`. The trail already treats a repeat in a lemma as
/// idempotent; the database now agrees with it, so a lemma reused as a hint is
/// classified the same way its own literals were.
///
/// A tautological clause — one holding both `l` and `-l` — is kept as it
/// stands. It is satisfied whenever its variable is assigned and has two free
/// literals otherwise, so it can never be unit and never be falsified: it can
/// only ever cause a rejection, never a verification. Dropping it would be
/// sound for the same reason deleting a clause is, but it would trade
/// `HintSatisfied` for `MissingHint` and leave the database saying something
/// the file did not say.
fn normalize(lits: &[Lit]) -> Clause {
    let mut literals = lits.to_vec();
    literals.sort_unstable();
    literals.dedup();
    literals.into_boxed_slice()
}

struct Checker {
    db: HashMap<ClauseId, Clause>,
    assign: Vec<u8>,
    trail: Vec<u32>,
    last_added_id: ClauseId,
    max_var: u32,
    stats: Stats,
}

impl Checker {
    fn new(cnf: &Cnf, limits: &Limits) -> Self {
        let mut db = HashMap::with_capacity(cnf.clauses.len());
        for (position, clause) in cnf.clauses.iter().enumerate() {
            // Identifiers are one-based positions, which is what LRAT hints
            // refer to. `position` is bounded by the file length.
            let id = ClauseId::try_from(position).unwrap_or(ClauseId::MAX);
            db.insert(id.saturating_add(1), normalize(clause));
        }
        let max_var = cnf.num_vars.min(limits.max_var);
        let size = usize::try_from(max_var)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        Self {
            last_added_id: ClauseId::try_from(cnf.clauses.len()).unwrap_or(ClauseId::MAX),
            stats: Stats {
                peak_live_clauses: db.len(),
                ..Stats::default()
            },
            db,
            assign: vec![UNSET; size],
            trail: Vec::new(),
            max_var,
        }
    }

    /// The counters, with the ones only the checker itself can measure.
    fn stats(&self) -> Stats {
        Stats {
            assignment_slots: self.assign.len(),
            ..self.stats
        }
    }

    fn run<R: BufRead>(&mut self, proof: LratReader<R>) -> Verdict {
        for step in proof {
            let step = match step {
                Ok(step) => step,
                // Fail closed: a proof we cannot read is a proof we cannot
                // accept. The error carries its own line number.
                //
                // One kind is answered differently, and it is the only
                // weakening in this crate: a binary proof is not a bad
                // certificate, it is the wrong file, and calling it a
                // rejection is the confusion Refute exists to remove. The kind
                // is named explicitly and singly, so nothing else can drift
                // into exit 2, and it has no route to `Verified` at all.
                Err(err) => {
                    if matches!(err.kind, ParseErrorKind::BinaryProof) {
                        return Verdict::Unsupported(Unsupported::BinaryProof { line: err.line });
                    }
                    return Verdict::NotVerified(Rejection {
                        step: None,
                        line: 0,
                        reason: Reason::Parse(err),
                    });
                }
            };
            match step {
                Step::Delete { ids, .. } => {
                    self.stats.deletions = self.stats.deletions.saturating_add(1);
                    for id in ids {
                        if self.db.remove(&id).is_none() {
                            // Permissive, and sound: deletion only ever removes
                            // tools from the checker. A spurious deletion can
                            // cause a later MissingHint but can never cause a
                            // false VERIFIED, and rejecting it would refuse
                            // other producers' output for no safety gain.
                            self.stats.unknown_deletions =
                                self.stats.unknown_deletions.saturating_add(1);
                        }
                    }
                }
                Step::Add {
                    id,
                    lits,
                    hints,
                    line,
                } => {
                    if let Some(verdict) = self.add(id, lits, hints, line) {
                        return verdict;
                    }
                }
            }
        }
        Verdict::NotVerified(Rejection {
            step: None,
            line: 0,
            reason: Reason::NoEmptyClause,
        })
    }

    /// Returns `Some` when the run is over, `None` to continue.
    fn add(&mut self, id: ClauseId, lits: Vec<Lit>, hints: Hints, line: u64) -> Option<Verdict> {
        // Classified before anything else, and before RUP is attempted on it:
        // running RUP on a lemma whose hints we do not understand would reject
        // a valid proof, and accepting it would accept anything.
        let hints = match hints {
            Hints::Rat { .. } => return Some(Verdict::Unsupported(Unsupported::RatHints { line })),
            Hints::Empty => return Some(Verdict::Unsupported(Unsupported::EmptyHints { line })),
            Hints::Rup(hints) => hints,
        };

        let reject = |reason: Reason| {
            Some(Verdict::NotVerified(Rejection {
                step: Some(id),
                line,
                reason,
            }))
        };

        // Monotonicity is also what rules out reusing an identifier, so there
        // is no separate duplicate check. Every key in the database is at most
        // `last_added_id`: the formula's occupy 1..=n where n is the starting
        // value, deletion only removes keys, and an addition inserts `id`
        // immediately after raising `last_added_id` to it. An `id` that gets
        // past the test above is therefore larger than every key present. A
        // rejection reason no input can produce is decoration, and this one
        // was: `Reason::DuplicateId` was removed with the check.
        if id <= self.last_added_id {
            return reject(Reason::NonMonotonicId {
                got: id,
                previous: self.last_added_id,
            });
        }
        self.stats.additions = self.stats.additions.saturating_add(1);

        if let Err(reason) = self.check_rup(&lits, &hints) {
            return reject(reason);
        }

        self.last_added_id = id;
        let derived_empty_clause = lits.is_empty();
        self.db.insert(id, normalize(&lits));
        self.stats.peak_live_clauses = self.stats.peak_live_clauses.max(self.db.len());

        if derived_empty_clause {
            return Some(self.finish_with_empty_clause());
        }
        None
    }

    /// The only place in this crate that produces a verdict of `Verified`.
    ///
    /// Called from exactly one site, immediately after the step that added the
    /// empty clause returned `Ok`. `tests/trust_boundary.rs` fails if a second
    /// site ever appears.
    fn finish_with_empty_clause(&self) -> Verdict {
        Verdict::Verified
    }

    /// Puts the negation of a lemma on the trail.
    ///
    /// Returns `true` when the lemma is a tautology, which the caller accepts:
    /// adding `x or not-x` preserves satisfiability and it can never be the
    /// empty clause. This is the one permissive rule on an addition, and
    /// rejecting instead would be a false rejection with no safety benefit.
    fn assume_negated(&mut self, lits: &[Lit]) -> bool {
        for lit in lits {
            match self.value(*lit) {
                VAR_TRUE => return true,
                // A repeated literal. Assigning it again is idempotent.
                VAR_FALSE => {}
                _ => self.assign_true(lit.negate()),
            }
        }
        false
    }

    /// Milestone 1's hint walk, with the unwinding taken out of it.
    ///
    /// `Ok(Some(hint))` means that hint was falsified and nothing before it
    /// was; `Ok(None)` means the list ran out with no conflict; `Err` is one of
    /// the four hint rejections. The caller owns the trail mark, because a RAT
    /// step walks its prefix and then keeps those propagations: every resolvent
    /// block is checked from the negated lemma *plus* the prefix, and without
    /// them 100 % of the RAT lines measured for `docs/TDD.md` part 2 fail.
    fn walk(&mut self, hints: &[ClauseId]) -> Result<Option<ClauseId>, Reason> {
        let last = hints.len().saturating_sub(1);
        for (position, hint) in hints.iter().enumerate() {
            self.stats.hints_resolved = self.stats.hints_resolved.saturating_add(1);
            let clause = match self.db.get(hint) {
                Some(clause) => clause.clone(),
                None => return Err(Reason::MissingHint(*hint)),
            };

            let mut satisfied = false;
            let mut free: Option<Lit> = None;
            let mut free_count: usize = 0;
            for lit in clause.iter() {
                match self.value(*lit) {
                    VAR_TRUE => {
                        satisfied = true;
                        break;
                    }
                    VAR_FALSE => {}
                    _ => {
                        free_count = free_count.saturating_add(1);
                        if free.is_none() {
                            free = Some(*lit);
                        }
                    }
                }
            }

            if satisfied {
                // In a well-formed derivation no hint is satisfied where it is
                // used. This is the rule that catches a valid proof of a
                // different formula.
                return Err(Reason::HintSatisfied(*hint));
            }
            match (free_count, free) {
                (0, _) => {
                    return if position == last {
                        Ok(Some(*hint))
                    } else {
                        // Sound to accept, but a conflict before the last hint
                        // means the list was reordered or padded, and real
                        // output never does it.
                        Err(Reason::EarlyConflict(*hint))
                    };
                }
                (1, Some(lit)) => self.assign_true(lit),
                _ => return Err(Reason::HintNotUnit(*hint)),
            }
        }

        Ok(None)
    }

    /// The step check, exactly as specified in `docs/TDD.md`.
    fn check_rup(&mut self, lits: &[Lit], hints: &[ClauseId]) -> Result<(), Reason> {
        let mark = self.trail.len();
        if self.assume_negated(lits) {
            self.unwind(mark);
            return Ok(());
        }
        let walked = self.walk(hints);
        self.unwind(mark);
        match walked {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(Reason::NoConflict),
            Err(reason) => Err(reason),
        }
    }

    fn value(&self, lit: Lit) -> u8 {
        let index = usize::try_from(lit.var()).unwrap_or(usize::MAX);
        match self.assign.get(index) {
            Some(&VAR_TRUE) => {
                if lit.is_negated() {
                    VAR_FALSE
                } else {
                    VAR_TRUE
                }
            }
            Some(&VAR_FALSE) => {
                if lit.is_negated() {
                    VAR_TRUE
                } else {
                    VAR_FALSE
                }
            }
            _ => UNSET,
        }
    }

    fn assign_true(&mut self, lit: Lit) {
        let var = lit.var();
        if var > self.max_var {
            // A literal the formula never mentioned. The parser has already
            // bounded it by Limits::max_var, so this growth is bounded too.
            self.max_var = var;
        }
        let index = usize::try_from(var).unwrap_or(usize::MAX);
        if index >= self.assign.len() {
            self.assign.resize(index.saturating_add(1), UNSET);
        }
        if let Some(slot) = self.assign.get_mut(index) {
            *slot = if lit.is_negated() {
                VAR_FALSE
            } else {
                VAR_TRUE
            };
            self.trail.push(var);
            self.stats.assignments = self.stats.assignments.saturating_add(1);
        }
    }

    /// Unwound in O(assigned), never by clearing the whole vector. Clearing is
    /// O(vars) per step, which is quadratic on a 100,000-variable formula; B13
    /// in the test suite is the tripwire, and it counts the writes rather than
    /// timing them, because a release build is fast enough to hide the
    /// difference on the sizes a test can afford.
    fn unwind(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop() {
                Some(var) => {
                    let index = usize::try_from(var).unwrap_or(usize::MAX);
                    if let Some(slot) = self.assign.get_mut(index) {
                        *slot = UNSET;
                        self.stats.assignments_undone =
                            self.stats.assignments_undone.saturating_add(1);
                    }
                }
                None => break,
            }
        }
    }
}
