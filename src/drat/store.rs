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
        let id = match self.bykey.get_mut(key.as_slice()) {
            // The last copy added, because popping is O(1) and the copies are
            // by definition indistinguishable.
            Some(ids) => match ids.pop() {
                Some(id) => id,
                None => return false,
            },
            None => return false,
        };
        let meta = match self.clauses.get_mut(index_of(id)) {
            Some(meta) => meta,
            None => return false,
        };
        if !meta.live {
            return false;
        }
        meta.live = false;
        self.live = self.live.saturating_sub(1);

        for lit in &key {
            let occ_code = code(*lit);
            if let Some(slot) = self.occ.get_mut(occ_code) {
                if let Some(at) = slot.iter().position(|held| *held == id) {
                    slot.swap_remove(at);
                    self.stats.occurrence_updates = self.stats.occurrence_updates.saturating_add(1);
                }
            }
        }
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
        true
    }

    /// Every live clause holding `-pivot`: the RAT candidate set.
    ///
    /// The one place that knows how the candidate set is found, and the one
    /// thing swapped out if the bet on the index ever loses. Returned owned
    /// because the caller assigns and propagates while it walks the list, and
    /// a borrow of the index across that would be a lie about what the loop
    /// can touch.
    pub(crate) fn resolution_candidates(&self, pivot: Lit) -> Vec<u32> {
        match self.occ.get(code(pivot.negate())) {
            Some(ids) => ids.clone(),
            None => Vec::new(),
        }
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
}
