//! The forward LRAT checker.
//!
//! One pass, forward, streaming. No watched literals, no occurrence lists, no
//! propagation engine. That is the whole reason to do LRAT before DRAT: with
//! hints, checking a step is a bounded walk over a list, and the soundness
//! argument fits in a paragraph.

use std::collections::{BTreeSet, HashMap};
use std::io::BufRead;

use crate::cnf::{parse_dimacs, Cnf, Warning};
use crate::drat::DratReader;
use crate::format::Format;
use crate::limits::Limits;
use crate::lit::{Clause, ClauseId, Lit};
use crate::lrat::{Hints, LratReader, ResolventBlock, Step};
use crate::parse::ParseErrorKind;
use crate::verdict::{EmptyClauseDerived, Reason, Rejection, Unsupported, Verdict};

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
    /// Additions carrying at least one resolvent block.
    pub rat_additions: u64,
    /// Additions with no hints at all: a claim that the pivot has no
    /// resolution candidate, which the checker then establishes for itself.
    pub vacuous_rat_additions: u64,
    /// Resolvent blocks checked.
    pub resolvent_blocks: u64,
    /// Additions that scanned the clause database for resolution candidates.
    ///
    /// Equal to `rat_additions + vacuous_rat_additions` on every proof in the
    /// corpus, and asserted so on every positive fixture. That equality is
    /// what catches a checker that scans on every addition, and — the one that
    /// matters — a checker that forgets to scan on the vacuous ones, which is
    /// the largest false-accept hole in this milestone.
    ///
    /// The one construct that would break it is a *tautological* RAT lemma,
    /// which is accepted before the scan by the same permissive rule
    /// milestone 1 applies to every addition. No solver emits one, and no
    /// measured file contains one.
    pub candidate_scans: u64,
    /// Clauses visited by those scans.
    ///
    /// The one performance bet in the design, made countable. See
    /// `Checker::resolution_candidates`: if this ever exceeds the hint literal
    /// visits on a real proof, the occurrence index is worth building.
    pub candidates_examined: u64,
    /// Resolution candidates those scans found. Equal to `resolvent_blocks` on
    /// a proof that verifies, because the blocks must name the set exactly.
    pub resolution_candidates: u64,
    /// Slots in the assignment vector, one per variable it can hold.
    ///
    /// Sized from the largest variable the formula actually mentions and grown
    /// on demand after that — never from the count the `p` line declares.
    /// `p cnf 4294967295 1` is nineteen bytes, and taking its word for it buys
    /// a 64 MB allocation, capped only by `Limits::max_var`.
    pub assignment_slots: usize,
    /// Literals assigned by unit propagation. DRAT only.
    ///
    /// Zero on the LRAT path, where the producer's hints name every
    /// propagation and there is no engine to count.
    pub propagations: u64,
    /// Clauses inspected in watch lists. DRAT only.
    pub watch_visits: u64,
    /// Occurrence-index slots written or cleared. DRAT only.
    ///
    /// The one performance bet in milestone 2, made countable. Part 2 measured
    /// the same choice and took the scan; part 3 measured it again on raw
    /// proofs and took the index, because `drat-trim`'s LRAT deletes far
    /// harder than the file the solver wrote — 159 live clauses on average
    /// against 666 for the same instance. The trigger for going back is
    /// written down: if this ever exceeds RAT additions times mean live
    /// clauses on a real proof, return to the scan.
    pub occurrence_updates: u64,
    /// Additions accepted because unit propagation reached a conflict. DRAT
    /// only; on the LRAT path the hint walk is the same thing under a name the
    /// file chose.
    pub rup_additions: u64,
    /// Additions accepted because the lemma holds a literal and its negation.
    ///
    /// The one permissive rule on an addition, on both paths: adding `x or
    /// not-x` preserves satisfiability and it can never be the empty clause.
    /// Counted so that `rup + rat + tautological == additions` is an identity
    /// a test can assert rather than a sentence in a document.
    pub tautological_additions: u64,
    /// Resolution candidates examined by the RAT check. DRAT only.
    ///
    /// Every live clause holding the negated pivot, every time — the loop has
    /// no early exit, because RAT is a claim about all of them.
    pub rat_candidates_checked: u64,
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
    /// Which reader read the proof.
    ///
    /// Reported rather than printed. The verdict line is identical whichever
    /// checker ran, and a reader who cannot tell them apart from it is reading
    /// the contract correctly; this is here for `--stats`, which prints the
    /// DRAT counter line only when the DRAT checker ran, so that the block is
    /// never a wall of zeroes.
    pub format: Format,
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
pub fn check_readers<F: BufRead, P: BufRead>(formula: F, mut proof: P, limits: &Limits) -> Outcome {
    // The first kilobyte, and no more. A peek is not a read: `fill_buf` leaves
    // every byte where it was, so the reader handed on below still starts at
    // the beginning of the file. A reader that returns nothing here — because
    // it is empty, or because it failed — is classified as LRAT, which is the
    // default arm, and the failure is reported by the reader with a line
    // number rather than swallowed here without one.
    let head: Vec<u8> = match proof.fill_buf() {
        Ok(buffered) => buffered
            .get(..crate::format::PEEK_BYTES)
            .unwrap_or(buffered)
            .to_vec(),
        Err(_) => Vec::new(),
    };
    let format = crate::format::detect(&head, limits);
    check_readers_with_format(formula, proof, limits, format)
}

/// [`check_readers`], with the format supplied rather than detected.
///
/// What `--drat` and `--lrat` call. Nothing in this library dispatches on a
/// path: an extension is a claim by whoever named the file, and a claim is not
/// evidence.
///
/// Forcing the wrong format is a rejection, never a wrong acceptance. Each
/// checker is sound for its own grammar, so a file the DRAT checker verifies
/// refutes the formula when read as DRAT, whatever its author meant it to be.
pub fn check_readers_with_format<F: BufRead, P: BufRead>(
    formula: F,
    proof: P,
    limits: &Limits,
    format: Format,
) -> Outcome {
    let cnf = match parse_dimacs(formula, limits) {
        Ok(cnf) => cnf,
        Err(err) => {
            return Outcome {
                verdict: Verdict::NotVerified(Rejection {
                    step: None,
                    line: 0,
                    resolvent: None,
                    reason: Reason::Parse(err),
                }),
                warnings: Vec::new(),
                stats: Stats::default(),
                format,
            }
        }
    };
    let warnings = cnf.warnings.clone();
    let (verdict, stats) = match format {
        Format::Lrat => check_with_stats(&cnf, LratReader::new(proof, limits), limits),
        Format::Drat => {
            crate::drat::checker::check_with_stats(&cnf, DratReader::new(proof, limits), limits)
        }
    };
    Outcome {
        verdict,
        warnings,
        stats,
        format,
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

/// A refusal on its way out of a step check: the reason, and the resolvent
/// block it happened inside, if any.
struct Refusal {
    reason: Reason,
    resolvent: Option<ClauseId>,
}

impl Refusal {
    fn new(reason: Reason) -> Self {
        Self {
            reason,
            resolvent: None,
        }
    }

    /// A refusal that happened while checking the resolvent with `clause`.
    fn at(reason: Reason, clause: ClauseId) -> Self {
        Self {
            reason,
            resolvent: Some(clause),
        }
    }
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
                        resolvent: None,
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
            resolvent: None,
            reason: Reason::NoEmptyClause,
        })
    }

    /// Returns `Some` when the run is over, `None` to continue.
    fn add(&mut self, id: ClauseId, lits: Vec<Lit>, hints: Hints, line: u64) -> Option<Verdict> {
        // Classified before anything else and before any checking, because
        // running RUP on a RAT lemma rejects a valid proof and taking a RAT
        // lemma on trust accepts anything.
        //
        // The counters are bumped here rather than beside the scan they
        // predict, so that `candidate_scans == rat_additions +
        // vacuous_rat_additions` is evidence about the checker rather than an
        // identity it satisfies by construction.
        match &hints {
            Hints::Rat { .. } => {
                self.stats.rat_additions = self.stats.rat_additions.saturating_add(1);
            }
            Hints::Empty => {
                self.stats.vacuous_rat_additions =
                    self.stats.vacuous_rat_additions.saturating_add(1);
            }
            Hints::Rup(_) => {}
        }

        let reject = |refusal: Refusal| {
            Some(Verdict::NotVerified(Rejection {
                step: Some(id),
                line,
                resolvent: refusal.resolvent,
                reason: refusal.reason,
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
            return reject(Refusal::new(Reason::NonMonotonicId {
                got: id,
                previous: self.last_added_id,
            }));
        }
        self.stats.additions = self.stats.additions.saturating_add(1);

        // The vacuous case is the general case with nothing in it. Giving it
        // its own code path is how the two drift, and the direction they drift
        // in is "accept the empty hint list", which accepts everything.
        let checked = match &hints {
            Hints::Rup(hints) => self.check_rup(&lits, hints),
            Hints::Rat { prefix, blocks } => self.check_rat(&lits, prefix, blocks),
            Hints::Empty => self.check_rat(&lits, &[], &[]),
        };
        if let Err(refusal) = checked {
            return reject(refusal);
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

    /// Builds the witness that this checker derived the empty clause.
    ///
    /// Called from exactly one site, immediately after the step that added the
    /// empty clause returned `Ok`. The witness is the only argument
    /// [`crate::verdict::verified`] takes, and that function is the only route
    /// to `Verdict::Verified` in the library.
    /// `tests/trust_boundary.rs` fails if either count changes.
    fn finish_with_empty_clause(&self) -> Verdict {
        crate::verdict::verified(EmptyClauseDerived(()))
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
    fn check_rup(&mut self, lits: &[Lit], hints: &[ClauseId]) -> Result<(), Refusal> {
        let mark = self.trail.len();
        if self.assume_negated(lits) {
            self.unwind(mark);
            return Ok(());
        }
        let walked = self.walk(hints);
        self.unwind(mark);
        match walked {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(Refusal::new(Reason::NoConflict)),
            Err(reason) => Err(Refusal::new(reason)),
        }
    }

    /// Every live clause holding the negated pivot.
    ///
    /// The one place that knows how the candidate set is found, and the one
    /// thing swapped out if the bet below ever loses. Option B was an
    /// occurrence index, `Lit -> [ClauseId]`, maintained on every insert and
    /// lazily on every delete; measured against the propagation the checker
    /// already does for its hints, it costs three times this scan on the two
    /// largest real proofs, because `drat-trim` deletes hard enough that the
    /// live database peaks at 1,354 clauses on a 4.1 MB proof while the index
    /// has to be maintained for every one of the quarter of a million clauses
    /// that ever exists.
    ///
    /// It is a bet on that deletion behaviour, so it is countable rather than
    /// argued: `candidates_examined` is reported by `--stats` on any real
    /// proof, and the trigger is written down — if it ever exceeds the hint
    /// literal visits on a real proof, build the index.
    ///
    /// Sorted, so that `MissingResolvent` names the same clause on every run.
    /// The database is a `HashMap`, whose iteration order is not.
    fn resolution_candidates(&mut self, pivot: Lit) -> BTreeSet<ClauseId> {
        let wanted = pivot.negate();
        self.stats.candidate_scans = self.stats.candidate_scans.saturating_add(1);
        self.stats.candidates_examined = self
            .stats
            .candidates_examined
            .saturating_add(u64::try_from(self.db.len()).unwrap_or(u64::MAX));
        let found: BTreeSet<ClauseId> = self
            .db
            .iter()
            .filter(|(_, clause)| clause.contains(&wanted))
            .map(|(id, _)| *id)
            .collect();
        self.stats.resolution_candidates = self
            .stats
            .resolution_candidates
            .saturating_add(u64::try_from(found.len()).unwrap_or(u64::MAX));
        found
    }

    /// The RAT step, exactly as specified in `docs/TDD.md` part 2.
    ///
    /// A clause `C` is RAT on a pivot `p` in `C` with respect to `F` when, for
    /// every clause `D` in `F` holding `-p`, the resolvent `C or (D \ {-p})`
    /// follows from `F` by unit propagation. Adding such a clause preserves
    /// satisfiability, so if `F + C` is unsatisfiable then `F` is.
    ///
    /// Three places a checker can lose that argument, and what happens here:
    ///
    /// 1. **The candidate set must be complete.** Miss one clause holding `-p`
    ///    and the condition was never checked, so an arbitrary clause is added
    ///    on evidence that looks fine. The set is therefore computed from this
    ///    checker's own database; the file's blocks may only ever *satisfy* it.
    /// 2. **The vacuous case must be proved, not assumed.** `205 57 -29 0 0`
    ///    is a valid lemma whose pivot has no candidate, and accepting it
    ///    because the hint list is empty is a checker that accepts anything.
    ///    It goes down this same path with an empty prefix and no blocks, and
    ///    is accepted only after the scan has returned nothing.
    /// 3. **`F` is the live database.** A superset of an unsatisfiable set is
    ///    unsatisfiable, so a step checked against the smaller formula still
    ///    refutes the larger one: a clause deleted earlier needs no block, and
    ///    demanding one would be a false rejection.
    fn check_rat(
        &mut self,
        lits: &[Lit],
        prefix: &[ClauseId],
        blocks: &[ResolventBlock],
    ) -> Result<(), Refusal> {
        let mark = self.trail.len();
        if self.assume_negated(lits) {
            self.unwind(mark);
            return Ok(());
        }
        // The pivot is the first literal *as written in the file*. `normalize`
        // sorts on the way into the database, and sorting changes which
        // literal is first: `46 21 -9 0 0` begins `-9` once sorted, and a
        // checker taking the pivot from there scans for the wrong literal and
        // rejects the smallest real RAT proof there is.
        let pivot = match lits.first() {
            Some(pivot) => *pivot,
            None => {
                self.unwind(mark);
                return Err(Refusal::new(Reason::RatWithoutPivot));
            }
        };

        match self.walk(prefix) {
            Err(reason) => {
                self.unwind(mark);
                return Err(Refusal::new(reason));
            }
            Ok(Some(hint)) => {
                self.unwind(mark);
                return Err(Refusal::new(Reason::RatLemmaIsRup(hint)));
            }
            Ok(None) => {}
        }

        // Every block starts from here: the negated lemma *and* the prefix's
        // propagations. Dropping the prefix reads perfectly well and fails
        // every RAT line in every real proof measured.
        let base = self.trail.len();
        let mut remaining = self.resolution_candidates(pivot);

        for block in blocks {
            self.stats.resolvent_blocks = self.stats.resolvent_blocks.saturating_add(1);
            let clause = match self.db.get(&block.clause) {
                Some(clause) if remaining.contains(&block.clause) => clause.clone(),
                // Not live, not a candidate, or already covered by an earlier
                // block. Ignoring it instead would hide a deleted clause, a
                // wrong pivot and a duplicate in one.
                _ => {
                    self.unwind(mark);
                    return Err(Refusal::at(
                        Reason::NotAResolutionCandidate { pivot },
                        block.clause,
                    ));
                }
            };
            remaining.remove(&block.clause);

            let mut falsified = false;
            for lit in clause.iter() {
                if *lit == pivot.negate() {
                    // Resolved away: it is not part of the resolvent.
                    continue;
                }
                match self.value(*lit) {
                    // Already true, so negating it conflicts at once: the
                    // resolvent is refuted by its own negation alone. All 703
                    // blocks measured for the design are like this.
                    VAR_TRUE => {
                        falsified = true;
                        break;
                    }
                    VAR_FALSE => {}
                    _ => self.assign_true(lit.negate()),
                }
            }

            if falsified {
                if !block.hints.is_empty() {
                    self.unwind(mark);
                    return Err(Refusal::at(Reason::ResolventFalsifiedEarly, block.clause));
                }
            } else {
                match self.walk(&block.hints) {
                    Err(reason) => {
                        self.unwind(mark);
                        return Err(Refusal::at(reason, block.clause));
                    }
                    Ok(None) => {
                        self.unwind(mark);
                        return Err(Refusal::at(Reason::NoConflict, block.clause));
                    }
                    Ok(Some(_)) => {}
                }
            }
            self.unwind(base);
        }

        // The blocks must name the candidate set exactly. Skipping candidates
        // whose resolvent is trivially refuted would be cheaper and would
        // accept the deletion of any real block, since every real block is
        // exactly that — which is the one mutation this milestone exists to
        // catch.
        if let Some(uncovered) = remaining.iter().next() {
            let uncovered = *uncovered;
            self.unwind(mark);
            return Err(Refusal::at(Reason::MissingResolvent { pivot }, uncovered));
        }

        self.unwind(mark);
        Ok(())
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
