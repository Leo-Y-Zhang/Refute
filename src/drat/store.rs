//! The clause database, the occurrence index, and unit propagation.
//!
//! Everything the LRAT checker did not need. There, the producer's hints named
//! every clause the checker had to look at, so the database could be a map and
//! the propagation could be a walk over a list. A raw DRAT file names nothing,
//! so this module is the propagation engine that has to find them.
//!
//! Three facts from `docs/TDD.md` part 3 shape it, and each was counted rather
//! than assumed:
//!
//! - **Duplicate live clauses occur.** 39 additions of the A217058 a(4)
//!   certificate duplicate a clause that is already live, with a largest
//!   multiplicity of 3. Deletion names literals, not an identifier, so a store
//!   keyed by literal set cannot hold two copies at all and a store that
//!   removes every identifier under the key loses both. Either way the proof
//!   fails later, for a reason that looks like a corrupt certificate.
//! - **The occurrence index beats the scan here**, which is the *opposite* of
//!   what part 2 measured for the LRAT path: 626,008 index slot updates
//!   against at least 1,309,853 clause visits on the same rung. `drat-trim`'s
//!   LRAT deletes far harder than the file the solver wrote, so the live
//!   database it leaves behind is small enough to scan and this one is not.
//! - **No persistent root-level trail.** The a(4) rung's formula has one unit
//!   clause and its proof adds 87 more over 31,195 steps, so propagating from
//!   scratch re-does at most 88 assignments per step. The saving is small, the
//!   machinery is a reason array plus a retraction path, and the unsound
//!   version of it — keeping the trail without tracking reasons — is a false
//!   `VERIFIED` waiting for a proof that deletes the clause that forced a
//!   unit. It is also why Refute can honour a deletion of a unit clause, which
//!   `drat-trim` must ignore.
//!
//! **Clauses enter and leave only while the trail is empty.** That is what
//! makes the watched-literal invariant trivially true at insertion, and it is
//! checkable rather than asserted: `assignments == assignments_undone` at the
//! end of every run, on every fixture.

use std::collections::HashMap;
use std::mem::size_of;

use crate::checker::Stats;
use crate::cnf::Cnf;
use crate::limits::Limits;
use crate::lit::Lit;

/// Unassigned.
pub(crate) const UNSET: u8 = 0;
/// The variable is assigned true.
const VAR_TRUE: u8 = 1;
/// The variable is assigned false.
const VAR_FALSE: u8 = 2;

/// Where one clause's literals live in the arena.
#[derive(Clone, Copy, Debug)]
struct ClauseMeta {
    start: u32,
    len: u32,
    live: bool,
}

/// The literal's index into the per-literal vectors: `2v` for `v`, `2v + 1`
/// for `-v`. Saturating rather than wrapping, because the input is untrusted;
/// the parser has already refused anything past `Limits::max_var`, so this
/// never saturates in practice.
fn code(lit: Lit) -> usize {
    let var = usize::try_from(lit.var()).unwrap_or(usize::MAX);
    let base = var.saturating_mul(2);
    if lit.is_negated() {
        base.saturating_add(1)
    } else {
        base
    }
}

/// Sorted and duplicate-free, which is both the arena's form and the deletion
/// key's.
///
/// A repeated literal is the same literal: `1 2 -3 -3` is the clause `1 2 -3`.
/// Order inside the store is not observable — the pivot is the *step's* first
/// literal, read from the file and never from here — so one form serves both,
/// and a deletion matches the clause it names whatever order the producer
/// wrote it in.
fn normalize(lits: &[Lit]) -> Vec<Lit> {
    let mut sorted = lits.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
}

/// What the store holds at the end of a run, in bytes.
///
/// The counters that make a memory rule assertable. `docs/TDD.md` part 4
/// measured a change that moved peak working set by a factor of five and left
/// all 128 tests green: a verdict cannot pin this, so a number has to.
pub(crate) struct Footprint {
    /// Every allocation the store owns.
    pub(crate) store_bytes: usize,
    /// Of the arena, the literals of clauses that are still live.
    pub(crate) live_arena_bytes: usize,
    /// Of the arena, the literals of clauses that are not.
    pub(crate) dead_arena_bytes: usize,
    /// Distinct clause bodies the deletion index holds keys for.
    pub(crate) deletion_index_entries: usize,
}

/// A `Vec<Vec<u32>>`: the slot vector itself, plus every slot's own heap.
///
/// The capacity is passed rather than read from the slice because a slice has
/// forgotten it, and the outer vector's spare capacity is real memory: it is
/// 96 bytes per variable whether a literal is ever pushed into it or not, which
/// is the term `Limits::max_drat_var` exists to bound.
fn nested_bytes(capacity: usize, slots: &[Vec<u32>]) -> usize {
    let mut bytes = capacity.saturating_mul(size_of::<Vec<u32>>());
    for slot in slots {
        bytes = bytes.saturating_add(slot.capacity().saturating_mul(size_of::<u32>()));
    }
    bytes
}

/// The clause database and the assignment it propagates under.
pub(crate) struct Store {
    lits: Vec<Lit>,
    clauses: Vec<ClauseMeta>,
    watches: Vec<Vec<u32>>,
    occ: Vec<Vec<u32>>,
    bykey: HashMap<Box<[Lit]>, Vec<u32>>,
    units: Vec<u32>,
    empties: usize,
    live: usize,
    assign: Vec<u8>,
    trail: Vec<Lit>,
    max_var: u32,
    /// Literals of live clauses, and of dead ones the arena still holds.
    ///
    /// Maintained on every add and delete rather than derived, because they
    /// are the compaction trigger and a trigger that walked the metadata
    /// array would be O(clauses ever added) per deletion. [`Store::footprint`]
    /// walks it anyway, once, at the end of a run, and a unit test asserts the
    /// two agree — the maintained pair is the fast answer, not the true one.
    live_lits: usize,
    dead_lits: usize,
    /// The floor from [`Limits::max_dead_arena_lits`], copied in at load.
    ///
    /// Held here because `delete` is where the trigger fires and `delete` has
    /// no other reason to know about limits.
    max_dead_arena_lits: usize,
    /// The counters, kept here because this is where nearly everything worth
    /// counting happens.
    pub(crate) stats: Stats,
}

impl Store {
    /// Loads a formula. Originals take identifiers `1..n` in file order, which
    /// is the LRAT numbering a reader already knows.
    pub(crate) fn new(cnf: &Cnf, limits: &Limits) -> Self {
        let max_var = cnf.num_vars.min(limits.max_var);
        let mut store = Self {
            lits: Vec::new(),
            clauses: Vec::with_capacity(cnf.clauses.len()),
            watches: Vec::new(),
            occ: Vec::new(),
            bykey: HashMap::with_capacity(cnf.clauses.len()),
            units: Vec::new(),
            empties: 0,
            live: 0,
            assign: Vec::new(),
            trail: Vec::new(),
            max_var: 0,
            live_lits: 0,
            dead_lits: 0,
            max_dead_arena_lits: limits.max_dead_arena_lits,
            stats: Stats::default(),
        };
        // Sized from the largest variable the formula actually mentions, never
        // from the `p` line and never from `Limits::max_var`. Part 1 learned
        // this on the assignment vector, where nineteen bytes of header bought
        // a 64 MB allocation; there are three more vectors here, two of them
        // vectors of vectors, so the same mistake would cost 24 bytes per
        // literal code rather than one.
        store.grow_to(max_var);
        for clause in &cnf.clauses {
            store.add(clause);
        }
        store
    }

    /// Grows every per-variable and per-literal vector to hold `var`.
    fn grow_to(&mut self, var: u32) {
        if var > self.max_var {
            self.max_var = var;
        }
        let vars = usize::try_from(var).unwrap_or(usize::MAX).saturating_add(1);
        if vars > self.assign.len() {
            self.assign.resize(vars, UNSET);
        }
        let codes = vars.saturating_mul(2);
        if codes > self.watches.len() {
            self.watches.resize(codes, Vec::new());
            self.occ.resize(codes, Vec::new());
        }
    }

    /// The identifier the next clause added will take.
    ///
    /// Counted from every clause ever added and never from the live count: the
    /// arena keeps a deleted clause's literals where they are, identifiers are
    /// dense and strictly increasing, and a rejection that named the live
    /// count would send a reader to a step that is not the one that failed.
    pub(crate) fn next_id(&self) -> u64 {
        u64::try_from(self.clauses.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1)
    }

    /// Adds a clause and returns its identifier.
    ///
    /// Identifiers are dense and strictly increasing: originals are `1..n`,
    /// lemmas continue from `n + 1` in proof order.
    pub(crate) fn add(&mut self, lits: &[Lit]) -> u32 {
        let stored = normalize(lits);
        for lit in &stored {
            self.grow_to(lit.var());
        }
        let start = u32::try_from(self.lits.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(stored.len()).unwrap_or(u32::MAX);
        self.lits.extend_from_slice(&stored);
        self.clauses.push(ClauseMeta {
            start,
            len,
            live: true,
        });
        let id = u32::try_from(self.clauses.len()).unwrap_or(u32::MAX);
        self.live = self.live.saturating_add(1);
        self.live_lits = self.live_lits.saturating_add(stored.len());
        self.stats.peak_live_clauses = self.stats.peak_live_clauses.max(self.live);

        for lit in &stored {
            if let Some(slot) = self.occ.get_mut(code(*lit)) {
                slot.push(id);
                self.stats.occurrence_updates = self.stats.occurrence_updates.saturating_add(1);
            }
        }
        // A list and not a single value, because duplicates are real.
        self.bykey
            .entry(stored.clone().into_boxed_slice())
            .or_default()
            .push(id);

        match stored.len() {
            0 => self.empties = self.empties.saturating_add(1),
            // A one-literal clause cannot hold two watches, so it is enqueued
            // at the start of every propagation instead.
            1 => self.units.push(id),
            _ => self.watch(id, &stored),
        }
        id
    }

    /// Removes exactly one live copy of the clause `lits` names.
    ///
    /// Returns `false` when no live clause matches, which is counted and not
    /// rejected: deletion only ever removes tools from the checker, so a
    /// spurious one can cause a later rejection and never a false `VERIFIED`.
    pub(crate) fn delete(&mut self, lits: &[Lit]) -> bool {
        let key = normalize(lits);
        let (id, emptied) = match self.bykey.get_mut(key.as_slice()) {
            // The last copy added, because popping is O(1) and the copies are
            // by definition indistinguishable.
            Some(ids) => match ids.pop() {
                Some(id) => (id, ids.is_empty()),
                None => return false,
            },
            None => return false,
        };
        // The prune, and the whole of it. Without this line the map keeps a
        // boxed copy of the literals of every distinct clause the proof ever
        // contained — a second arena, and the largest single item in the
        // store: 96.5 MB of 179.8 MB on the largest proof measured for
        // `docs/TDD.md` part 4, of which 1.6 MB belonged to a live clause.
        //
        // Only when the last copy has gone. Duplicate live clauses are real —
        // 39 additions of the A217058 a(4) certificate duplicate a clause that
        // is already live, with a largest multiplicity of three — and dropping
        // the key while a copy remains loses the survivor's only route back,
        // so its later deletion finds nothing and the proof fails for a reason
        // that looks like a corrupt certificate.
        if emptied {
            self.bykey.remove(key.as_slice());
        }
        let meta = match self.clauses.get_mut(index_of(id)) {
            Some(meta) => meta,
            None => return false,
        };
        if !meta.live {
            return false;
        }
        meta.live = false;
        self.live = self.live.saturating_sub(1);
        self.live_lits = self.live_lits.saturating_sub(key.len());
        self.dead_lits = self.dead_lits.saturating_add(key.len());

        // The occurrence index is deliberately not touched here. It used to be
        // cleared literal by literal, and each clearing was a linear search
        // over a list holding every live clause containing that literal:
        // 200,595,972 entries compared on the A217058 a(4) rung and
        // 31,076,047,076 on the a(7) rung, to answer 234 and 384 candidate
        // queries. An order of magnitude more work than propagation, which is
        // the thing the checker is supposed to be doing.
        //
        // A stale entry is safe in the only direction that matters and it is
        // worth being explicit about why. Completeness is what soundness rests
        // on: every clause containing a literal was pushed onto that literal's
        // list when it was added, and nothing removes an identifier except a
        // compaction, which drops only clauses that are dead. So a list is
        // always a superset of the candidates. `resolution_candidates` then
        // re-derives membership from the store rather than trusting the list,
        // which turns a stale entry into a dropped entry and never into a
        // missed candidate.
        match key.len() {
            0 => self.empties = self.empties.saturating_sub(1),
            1 => {
                if let Some(at) = self.units.iter().position(|held| *held == id) {
                    self.units.swap_remove(at);
                }
            }
            // The two watched literals are the first two in the arena, however
            // often propagation has swapped them since. Deletion happens with
            // the trail empty, so nothing is mid-swap.
            _ => {
                for offset in 0..2 {
                    if let Some(lit) = self.literal_at(id, offset) {
                        self.unwatch(id, lit);
                    }
                }
            }
        }
        // The only call site, and the reason the trail is empty here: deletion
        // happens between steps, and every step unwinds itself completely.
        // `assignments == assignments_undone` already asserts that on every
        // positive fixture, so the identity stops being a nicety and becomes
        // the precondition this depends on.
        //
        // Both halves of the trigger matter. The ratio alone would copy a
        // four-literal arena the first time a proof deleted its second clause;
        // the floor alone would never fire on a proof that keeps everything it
        // adds and deletes a little.
        if self.dead_lits > self.live_lits && self.dead_lits > self.max_dead_arena_lits {
            self.compact();
        }
        true
    }

    /// Copies the live clauses' literals into a fresh arena, in identifier
    /// order, and rewrites every live clause's start.
    ///
    /// **Identifiers do not move.** Only `ClauseMeta::start` is rewritten, so
    /// every rejection names the clause it named before, `next_id` still
    /// counts every clause ever added, and the LRAT numbering a reader knows
    /// is unchanged. That is what makes this invisible from outside and the
    /// reason 128 tests were required to stay green across it rather than be
    /// re-baselined.
    ///
    /// Three things are load-bearing beyond that:
    ///
    /// - **The order within a clause is preserved.** The first two literals in
    ///   the arena are the watched ones, however often `visit` has swapped
    ///   them. A compaction that sorted, deduplicated or reordered would break
    ///   the watch invariant silently, on a database that then propagates
    ///   wrongly for the rest of the run. It copies the slice.
    /// - **Nothing outside `ClauseMeta::start` refers to the arena.**
    ///   `watches`, `occ`, `units` and `bykey` all hold identifiers, which is
    ///   what makes the remap local.
    /// - **A dead clause's `len` is zeroed.** Not required — a dead clause is
    ///   already unreachable through every index — but it turns any future
    ///   stale reference from "reads someone else's literals" into "reads
    ///   nothing", which is the fail-closed direction.
    fn compact(&mut self) {
        let mut fresh = Vec::with_capacity(self.live_lits);
        for meta in &mut self.clauses {
            if !meta.live {
                meta.start = 0;
                meta.len = 0;
                continue;
            }
            let start = usize::try_from(meta.start).unwrap_or(usize::MAX);
            let end = start.saturating_add(usize::try_from(meta.len).unwrap_or(0));
            let landed = u32::try_from(fresh.len()).unwrap_or(u32::MAX);
            fresh.extend_from_slice(self.lits.get(start..end).unwrap_or(&[]));
            meta.start = landed;
        }
        self.lits = fresh;
        self.dead_lits = 0;

        // The occurrence index is purged here and nowhere else. Taken out and
        // put back so that the predicate can read the metadata array while the
        // lists are being rewritten; the vector itself moves, not its heap.
        let mut occ = std::mem::take(&mut self.occ);
        for slot in &mut occ {
            slot.retain(|id| self.is_live(*id));
            // `retain` does not give capacity back, and with deletion no
            // longer touching the index that capacity is every clause ever
            // added holding the literal rather than every live one. Measured
            // on the a(7) rung, and this line is the difference between the
            // lazy index being worth having and not:
            //
            //   without   34.1 MB peak working set, 25.2 MB accounted
            //   with      31.2 MB peak working set, 18.7 MB accounted
            //   eager     31.7 MB peak working set, 22.5 MB accounted
            //
            // Reallocating every non-empty slot at every compaction is 44
            // compactions times some hundreds of slots on that proof, and it
            // makes the run faster rather than slower, which is measurement
            // and not a reason.
            slot.shrink_to_fit();
        }
        self.occ = occ;
        self.stats.compactions = self.stats.compactions.saturating_add(1);
    }

    /// Whether the clause an identifier names is still in the database.
    fn is_live(&self, id: u32) -> bool {
        matches!(self.clauses.get(index_of(id)), Some(meta) if meta.live)
    }

    /// Every live clause holding `-pivot`: the RAT candidate set.
    ///
    /// The one place that knows how the candidate set is found, and the one
    /// thing swapped out if the bet on the index ever loses. Returned owned
    /// because the caller assigns and propagates while it walks the list, and
    /// a borrow of the index across that would be a lie about what the loop
    /// can touch.
    ///
    /// Takes `&mut self` because it counts, and because it writes the filtered
    /// list back. The entries it walks are the whole price of the index now
    /// that deletion pays nothing, and the trigger for abandoning it is
    /// written against that number: **if a real proof reports more
    /// `occurrence_entries_filtered` than `rat_additions` times
    /// `peak_live_clauses`, the index is losing to a plain scan of the live
    /// clauses and this function should become one.** One function, exactly as
    /// part 3 wrote the trigger it lost.
    ///
    /// **The filter is a predicate over the store, not a trust in the list.**
    /// A candidate is returned because its clause is live *and* because it
    /// really does contain the negated pivot, both re-derived here. That is
    /// what makes a stale entry harmless: an entry that should not be there is
    /// dropped, and an entry that should be there was never removed, because
    /// nothing removes one except a compaction and a compaction drops only
    /// clauses that are dead.
    ///
    /// The containment check is belt rather than brace — every identifier in
    /// `occ[l]` was pushed because its clause held `l`, and a clause's
    /// literals never change. It is here because it costs a walk of a clause
    /// that is about to be walked anyway, and because the alternative is a
    /// soundness argument that rests on two invariants instead of one.
    pub(crate) fn resolution_candidates(&mut self, pivot: Lit) -> Vec<u32> {
        let want = pivot.negate();
        let slot = code(want);
        let held = match self.occ.get(slot) {
            Some(ids) => ids.clone(),
            None => return Vec::new(),
        };
        self.stats.occurrence_entries_filtered = self
            .stats
            .occurrence_entries_filtered
            .saturating_add(u64::try_from(held.len()).unwrap_or(u64::MAX));

        let mut kept = Vec::with_capacity(held.len());
        for id in held {
            if self.is_live(id) && self.slice(id).contains(&want) {
                kept.push(id);
            }
        }
        // Written back, so the same query does not pay for the same dead
        // entries twice. With compaction purging the lists as well, the a(7)
        // rung's queries walk 384 entries in total — one per candidate.
        if let Some(ids) = self.occ.get_mut(slot) {
            ids.clone_from(&kept);
        }
        kept
    }

    /// The literals of a clause, in the arena's order.
    pub(crate) fn clause(&self, id: u32) -> Vec<Lit> {
        self.slice(id).to_vec()
    }

    /// The value of a literal under the current assignment.
    pub(crate) fn value(&self, lit: Lit) -> u8 {
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

    /// True when the literal is false under the current assignment.
    fn is_false(&self, lit: Lit) -> bool {
        self.value(lit) == VAR_FALSE
    }

    /// How many literals are on the trail. The caller's unwind mark.
    pub(crate) fn trail_len(&self) -> usize {
        self.trail.len()
    }

    /// Assigns a literal true and pushes it onto the trail.
    pub(crate) fn assign_true(&mut self, lit: Lit) {
        self.grow_to(lit.var());
        let index = usize::try_from(lit.var()).unwrap_or(usize::MAX);
        if let Some(slot) = self.assign.get_mut(index) {
            *slot = if lit.is_negated() {
                VAR_FALSE
            } else {
                VAR_TRUE
            };
            self.trail.push(lit);
            self.stats.assignments = self.stats.assignments.saturating_add(1);
        }
    }

    /// Unwinds the trail to `mark`, in O(assigned) and never by clearing the
    /// whole assignment vector.
    pub(crate) fn unwind(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop() {
                Some(lit) => {
                    let index = usize::try_from(lit.var()).unwrap_or(usize::MAX);
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

    /// Unit propagation from scratch: the unit clauses, then the trail.
    ///
    /// Called once per step, before any candidate is looked at. The empty
    /// clause in the *formula* is a conflict on its own, which is how a
    /// formula holding a bare `0` is refuted in one step.
    pub(crate) fn propagate_from_scratch(&mut self) -> bool {
        if self.empties > 0 {
            return true;
        }
        for index in 0..self.units.len() {
            let id = match self.units.get(index) {
                Some(id) => *id,
                None => break,
            };
            let lit = match self.literal_at(id, 0) {
                Some(lit) => lit,
                None => continue,
            };
            match self.value(lit) {
                VAR_TRUE => continue,
                VAR_FALSE => return true,
                _ => {
                    self.assign_true(lit);
                    self.stats.propagations = self.stats.propagations.saturating_add(1);
                }
            }
        }
        self.propagate(0)
    }

    /// Watched-literal propagation over the trail from `from`.
    ///
    /// Returns `true` on conflict. Everything before `from` has already been
    /// propagated to completion by an earlier call in the same step, which is
    /// why a resolvent check can start where the RUP check finished.
    pub(crate) fn propagate(&mut self, from: usize) -> bool {
        let mut head = from;
        while head < self.trail.len() {
            let assigned = match self.trail.get(head) {
                Some(lit) => *lit,
                None => break,
            };
            head = head.saturating_add(1);
            let falsified = assigned.negate();
            let watch_code = code(falsified);

            let mut at = 0usize;
            // Indexed rather than iterated, because the list is mutated as it
            // is walked: a watch that moves is `swap_remove`d from here and
            // pushed onto another literal's list, and the entry swapped into
            // its place still has to be visited.
            while let Some(id) = self
                .watches
                .get(watch_code)
                .and_then(|w| w.get(at))
                .copied()
            {
                self.stats.watch_visits = self.stats.watch_visits.saturating_add(1);
                match self.visit(id, falsified) {
                    Visit::Keep => at = at.saturating_add(1),
                    Visit::Moved(to) => {
                        if let Some(list) = self.watches.get_mut(watch_code) {
                            list.swap_remove(at);
                        }
                        if let Some(list) = self.watches.get_mut(to) {
                            list.push(id);
                        }
                    }
                    Visit::Unit(lit) => {
                        self.assign_true(lit);
                        self.stats.propagations = self.stats.propagations.saturating_add(1);
                        at = at.saturating_add(1);
                    }
                    Visit::Conflict => return true,
                }
            }
        }
        false
    }

    /// One clause of a watch list, under a literal that has just become false.
    fn visit(&mut self, id: u32, falsified: Lit) -> Visit {
        let (start, len) = match self.clauses.get(index_of(id)) {
            Some(meta) if meta.live => (
                usize::try_from(meta.start).unwrap_or(usize::MAX),
                usize::try_from(meta.len).unwrap_or(0),
            ),
            // Deletion removes the clause from both its watch lists, so a dead
            // clause cannot be here. Keeping the entry is the safe answer if
            // one ever is: a clause that is never propagated can only cause a
            // rejection.
            _ => return Visit::Keep,
        };
        if len < 2 {
            return Visit::Keep;
        }
        // Normalise so that the falsified literal is the second watch.
        if self.lits.get(start) == Some(&falsified) {
            self.lits.swap(start, start.saturating_add(1));
        }
        let other = match self.lits.get(start) {
            Some(lit) => *lit,
            None => return Visit::Keep,
        };
        if self.value(other) == VAR_TRUE {
            return Visit::Keep;
        }
        let end = start.saturating_add(len);
        let mut replacement = None;
        for at in start.saturating_add(2)..end {
            match self.lits.get(at) {
                Some(lit) if !self.is_false(*lit) => {
                    replacement = Some(at);
                    break;
                }
                _ => {}
            }
        }
        match replacement {
            Some(at) => {
                let second = start.saturating_add(1);
                self.lits.swap(second, at);
                match self.lits.get(second) {
                    Some(lit) => Visit::Moved(code(*lit)),
                    None => Visit::Keep,
                }
            }
            None => match self.value(other) {
                UNSET => Visit::Unit(other),
                _ => Visit::Conflict,
            },
        }
    }

    fn watch(&mut self, id: u32, stored: &[Lit]) {
        for offset in 0..2 {
            if let Some(lit) = stored.get(offset) {
                if let Some(list) = self.watches.get_mut(code(*lit)) {
                    list.push(id);
                }
            }
        }
    }

    fn unwatch(&mut self, id: u32, lit: Lit) {
        if let Some(list) = self.watches.get_mut(code(lit)) {
            if let Some(at) = list.iter().position(|held| *held == id) {
                list.swap_remove(at);
            }
        }
    }

    fn literal_at(&self, id: u32, offset: usize) -> Option<Lit> {
        let meta = self.clauses.get(index_of(id))?;
        if usize::try_from(meta.len).unwrap_or(0) <= offset {
            return None;
        }
        let start = usize::try_from(meta.start).unwrap_or(usize::MAX);
        self.lits.get(start.saturating_add(offset)).copied()
    }

    fn slice(&self, id: u32) -> &[Lit] {
        match self.clauses.get(index_of(id)) {
            Some(meta) => {
                let start = usize::try_from(meta.start).unwrap_or(usize::MAX);
                let end = start.saturating_add(usize::try_from(meta.len).unwrap_or(0));
                self.lits.get(start..end).unwrap_or(&[])
            }
            None => &[],
        }
    }

    /// Slots in the assignment vector, for `--stats`.
    pub(crate) fn assignment_slots(&self) -> usize {
        self.assign.len()
    }

    /// What the store holds, walked at the end of a run.
    ///
    /// Summed from **capacities**, not from lengths, so it reports what the
    /// process asked the allocator for rather than what it is using. That is
    /// the figure a peak working set can be compared against: the
    /// instrumented build `docs/TDD.md` part 4 was measured with accounted for
    /// 179.8 MB of a 182.6 MB peak on the largest proof in the ladder.
    ///
    /// One term is a model and is named as such. `HashMap` does not report the
    /// bytes of its own table, so the deletion index's table is priced at one
    /// key, one value and one control byte per element it says it can hold
    /// without reallocating. Everything behind it — the boxed keys, the
    /// identifier lists — is a real allocation and is summed exactly. The
    /// arena, the metadata array, the watch and occurrence lists, the unit
    /// list, the assignment and the trail are every one of them a capacity the
    /// container reports itself.
    ///
    /// O(clauses ever added) plus O(literal codes), once per run. It walks the
    /// metadata array rather than maintaining a running pair, deliberately:
    /// this commit adds counters and changes no behaviour, and the running
    /// pair arrives with the compaction that needs it as a trigger.
    pub(crate) fn footprint(&self) -> Footprint {
        let lit_bytes = size_of::<Lit>();
        let mut live_lits = 0usize;
        let mut dead_lits = 0usize;
        for meta in &self.clauses {
            let len = usize::try_from(meta.len).unwrap_or(0);
            if meta.live {
                live_lits = live_lits.saturating_add(len);
            } else {
                dead_lits = dead_lits.saturating_add(len);
            }
        }

        let store_bytes = self
            .lits
            .capacity()
            .saturating_mul(lit_bytes)
            .saturating_add(
                self.clauses
                    .capacity()
                    .saturating_mul(size_of::<ClauseMeta>()),
            )
            .saturating_add(nested_bytes(self.watches.capacity(), &self.watches))
            .saturating_add(nested_bytes(self.occ.capacity(), &self.occ))
            .saturating_add(self.deletion_index_bytes())
            .saturating_add(self.units.capacity().saturating_mul(size_of::<u32>()))
            .saturating_add(self.assign.capacity())
            .saturating_add(self.trail.capacity().saturating_mul(lit_bytes));

        Footprint {
            store_bytes,
            live_arena_bytes: live_lits.saturating_mul(lit_bytes),
            dead_arena_bytes: dead_lits.saturating_mul(lit_bytes),
            deletion_index_entries: self.bykey.len(),
        }
    }

    /// The deletion index: its table, its keys, and the identifier lists.
    ///
    /// The largest single item in the store on the proof part 4 measured —
    /// 96.5 MB of 179.8 MB, of which 1.6 MB belonged to a clause still live —
    /// because `delete` pops an identifier out of the list and leaves the key
    /// behind, so the map keeps a copy of the literals of every distinct
    /// clause the proof ever contained.
    fn deletion_index_bytes(&self) -> usize {
        let per_slot = size_of::<Box<[Lit]>>()
            .saturating_add(size_of::<Vec<u32>>())
            .saturating_add(1);
        let mut bytes = self.bykey.capacity().saturating_mul(per_slot);
        for (key, ids) in &self.bykey {
            bytes = bytes
                .saturating_add(key.len().saturating_mul(size_of::<Lit>()))
                .saturating_add(ids.capacity().saturating_mul(size_of::<u32>()));
        }
        bytes
    }
}

/// What to do with one clause in a watch list.
enum Visit {
    /// The clause is satisfied or still has two live watches here.
    Keep,
    /// The watch moved to the literal code given, so this list is one shorter.
    Moved(usize),
    /// Every other literal is false: this one is forced.
    Unit(Lit),
    /// Every literal is false.
    Conflict,
}

/// Identifiers are one-based, so that they match the LRAT numbering a reader
/// already knows; the arrays behind them are not.
fn index_of(id: u32) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    // A test asserts by panicking. The package's panic floor in Cargo.toml is
    // there for the library and the binary, where a panic on input-derived
    // data is a denial of service; here it would only make the failure report
    // worse. The integration tests lift the same half for the same reason.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{Store, UNSET};
    use crate::cnf::parse_dimacs;
    use crate::limits::Limits;
    use crate::lit::Lit;

    fn lit(raw: i32) -> Lit {
        Lit::new(raw).expect("a non-zero literal")
    }

    fn store(dimacs: &str) -> Store {
        let cnf = parse_dimacs(dimacs.as_bytes(), &Limits::default()).expect("a formula");
        Store::new(&cnf, &Limits::default())
    }

    /// The same, with the compaction floor at zero.
    ///
    /// The trigger is still a comparison against the live half, so this does
    /// not compact on every deletion: it compacts as soon as the dead half is
    /// larger, which on a small formula is a handful of deletions rather than
    /// the hundreds the default floor of 1,024 literals would need.
    fn forced(dimacs: &str) -> Store {
        let limits = Limits {
            max_dead_arena_lits: 0,
            ..Limits::default()
        };
        let cnf = parse_dimacs(dimacs.as_bytes(), &limits).expect("a formula");
        Store::new(&cnf, &limits)
    }

    #[test]
    fn originals_take_one_based_identifiers_in_file_order() {
        let store = store("p cnf 2 2\n1 2 0\n-1 -2 0\n");
        assert_eq!(store.clause(1), vec![lit(1), lit(2)]);
        assert_eq!(store.clause(2), vec![lit(-2), lit(-1)]);
        assert_eq!(store.next_id(), 3, "the next lemma continues from n + 1");
    }

    #[test]
    fn a_deletion_removes_exactly_one_of_two_copies() {
        let mut store = store("p cnf 2 2\n-1 -2 0\n-1 -2 0\n");
        assert_eq!(store.resolution_candidates(lit(1)).len(), 2);
        assert!(store.delete(&[lit(-1), lit(-2)]));
        assert_eq!(
            store.resolution_candidates(lit(1)).len(),
            1,
            "both copies went"
        );
        assert!(store.delete(&[lit(-2), lit(-1)]), "order must not matter");
        assert!(store.resolution_candidates(lit(1)).is_empty());
        assert!(!store.delete(&[lit(-1), lit(-2)]), "a third deletion");
    }

    #[test]
    fn candidates_are_the_live_clauses_holding_the_negated_pivot() {
        let mut store = store("p cnf 3 3\n-1 2 0\n-1 -2 0\n1 3 0\n");
        assert_eq!(store.resolution_candidates(lit(1)), vec![1, 2]);
        assert!(store.delete(&[lit(-1), lit(2)]));
        assert_eq!(store.resolution_candidates(lit(1)), vec![2]);
    }

    #[test]
    fn the_vectors_grow_past_the_formulas_largest_variable() {
        let mut store = store("p cnf 1 1\n1 0\n");
        let before = store.assignment_slots();
        store.add(&[lit(9)]);
        assert!(
            store.assignment_slots() > before,
            "the assignment vector did not grow"
        );
        assert_eq!(store.value(lit(9)), UNSET);
    }

    #[test]
    fn propagation_derives_a_conflict_from_units_alone() {
        let mut store = store("p cnf 2 3\n1 0\n-1 2 0\n-2 0\n");
        assert!(store.propagate_from_scratch(), "the formula is refutable");
        store.unwind(0);
        assert_eq!(store.trail_len(), 0);
    }

    #[test]
    fn deleting_a_unit_clause_takes_its_propagation_with_it() {
        let mut store = store("p cnf 2 3\n1 0\n-1 2 0\n-2 0\n");
        assert!(store.delete(&[lit(1)]));
        assert!(
            !store.propagate_from_scratch(),
            "the deletion was not honoured"
        );
    }

    /// S01. A compaction gives every live clause back exactly what it had.
    ///
    /// The serious failure mode of this milestone, and the only one nobody
    /// would notice for months: a remap that is wrong by one gives a step some
    /// other clause's literals, and there is no file to disagree with it.
    ///
    /// Propagation runs first, and that is the point of the test rather than
    /// scene-setting. `visit` swaps literals inside the arena to keep the
    /// watched pair in the first two slots, so by the time the snapshot is
    /// taken the clauses are no longer in the order they were written. A
    /// compaction that sorted, deduplicated or normalised would pass a test
    /// that compared against the input and fail this one.
    #[test]
    fn s01_compaction_gives_every_live_clause_back_exactly_what_it_had() {
        let mut store = forced("p cnf 5 5\n1 2 3 0\n-1 2 0\n-2 3 4 0\n-3 4 5 0\n1 -4 5 0\n");
        store.assign_true(lit(-1));
        store.propagate(0);
        store.unwind(0);

        let survivors = [1u32, 5];
        let before: Vec<Vec<Lit>> = survivors.iter().map(|id| store.clause(*id)).collect();
        assert_eq!(store.stats.compactions, 0, "nothing has been deleted yet");

        assert!(store.delete(&[lit(-1), lit(2)]));
        assert!(store.delete(&[lit(-2), lit(3), lit(4)]));
        assert!(store.delete(&[lit(-3), lit(4), lit(5)]));
        assert_eq!(
            store.stats.compactions, 1,
            "eight dead literals against six live did not trigger a compaction"
        );
        assert_eq!(
            store.lits.len(),
            6,
            "the arena still holds the dead clauses' literals"
        );

        for (id, was) in survivors.iter().zip(before) {
            assert_eq!(store.clause(*id), was, "clause {id} changed under it");
        }
        assert_eq!(store.next_id(), 6, "an identifier moved");
    }

    /// S02. The deletion index drops a key only when its last copy has gone.
    ///
    /// Three copies, deleted one at a time. A prune that fired on the first
    /// deletion would leave the survivors live and unreachable: their later
    /// deletions find nothing, the candidate set keeps clauses the proof
    /// deleted, and a real certificate fails for a reason that looks like
    /// corruption. The A217058 a(4) rung would fail outright, on 39 additions
    /// that duplicate a clause already live.
    #[test]
    fn s02_a_key_survives_until_its_last_copy_goes() {
        let mut store = store("p cnf 2 3\n-1 -2 0\n-1 -2 0\n-1 -2 0\n");
        assert_eq!(store.resolution_candidates(lit(1)).len(), 3);
        assert!(store.delete(&[lit(-1), lit(-2)]));
        assert_eq!(
            store.resolution_candidates(lit(1)).len(),
            2,
            "one copy went, and so did the key"
        );
        assert!(store.delete(&[lit(-2), lit(-1)]), "order must not matter");
        assert!(store.delete(&[lit(-1), lit(-2)]), "the last copy");
        assert!(store.resolution_candidates(lit(1)).is_empty());
        assert!(
            !store.delete(&[lit(-1), lit(-2)]),
            "a fourth deletion found something"
        );
    }

    /// B42. The same, at a scale where no two survivors move by the same
    /// amount, and across more than one compaction.
    ///
    /// S01 has two survivors and one compaction, so a remap that rewrote only
    /// the first live clause's start would have a one-in-two chance of passing
    /// it. Here 720 clauses are added and 504 deleted, fourteen deleted to six
    /// kept, in three rounds — because the second compaction runs over an
    /// arena the first one already rewrote, and that is a different thing to
    /// get wrong.
    ///
    /// The proportion is `rat_pigeonhole`'s, which is 702 additions and 487
    /// deletions. The rounds are not, and they are here because of a
    /// measurement: a compaction resets the dead half to zero and the live
    /// half only shrinks, so one long deletion run triggers **once** however
    /// long it is. The first version of this test asked for more than five
    /// compactions across 490 deletions and got one. Three rounds give two.
    #[test]
    fn b42_every_live_clause_survives_a_compaction_at_scale() {
        let mut store = forced("p cnf 3 1\n1 2 3 0\n");
        let mut survivors: Vec<(u32, Vec<Lit>)> = Vec::new();
        let mut next_var = 4i32;
        let mut deleted = 0usize;

        for _round in 0..3 {
            let mut added: Vec<(u32, Vec<Lit>)> = Vec::new();
            for _ in 0..240 {
                let body = vec![
                    lit(next_var),
                    lit(next_var.saturating_add(1)),
                    lit(next_var.saturating_add(2)),
                ];
                next_var = next_var.saturating_add(3);
                let id = store.add(&body);
                added.push((id, body));
            }
            for (step, (id, body)) in added.into_iter().enumerate() {
                if step % 20 < 14 {
                    assert!(store.delete(&body), "a deletion found nothing");
                    deleted = deleted.saturating_add(1);
                } else {
                    survivors.push((id, body));
                }
            }
        }

        assert_eq!(deleted, 504, "the deletion pattern moved");
        assert_eq!(survivors.len(), 216, "the survivor count moved");
        // Two, measured. The bound is what the test needs — a compaction that
        // runs over an arena an earlier one rewrote — and not the number,
        // which moves with the round size and is nobody's contract.
        assert!(
            store.stats.compactions >= 2,
            "{} compactions across three rounds of {deleted} deletions",
            store.stats.compactions
        );

        for (id, body) in &survivors {
            assert_eq!(
                store.clause(*id),
                *body,
                "clause {id} came back with someone else's literals"
            );
        }
        assert_eq!(store.clause(1), vec![lit(1), lit(2), lit(3)], "the formula");
        assert_eq!(store.next_id(), 722, "an identifier moved");
    }

    /// `store_bytes` counts the arena, so compacting the arena moves it.
    ///
    /// Written because the mutation-kill pass found the rule unpinned: taking
    /// the arena term out of `footprint` entirely left all 149 tests green,
    /// including three assertions written to catch exactly that.
    ///
    /// They miss it because the arena cannot be told apart from the indexes
    /// over it by size. The occurrence index holds one 4-byte identifier per
    /// literal of every clause added, and the arena holds one 4-byte literal,
    /// so any `>=` against a single reported figure still passes with either
    /// one of them dropped. Neither `swap_remove` nor `retain` gives capacity
    /// back, so deleting everything does not separate them either — measured,
    /// on the first version of this test, which asserted that and passed under
    /// the mutation.
    ///
    /// What does separate them is compaction. It replaces the arena with a
    /// vector sized to the live literals and leaves every occurrence list's
    /// capacity exactly where it was, so the same sequence of operations under
    /// two floors differs in the arena and in nothing else. A `store_bytes`
    /// that has stopped counting the arena reports the same figure for both.
    #[test]
    fn compaction_is_visible_in_the_size_the_store_reports() {
        // The same 200 additions and 190 deletions, run twice. Duplicates on
        // purpose: 200 copies of one body share a single key, so the deletion
        // index is a rounding error here and the arena is not.
        let held = |floor: usize| -> (usize, u64) {
            let limits = Limits {
                max_dead_arena_lits: floor,
                ..Limits::default()
            };
            let cnf = parse_dimacs("p cnf 60 0\n".as_bytes(), &limits).expect("a formula");
            let mut store = Store::new(&cnf, &limits);
            let body: Vec<Lit> = (10..60).map(lit).collect();
            assert_eq!(body.len(), 50);
            for _ in 0..200 {
                store.add(&body);
            }
            for copy in 0..190 {
                assert!(store.delete(&body), "deletion {copy} found nothing");
            }
            (store.footprint().store_bytes, store.stats.compactions)
        };

        let (uncompacted, never) = held(usize::MAX);
        let (compacted, ran) = held(0);
        assert_eq!(never, 0, "a floor of usize::MAX compacted");
        assert!(ran > 0, "a floor of zero did not compact");
        assert!(
            compacted < uncompacted,
            "{compacted} bytes after {ran} compactions against {uncompacted} without any"
        );
    }

    /// The compaction trigger's inputs are what a walk of the metadata says.
    ///
    /// `live_lits` and `dead_lits` are maintained on every add and delete
    /// because the trigger cannot afford to walk the metadata array on each
    /// deletion. That makes them a second implementation of a number
    /// [`Store::footprint`] derives independently, and two implementations of
    /// one number is exactly where they drift.
    #[test]
    fn the_maintained_literal_counts_match_a_walk_of_the_metadata() {
        let mut store = store("p cnf 4 4\n1 2 3 0\n-1 2 0\n-2 3 4 0\n-3 4 0\n");
        store.add(&[lit(1), lit(-4)]);
        assert!(store.delete(&[lit(-1), lit(2)]));
        assert!(store.delete(&[lit(-3), lit(4)]));

        let footprint = store.footprint();
        let lit_bytes = size_of::<Lit>();
        assert_eq!(
            footprint.live_arena_bytes,
            store.live_lits.saturating_mul(lit_bytes),
            "live literals"
        );
        assert_eq!(
            footprint.dead_arena_bytes,
            store.dead_lits.saturating_mul(lit_bytes),
            "dead literals"
        );
        assert_eq!(store.stats.compactions, 0, "the default floor is 1024");
    }
}
