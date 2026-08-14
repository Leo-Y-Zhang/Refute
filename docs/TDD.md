# TDD — Refute: forward LRAT checker

**Status:** part 1 built · part 2 built · part 3 draft
**Date:** 2026-08-14 · **PRD:** [PRD.md](PRD.md) · **Repo:** Refute

Part 1 is milestone 1 — RUP steps with hints — and is built, reviewed and green.
Nothing in it is amended below; [part 2](#part-2--milestone-1b-rat-hint-blocks)
adds RAT hint blocks and states, per rule, where it changes a part 1 decision.
[Part 3](#part-3--milestone-2-direct-drat) is milestone 2 — direct DRAT — and
amends nothing in either: it adds a second reader and a second checker beside
them, and every test of the first two must stay green at every commit in its
build order.

---

# Part 1 — milestone 1: RUP with hints

## Approach

One pass, forward, streaming. Parse the CNF into a clause database keyed by
position (`1..n`). Then read the LRAT file step by step without holding it in
memory. A deletion step removes ids from the database. An addition step is checked
by assigning the negation of every literal in the lemma, walking its hint list in
order, propagating each hint clause as a unit, and requiring the final hint to be
falsified. Then the lemma joins the database under its own id and the trail is
unwound. The run ends the instant a step fails, or when a checked step adds the
empty clause. Nothing else returns `Verified`.

No watched literals, no occurrence lists, no propagation engine. That is the whole
reason to do LRAT before DRAT: with hints, step checking is a bounded walk over a
list, and its soundness argument fits in a paragraph.

## Trust boundary

Refute's only security property is: **`s VERIFIED` is printed only when a checked
sequence of steps derives the empty clause from the parsed formula.** Everything
below is in service of that.

The property is enforced structurally, not by discipline:

- `Verdict::Verified` is a unit variant with no public constructor path other than
  `Checker::finish_with_empty_clause()`, a private method called from exactly one
  site — immediately after the step that added the empty clause returned `Ok`.
- `Verdict` has no `Default`, no `From<bool>`, and is `#[must_use]`.
- The CLI maps verdicts to output in one `match`, with no `_ =>` arm, so a new
  variant cannot silently fall into the success branch.
- A unit test asserts that the source contains exactly one occurrence of the token
  `Verdict::Verified` outside the enum definition and the tests. Crude, and it has
  caught this class of mistake in other codebases.

## Data model

No database, no migrations. The persistent artefacts are files. State is in memory
for the length of one run.

| Structure | Type | Notes |
|---|---|---|
| `Lit` | `i32` newtype, non-zero | DIMACS sign convention preserved; `Lit::var() -> u32` |
| `Clause` | `Box<[Lit]>` | Immutable once stored, and duplicate-free: a repeated literal is the same literal, and counting it twice makes a unit clause look non-unit |
| `ClauseId` | `u64` | LRAT ids are strictly increasing but **sparse** (measured: 2,873 lemmas spanning ids 205..3571) |
| `ClauseDb` | `HashMap<ClauseId, Clause>` | Sparse ids rule out a plain `Vec`. An arena + index map is a milestone-3 optimisation and must be justified by a benchmark, not by taste |
| `Assign` | `Vec<u8>` indexed by var, `0` unset / `1` true / `2` false | Sized once from the parsed formula's max variable, grown on demand |
| `Trail` | `Vec<u32>` of assigned vars | Unwound after each step in O(assigned), **never** by clearing the whole `Assign` vector — that is O(vars) per step and quadratic on a 100k-variable formula |

**The null case here is the empty case,** and there are four of them, all of which
occur in real files and all of which must be tested:

- an empty clause **in the CNF** (a bare `0` line) — formula is trivially UNSAT;
- an empty deletion list (`204 d 0`) — measured, occurs once per file;
- an empty hint list on an addition (`205 57 -29 0 0`) — measured, 2 % of lines;
- the empty clause as the final lemma — the thing the whole run is looking for.

## Interfaces

```rust
// cnf.rs
pub struct Cnf { pub num_vars: u32, pub clauses: Vec<Box<[Lit]>> }
pub fn parse_dimacs<R: BufRead>(r: R, limits: &Limits) -> Result<Cnf, ParseError>;

// lrat.rs — streaming; never holds the file
pub enum Hints {
    Rup(Vec<ClauseId>),                 // all positive
    Rat,                                // any negative id present: unsupported in M1
    Empty,                              // "... 0 0": unsupported in M1, NOT a pass
}
pub enum Step {
    Add { id: ClauseId, lits: Vec<Lit>, hints: Hints, line: u64 },
    Delete { ids: Vec<ClauseId>, line: u64 },
}
pub struct LratReader<R: BufRead> { /* ... */ }
impl<R: BufRead> Iterator for LratReader<R> { type Item = Result<Step, ParseError>; }

// verdict.rs
#[must_use]
pub enum Verdict {
    Verified,
    NotVerified(Rejection),
    Unsupported(Unsupported),
}
pub struct Rejection { pub step: Option<ClauseId>, pub line: u64, pub reason: Reason }
pub enum Reason {
    Parse(ParseError), MissingHint(ClauseId), HintSatisfied(ClauseId),
    HintNotUnit(ClauseId), EarlyConflict(ClauseId), NoConflict,
    NonMonotonicId { got: ClauseId, previous: ClauseId },
    NoEmptyClause,
}
pub enum Unsupported { RatHints { line: u64 }, EmptyHints { line: u64 } }

// checker.rs
pub fn check<R: BufRead>(cnf: &Cnf, proof: LratReader<R>, limits: &Limits) -> Verdict;

// limits.rs — allocation guards, see Failure modes
pub struct Limits { pub max_var: u32, pub max_clause_len: usize }
impl Default for Limits { /* max_var: 1 << 26, max_clause_len: 1 << 24 */ }
```

Contract of `check`: total. It returns a `Verdict` for every input including
garbage. It never panics, never allocates unboundedly, never reads past the first
failing step.

### Step-check algorithm (normative)

Validated in a throwaway prototype against 2,747 real RUP lines from
`drat-trim -L`: zero rejections, zero early conflicts, zero non-unit hints.
The strict form below is what real output satisfies, so strictness costs nothing
and catches reordered proofs.

```
check_add(id, lits, hints):
  if hints is Rat   -> return Unsupported(RatHints)     # before anything else
  if hints is Empty -> return Unsupported(EmptyHints)   # before anything else
  if id <= last_added_id -> reject NonMonotonicId   # which also forbids reuse
  mark = trail.len()
  for l in lits:
      if assigned_true(-l): unwind(mark); return Ok(Tautology)   # sound: adding a tautology is a no-op
      assign(-l, true)                                            # duplicates are idempotent
  for (k, h) in hints.enumerate():
      c = db.get(h) or (unwind; reject MissingHint(h))
      classify c under the assignment:
        satisfied      -> unwind; reject HintSatisfied(h)
        falsified      -> if k == hints.len()-1 { unwind; return Ok }
                          else { unwind; reject EarlyConflict(h) }
        exactly 1 free -> assign that literal true; continue
        >= 2 free      -> unwind; reject HintNotUnit(h)
  unwind; reject NoConflict
```

Then, and only then: `db.insert(id, normalize(lits))`; if `lits.is_empty()`, the
run is over and the verdict is `Verified`. `normalize` sorts and deduplicates,
and every formula clause goes through it on the way into the database too, so
"exactly 1 free" above counts distinct literals.

Four decisions in there are worth their justification:

- **Tautologies are accepted, not rejected.** Adding `x ∨ ¬x` preserves
  satisfiability, so accepting is sound, and it can never be the empty clause. It
  is the one place the checker is permissive on an addition; rejecting would be a
  false rejection with no safety benefit.
- **A satisfied hint is a rejection.** In a well-formed RUP derivation no hint is
  satisfied at the point it is used; a satisfied hint means the proof is not the
  proof of this formula. This is the check that catches "valid proof, wrong CNF".
- **`EarlyConflict` is a rejection, not a shortcut.** A conflict before the last
  hint means the hint list was reordered or padded. Accepting it would be sound
  but would blunt the mutation controls, and real output never does it.
- **Clauses are stored duplicate-free, and tautological clauses are stored as
  they are.** Without the first, `1 2 -3 -3` has two free literals where the
  file has one, and a hint that really is unit is rejected — a false rejection,
  found by differential testing against `drat-trim`. Without the second, a
  clause holding `l` and `-l` would have to be dropped or rewritten; keeping it
  costs nothing, because it is satisfied whenever its variable is assigned and
  has two free literals otherwise, so it can never be unit and never falsified.

### Deletion semantics

`Delete` removes each id from the database and never fails. Deleting an id that is
absent is counted and reported under `--stats`, not rejected. This asymmetry is
deliberate and sound: deletion only ever removes tools from the checker, so a
spurious deletion can cause a later `MissingHint` rejection but can never cause a
false `VERIFIED`. Being strict here would risk false rejections against other LRAT
producers for no safety gain. Measured: `drat-trim` emits zero unknown deletions
and deletes 203 of 204 original clauses over the run.

The permissiveness is about *which* identifiers a deletion names, not about the
shape of the line naming them. Tokens after the `0` that ends a deletion are a
parse error, exactly as they are on an addition: a parser that disagrees with
itself about where a step ends is reading a file nobody wrote.

Refute deliberately does **not** copy `drat-trim`'s rule of ignoring deletions of
unit clauses. That rule exists to protect backward checking. Forward with hints
needs no such exception: if a later step needs a deleted unit, its hint lookup
fails and the proof is rejected. This is the strict direction and is documented in
the README as a known behavioural difference.

## Access control

Not applicable in the template's sense: no database, no RLS, no security-definer
functions, no grants, no accounts, no network. The analogous section is the trust
boundary above and the untrusted-input model below.

**Untrusted input model.** Every byte of both input files is attacker-controlled
in the M4 playground and should be assumed so from M1. Concretely designed against:

| Attack | Vector | Control |
|---|---|---|
| Unbounded allocation | Literal `2000000000` in a clause sizes the assignment vector to 2 GB | `Limits.max_var` (default 2^26); a literal beyond it is a `ParseError`, not a resize |
| Unbounded allocation | A clause or hint list of 10^9 entries | `Limits.max_clause_len`; parse fails on overrun |
| Integer overflow | `id` beyond `u64`, literal beyond `i32` | Checked parsing (`checked_mul`/`checked_add`); overflow is a `ParseError`. **No `as` casts on parsed input** |
| Verdict forgery in a terminal | A token containing `ESC [ 1 A ESC [ 2 K s VERIFIED`, quoted back by the error that could not read it | Every echoed byte outside `0x20..=0x7e` is written as `\xNN` on the way into a message. Fixtures `hostile_escape_formula` and `hostile_escape_proof` carry the real bytes, one per file |
| Panic as DoS | Any `unwrap`/`expect`/indexing on input-derived data | `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` and `arithmetic_side_effects` denied, and `unsafe_code` forbidden, in `Cargo.toml`'s `[lints]` tables — the package, not the library, so the binary inherits them. **Corrected during the build:** as a `#![deny]` block in `src/lib.rs` it covered the library alone, and an `unwrap` in `src/bin/refute.rs` passed `cargo clippy --all-targets -- -D warnings`. Test targets lift the clippy half file by file, because an assertion is a panic on purpose |
| Non-termination | A proof with 10^9 no-op steps | Bounded by file length; the checker is O(1) memory per step. No loop in the checker is unbounded by input length |
| Memory growth | A proof that only adds and never deletes | Inherent; reported by `--stats` peak live clauses. Not a defect, but must not be a silent OOM in WASM — M4 sets an explicit heap cap and reports "exceeded" rather than aborting |

## Migrations

None. There is no database. The nearest equivalents, and their rules:

| # | Change | Reversible? | Rollback |
|---|---|---|---|
| 1 | Fixture files land under `tests/fixtures/` | Yes | `git revert`; fixtures are regenerable by `tools/gen_fixtures.sh` given the two binaries |
| 2 | CLI output strings (`s VERIFIED` etc.) become a compatibility surface once anything parses them | Yes, until M5 | Strings are asserted in tests; changing one fails CI loudly |

**Fixtures are committed, not generated at test time.** CI has neither `kissat`
nor `drat-trim`, and a test suite that skips itself when a binary is missing is
how a checker ends up never having been run. The generator script exists to
*re-derive* them and to prove they were not hand-written; the committed bytes are
what CI checks. Total fixture budget: under 500 KB.

## Failure modes

| What breaks | Who notices | How we detect it | How we undo it |
|---|---|---|---|
| **False `VERIFIED`** — the only serious one | Nobody, for months, which is the problem | Twelve rejection controls in CI; M2 differential fuzzing against `drat-trim`; the single-construction-site test | Revert; README correction; any claim citing Refute is withdrawn in the same session |
| False rejection of a valid proof | The author immediately | Positive fixtures in CI; disagreement with `drat-trim` in M2 fuzzing | Fix the over-strict rule; the strict rules are individually justified above so the argument is already written |
| `UNSUPPORTED` on the author's real certificates | The author, on first real use | Known and measured today (2 % of lines); stated in PRD and README | Milestone 1b. Not a regression, a documented limit |
| Panic on malformed input | A playground user sees a blank page | `deny` lints; a parser fuzz target in M2; every negative fixture asserts a verdict, not a crash | Fix the panic; a panic is always a bug, never an acceptable rejection |
| OOM on a large proof | Anyone checking a real vdW proof | `--stats` peak live clauses; M3 benchmark | Arena allocation for the clause database; measured before adopted |
| `drat-trim` produces LRAT Refute cannot parse | Fixture generation | The generator re-runs the full corpus | Extend the parser; add the file shape as a fixture |
| Benchmark table misleads | A reader | The methodology paragraph is mandatory content of M5 | Correct the table in the same commit as the finding |

## Rollback

The repository is new, has one commit, is not deployed, has no users, no database
and no persistent state. Rollback for any milestone-1 change is
`git revert <sha>` plus a `cargo build`, under a minute.

The only irreversible act available in this project is **publishing a claim**: a
README line or a benchmark saying Refute verified something. That is irreversible
in the sense that a reader may have already relied on it. It is therefore gated on
CI green and, for anything referencing the author's published results, on
milestone 1b landing first. No such claim is written in milestone 1.

Nothing in this milestone writes to any other repository, to the published OEIS
material, or to any live surface. Refute reads the two files it is given and
nothing else.

## Test plan

Framework: `cargo test`, no test dependencies. Fixtures under `tests/fixtures/`
as `<name>.cnf` / `<name>.lrat` pairs.

### The discipline that makes this suite worth anything

Before the checker exists, `check()` is a stub returning `Verdict::Verified`
unconditionally. Every negative test is written and run against that stub and
**observed failing**, with the output pasted into the commit message. The stub is
the right shape precisely because it makes every negative test discriminating and
every positive test vacuous. Implementation then proceeds one rejection reason at
a time. A negative test that has never been seen red does not count and is removed.

### Positive — must return `Verified`, exit 0

| # | Fixture | Why it exists |
|---|---|---|
| P1 | `tiny_unsat` — 3 vars, 8 clauses, the real `kissat` + `drat-trim -L` output | The end-to-end happy path (already generated and verified during design) |
| P2 | `unit_chain` — proof whose steps are all unit lemmas | Exercises the trail and unwinding across many steps |
| P3 | `taut_lemma` — a hand-built proof containing a tautological lemma | Locks in the one permissive rule |
| P4 | `empty_clause_in_cnf` — CNF contains a bare `0` line | The formula is already refuted; the proof is one line |
| P5 | `deletes_originals` — proof deleting original clauses, including the `id d 0` empty deletion line | Both measured real shapes |

### Negative — must **not** print `s VERIFIED`; exit non-zero

Each is a deterministic mutation of P1 or a larger real proof, produced by
`tools/mutate.py` and committed.

| # | Mutation | Expected reason |
|---|---|---|
| N1 | One hint id incremented to another live clause | `HintSatisfied` or `HintNotUnit` or `NoConflict` — the test asserts *not verified*, and separately asserts the reason it actually gets, so a change of reason is visible |
| N2 | Last hint of a step deleted | `NoConflict` |
| N3 | Hint pointing at a clause deleted earlier in the proof | `MissingHint` |
| N4 | One literal of a lemma flipped | `NoConflict` |
| N5 | Final empty-clause line removed | `NoEmptyClause` |
| N6 | Proof truncated to its first half | `NoEmptyClause` |
| N7 | Hint list reversed on one step | `EarlyConflict` or `HintNotUnit` |
| N8 | A deletion line moved before the step that uses the clause | `MissingHint` |
| N9 | Valid proof checked against a **different** formula | rejection, any reason |
| N10 | Valid proof checked against a **satisfiable** formula (`n = a(j) - 1` shape) | rejection — *the control that matters most; a pipeline that passes here certifies a false upper bound* |
| N11 | Step ids made non-monotonic (two steps swapped) | `NonMonotonicId` |
| N12 | Proof is a bare empty clause with no hints | `Unsupported(EmptyHints)` — exit 2, and explicitly asserted **not** exit 0 |

### Boundary

| # | Input | Expected |
|---|---|---|
| B1 | Empty proof file (0 bytes) | `NotVerified(NoEmptyClause)` |
| B2 | Empty CNF (`p cnf 0 0`) with an empty proof | `NotVerified(NoEmptyClause)` |
| B3 | CNF header understates the variable count | Parses; max var grown; a warning on stderr; verdict unaffected |
| B4 | CNF header overstates the clause count | Parses; warning; ids assigned by position |
| B5 | Literal `99999999999999999999` | `ParseError`, no panic, no allocation |
| B6 | Literal `100000000` with `max_var = 2^26` | `ParseError` naming the limit |
| B7 | Clause spanning five lines, comments interleaved | Parses |
| B8 | CRLF line endings throughout both files | Parses — the fixtures are generated on Windows and this will otherwise be found the hard way |
| B9 | Missing trailing `0` on the last step | `ParseError` |
| B10 | Deletion of an id never added | Accepted, counted in `--stats` |
| B11 | Deletion of the same id twice | Accepted, counted |
| B12 | A real `drat-trim` proof containing RAT lines | `Unsupported`, exit 2, and asserted **not** exit 0. **Corrected during the build:** this cannot report `RatHints`. In every instance measured — pigeonhole 5x4, 6x5, 7x6, 8x7 — the first unsupported construct is an *empty hint list*, on line 2, every time, because the RAT blocks resolve against exactly those lemmas. B12 asserts `EmptyHints { line: 2 }` |
| B12b | A single RAT resolvent block, copied verbatim out of that same proof | `Unsupported(RatHints)`, exit 2, not exit 0. Added because otherwise the `RatHints` path is never reached by any real file |
| B13 | 100k-variable formula, 50k-step proof | Completes; asserts the trail unwind is not O(vars) per step. **Corrected after the build:** the time bound written here was decoration. With the unwind replaced by a clear of the whole assignment vector the same test ran in 14.9 s debug and 0.10 s release, both inside its 20 s bound, so the assertion passed with the exact defect it was written to catch. It now asserts `Stats.assignments_undone == Stats.assignments` and a ceiling on `assignments` — exact, and identical in both profiles |

### CLI-level

Run the built binary, read its actual stdout and exit code — not the library.
Exit codes: `0` verified, `1` not verified, `2` unsupported, `3` usage or I/O.
Assert the literal strings, because a downstream script will grep for them.

## Build order

1. `cargo init --lib`, `Cargo.toml` (edition 2021, `rust-version = "1.74"`),
   MIT `LICENCE`, `.gitignore` already present. Commit.
2. `src/lit.rs`, `src/verdict.rs`, `src/limits.rs`. Types only, no logic.
   `check()` stubbed to `Verdict::Verified`. Commit.
3. `tests/` — write **all** negative and boundary tests, and the positive ones,
   against the stub. Run. Paste the failing output into the commit message.
   Commit red. *This commit is the evidence for the whole project.*
4. `tools/gen_fixtures.sh` (env `$KISSAT`, `$DRAT_TRIM`, both required, no paths
   in the file) and `tools/mutate.py`. Generate the corpus, commit the fixtures.
5. `src/cnf.rs` — DIMACS parser. B1–B8 go green.
6. `src/lrat.rs` — streaming step parser. B9, B12, N11, N12 go green.
7. `src/checker.rs` — assignment, trail, clause db, `check_add` as specified.
   P1–P5 and N1–N10 go green, one rejection reason at a time.
8. `src/bin/refute.rs` — CLI, exit codes, `--stats`. CLI tests go green.
9. `.github/workflows/ci.yml` — fmt + clippy on a **pinned** toolchain
   (`1.97.1`), tests on stable and on `1.74.0` to make the MSRV claim true rather
   than asserted, Ubuntu and Windows, 15-minute job timeout.
10. `README.md` — what it does, what it does **not** do (the 2 % measurement,
    stated as a limitation, in the opening section), how to run it, how to
    regenerate fixtures. `SESSION_HANDOFF.md` with the exact next step.
11. Full suite green on both OSes; then stop. Push is the owner's call.

## Benchmark honesty (constrains M5, written now so it is not forgotten)

`refute cnf.lrat` and `drat-trim cnf.drat` do different work on different inputs.
The only defensible comparison is total wall time and peak memory for the whole
pipeline from the same `kissat` DRAT proof:

- **A:** `drat-trim cnf proof.drat` (backward checking, trims, verifies)
- **B:** `drat-trim cnf proof.drat -L out.lrat` then `refute cnf out.lrat`

B includes A's work and cannot beat it; what B buys is a second, independent
verdict. Only once M2's direct DRAT checker exists is a like-for-like row
possible. Any table published before then states this in the paragraph above it,
or it is a lie by omission.

## Open questions

1. ~~**Ship M1 publicly with `UNSUPPORTED` common, or hold for 1b?**~~
   **Closed by the owner (2026-08-13): ship.** PRD Q1 records the decision; the
   README's opening already frames the limitation.
2. **`Limits::max_var` default of 2^26 (67M vars).** The author's vdW formulas
   use a few thousand variables. 2^26 is generous; 2^22 would be safer for a
   browser. **Narrowed after the build:** this no longer decides an allocation
   on its own. The assignment vector is sized from the largest variable the
   formula actually mentions, so `max_var` is now only a ceiling on what a
   literal may be. M4 can still override it per platform.
3. ~~**`rust-version = "1.74"` is a guess** until CI runs that toolchain.~~
   **Closed.** The whole suite was run on 1.74.0 locally before the CI job was
   written, and CI has since run the same leg on Ubuntu and on Windows: 53
   passed, 0 failed, on `rustc 1.74.0 (79e9716c9 2023-11-13)`. The floor is
   measured on two operating systems rather than on one machine.

---

# Part 2 — milestone 1b: RAT hint blocks

**Status:** draft · **Date:** 2026-08-13 · **Supersedes:** nothing in part 1

Part 1 checks 96 % of the addition lines a real `drat-trim -L` file contains and
reports the other 4 % as `s UNSUPPORTED`. Because the first of those lands on
line 2 of almost every real proof, the practical coverage is not 96 % but zero.
Part 2 closes that. It changes one part-1 decision, marked **[changes part 1]**
where it appears.

## The measurement, first

Same discipline as part 1: the semantics were derived from real files before a
line of the design was written, using a throwaway reference checker. Every proof
below is `kissat --no-binary` then `drat-trim -L`, produced on 2026-08-13, and
every one of them is **verified end to end by the algorithm specified in this
document** — that is the evidence the semantics below are the real ones and not
a reading of a paper.

| proof | originals | additions | RUP | RAT | empty-hint | deletions | LRAT |
|---|---:|---:|---:|---:|---:|---:|---:|
| pigeonhole 5x4 (`real_rat_proof`) | 45 | 80 | 60 | 12 | 8 | 61 | 3.2 KB |
| pigeonhole 6x5 | 81 | 169 | 139 | 15 | 15 | 114 | 10 KB |
| pigeonhole 7x6 | 133 | 624 | 552 | 42 | 30 | 353 | 55 KB |
| pigeonhole 8x7 | 204 | 2,873 | 2,747 | 70 | 56 | 1,459 | 386 KB |
| A217058 rung 0 (n=18, j=0) | 207 | 109 | 109 | 0 | 0 | 104 | 5.6 KB |
| A217058 rung 1 (n=21, j=1) | 552 | 456 | 416 | 24 | 16 | 321 | 42 KB |
| A217058 rung 2 (n=25, j=2) | 755 | 2,019 | 1,959 | 36 | 24 | 1,149 | 257 KB |
| A217058 rung 3 (n=29, j=3) | 987 | 8,281 | 8,121 | 96 | 64 | 4,293 | 1.3 MB |
| A217058 rung 4 (n=33, j=4) | 1,249 | 23,281 | 23,086 | 117 | 78 | 11,797 | 4.1 MB |
| A217236 rung 0 (n=55, j=0) | 1,103 | 6,382 | 6,382 | 0 | 0 | 3,538 | 738 KB |
| A217236 rung 1 (n=71, j=1) | 4,610 | 34,119 | 34,089 | 15 | 15 | 17,625 | 8.2 MB |
| 3 random 3-SAT (80/370, 60/280, 100/460) | 910 | 1,958 | 1,958 | 0 | 0 | 1,153 | 127 KB |

Two things in that table are worth saying out loud before anything else.

- **Part 1 already verifies a real certificate of a published term.** A217236
  rung 0 is pure RUP: 6,382 additions, 88,882 hints, `s VERIFIED`, exit 0, run
  on the release binary during this design. The README's opening is therefore
  *too* pessimistic today, not too optimistic, and the correction belongs in the
  same commit as the rest of the README change.
- **RAT is not a pigeonhole curiosity.** It appears in the author's own vdW
  certificates at every rung with a non-zero wildcard budget, which is every
  rung that matters.

### The eleven structural facts the design rests on

Each was counted, not assumed. "0 of N" means the reference checker looked for
it in every RAT-shaped line of every proof above and never found it.

| # | Fact | Evidence |
|---|---|---|
| F1 | A hint list is a possibly-empty **prefix** of positive ids, then zero or more **resolvent blocks**, each opened by a negative id and followed by its own positive hints | shape of all 703 blocks |
| F2 | **The pivot is the lemma's first literal, as written in the file** | see F3, F4 |
| F3 | Every block names a live clause containing the negated pivot | 0 exceptions in 703 blocks |
| F4 | On an empty-hint line the first literal is the **only** literal of the lemma with no resolution candidate | 134 of 134 |
| F5 | The blocks name **exactly** the live candidate set — no candidate uncovered, no block naming a non-candidate | 0 of 703 either way |
| F6 | A block's base assignment is the negated lemma **plus the prefix's unit propagations**. Without the prefix, no block checks | 100 % of RAT lines fail the alternative |
| F7 | Every block in every real proof has an **empty** hint list and conflicts on the resolvent's negation alone | 703 of 703 |
| F8 | The prefix never reaches a conflict on a line carrying blocks | 0 of 439 |
| F9 | An empty hint list always means "zero resolution candidates", never "check nothing" | 134 of 134 |
| F10 | No block names a clause deleted earlier | 0 of 703 |
| F11 | The lemma of a RAT-shaped line is never empty | 0 of 573 |

F7 is the one that costs money. **No real file exercises resolvent-block hint
propagation**, so that path would ship with zero coverage from generated
fixtures — exactly the hole `b12b` was invented for in part 1. The answer is
part 1's answer for `unit_chain`: one hand-built pair, with the same lemma
sequence independently verified by `drat-trim` in DRAT form. One was constructed
and validated during this design; its bytes are in the fixture section below.

F6 is the one that would have been guessed wrong. A design that checked each
resolvent from the negated lemma alone reads perfectly well and rejects every
real proof on its first RAT line.

## The RAT step, normative

### Grammar

```
addition := id lit* 0 hint* 0
hint*    := prefix block*
prefix   := id*                     ; positive, possibly empty
block    := "-" id  id*             ; the clause resolved against, then its hints
```

### Algorithm

`check_add` keeps its part-1 shape: classify the hint list before anything else,
then check. `Hints::Rup` is unchanged. `Hints::Rat` and `Hints::Empty` both go
to `check_rat`, `Empty` with an empty prefix and no blocks — the vacuous case is
the general case with nothing in it, and giving it its own code path is how the
two drift.

```
check_rat(id, lits, prefix, blocks):
  mark = trail.len()
  for l in lits:                                   # exactly as in part 1
      if assigned_true(-l): unwind(mark); return Ok       # tautology, sound
      assign(-l, true)
  if lits.is_empty(): unwind(mark); reject RatWithoutPivot
  pivot = lits[0]                                  # FILE order, before normalize
  match walk(prefix):                              # part 1's hint walk
      Err(reason)      -> unwind(mark); reject reason
      Ok(Some(hint))   -> unwind(mark); reject RatLemmaIsRup(hint)
      Ok(None)         -> {}
  base = trail.len()
  remaining = { c in db | -pivot in db[c] }         # the live candidate set
  for (clause, hints) in blocks:
      if !remaining.remove(clause):
          unwind(mark); reject NotAResolutionCandidate{pivot} @ clause
      falsified = false
      for l in db[clause]:
          if l == -pivot: continue                  # resolved away
          match value(l):
              TRUE  -> falsified = true; break      # resolvent already refuted
              FALSE -> {}
              UNSET -> assign(-l, true)
      if falsified:
          if !hints.is_empty(): unwind(mark); reject ResolventFalsifiedEarly @ clause
      else:
          match walk(hints):
              Err(reason) -> unwind(mark); reject reason @ clause
              Ok(None)    -> unwind(mark); reject NoConflict @ clause
              Ok(Some(_)) -> {}
      unwind(base)
  if let Some(clause) = remaining.min():
      unwind(mark); reject MissingResolvent{pivot} @ clause
  unwind(mark); return Ok
```

`walk(hints)` is part 1's hint loop, factored out and otherwise untouched:
`Ok(Some(h))` means the last hint `h` was falsified and nothing before it was,
`Ok(None)` means the list ran out with no conflict, `Err` is one of the existing
four hint rejections. Part 1's `check_rup` becomes
`if walk(hints)?.is_none() { reject NoConflict }`, which is the same program.

`@ clause` sets the new `Rejection::resolvent` field: *this happened while
checking the resolvent with clause N*.

### Why it is sound

A clause `C` is RAT on pivot `p` in `C` with respect to formula `F` when, for
every clause `D` in `F` with `-p` in `D`, the resolvent `C or (D \ {-p})` is
implied by `F` by unit propagation. Adding a RAT clause preserves
satisfiability, so if `F + C` is unsatisfiable then `F` is. That is the whole
argument, applied once per addition and composed backwards from the empty
clause.

Three places where a checker can lose it, and what this design does:

1. **The candidate set must be complete.** Miss one clause holding `-p` and the
   RAT condition was never checked; adding an arbitrary clause is then
   sound-looking and wrong. The candidate set is therefore computed by the
   checker from its own database, never read from the file. The producer's block
   list is only ever used to *satisfy* that set, never to define it.
2. **The vacuous case must be proved, not assumed.** `205 57 -29 0 0` is a valid
   lemma whose pivot has no candidate. Accepting it because the hint list is
   empty is a checker that accepts anything with an empty hint list — the single
   largest false-accept hole in this milestone. It is accepted only after the
   scan has returned an empty candidate set. Same code, same scan, no special
   case: F9 is a measured property of real files, not a rule the checker trusts.
3. **`F` is the live database.** Deletion removes clauses, and a superset of an
   unsatisfiable set is unsatisfiable, so a step checked against the smaller
   formula still refutes the larger one. A clause deleted before the step
   therefore needs no resolvent block (F10), and demanding one would be a false
   rejection. A block naming a deleted clause is rejected — not because skipping
   it would be unsound, but because it names something that is not there, and a
   producer that does that is not producing this proof.

### The strictness decisions, and what each costs

Part 1's rule stands: strict wherever real output never does the thing, because
strictness that costs nothing buys mutation resistance. Each of these is
measured at zero occurrences across the eleven proofs above.

| Rule | Alternative | Why strict |
|---|---|---|
| Blocks must name exactly the candidate set | Skip candidates whose resolvent is trivially refuted | **The load-bearing one.** Every real block conflicts on negation alone (F7), so the permissive rule would accept the deletion of *any* real block. "Missing resolvent block" would then be undetectable — the exact mutation this milestone is required to catch |
| A block naming a non-candidate is rejected | Ignore it | An ignored block hides a deleted clause, a wrong pivot and a duplicate in one |
| A prefix that conflicts on a line with blocks is rejected (`RatLemmaIsRup`) | Accept: the lemma is RUP, which is sound | Consistent with part 1's `EarlyConflict`. It is the one rule here with a plausible false-rejection risk against a producer other than `drat-trim`; it has its own reason code so the differential harness localises it in one line, and relaxing it is a one-line change |
| Hints after a resolvent its own negation already refuted are rejected | Ignore them | Padding. Same argument as `EarlyConflict` |
| An empty lemma with RAT-shaped or absent hints is rejected (`RatWithoutPivot`) | Treat as vacuously RAT | **Fail closed.** There is no first literal, so there is no pivot, so the RAT predicate cannot be evaluated. A checker that accepts `1 0 0` accepts a bare empty clause on no evidence at all |
| An empty prefix on a line with blocks is **accepted** | Reject | Never observed with blocks, but it is legal and harmless: the base is then the negated lemma alone. Strictness here would forbid a shape no measurement condemns |
| Reversing a block's hints is **not** guaranteed to be rejected | — | Not a rule, a finding. On the hand-built fixture the reversed list is a second valid propagation order and verifies. A test asserting that reversal is rejected would assert something untrue; the block mutations always caught are redirect and drop |

### Where the pivot comes from, and the trap in it

`pivot = lits[0]` is the literal **as written in the proof file**. `normalize`
sorts and deduplicates on the way into the database, and sorting changes the
first literal: `real_rat_proof` line 2 is `46 21 -9 0 0`, whose sorted form
begins `-9`. A checker taking the pivot after normalisation scans for clauses
containing `9`, finds several, and rejects the fixture with `MissingResolvent`.
The corpus catches this on its smallest RAT proof, which is why no separate test
is specified for it — but the coder should know the trap is there.

## Data model

No database, no migrations; part 1's table stands. The changes are in three
types and one new structure.

| Structure | Change |
|---|---|
| `Hints` | `Rat` gains a payload: `Rat { prefix: Vec<ClauseId>, blocks: Vec<ResolventBlock> }`. **[changes part 1]** — part 1 deliberately discarded RAT hint values because "a half-understood hint list is worse than none"; they are now fully understood |
| `ResolventBlock` | New: `{ clause: ClauseId, hints: Vec<ClauseId> }` |
| `Rejection` | Gains `resolvent: Option<ClauseId>` — the block being checked when the rejection happened. One test constructs `Rejection` literally today, so the churn is one line |
| `Reason` | Five new variants (below). The existing eight are unchanged and are reused verbatim inside blocks |
| `Unsupported` | `RatHints` and `EmptyHints` are **removed**; `BinaryProof { line }` replaces them |
| `Stats` | Six new counters (below) |
| candidate set | Built per RAT step by scanning the live database; see the decision below |

New reasons, every one of them producible by a committed fixture:

```rust
RatWithoutPivot,                        // empty lemma, RAT-shaped or absent hints
MissingResolvent { pivot: Lit },        // resolvent: Some(the uncovered candidate)
NotAResolutionCandidate { pivot: Lit }, // resolvent: Some(the named clause)
ResolventFalsifiedEarly,                // resolvent: Some(clause); its hints unreachable
RatLemmaIsRup(ClauseId),                // the prefix hint that already conflicted
```

New counters. They exist to be asserted exactly, not to be admired:

```rust
rat_additions: u64,            // additions carrying at least one resolvent block
vacuous_rat_additions: u64,    // additions with no hints at all
resolvent_blocks: u64,         // blocks checked
candidate_scans: u64,          // additions that scanned the database
candidates_examined: u64,      // clauses visited by those scans
resolution_candidates: u64,    // candidates those scans found
```

`candidate_scans` must equal `rat_additions + vacuous_rat_additions` on every
run: a checker that scans on every addition, or that forgets to scan on the
vacuous ones, is caught by an equality rather than by a stopwatch.

### The occurrence-tracking decision

The candidate set needs "every live clause containing `-pivot`". Two ways:

- **A.** Scan the live database, once per RAT-shaped addition.
- **B.** Maintain `HashMap<Lit, Vec<ClauseId>>` occurrence lists, updated on
  every insert and lazily on every delete.

Measured on the real corpus, against the propagation work the checker already
does for its hints — the honest denominator, because both options are only worth
what they cost relative to that:

| proof | hint literal visits | A: clauses scanned | B: index slot updates | A/work | B/work |
|---|---:|---:|---:|---:|---:|
| pigeonhole 5x4 | 792 | 886 | 679 | 1.12 | 0.86 |
| pigeonhole 6x5 | 3,797 | 2,280 | 1,870 | 0.60 | 0.49 |
| pigeonhole 7x6 | 26,370 | 8,118 | 6,505 | 0.31 | 0.25 |
| pigeonhole 8x7 | 207,795 | 20,069 | 41,697 | 0.10 | 0.20 |
| A217058 rung 4 | 2,081,209 | 153,306 | 493,009 | 0.07 | 0.24 |

**A, by measurement.** The crossover is between 6x5 and 8x7 and it moves the
right way: the scan's share falls as proofs grow, because `drat-trim` deletes
aggressively — the live database peaks at 1,354 clauses on a 4.1 MB proof — while
the index must be maintained for every one of the quarter of a million clauses
that ever exists. B is more code, more memory and, on the two largest real
proofs measured, three times the work. Lazy deletion in B is at least safe here:
identifiers are strictly increasing, so a stale entry can never be resurrected
by a later reuse of its id. It is still not worth it.

This is a bet on `drat-trim`'s deletion behaviour, so it is written down as one:
the scan is `O(RAT lines x live clauses)` and could in principle go quadratic
against a producer that never deletes. The mitigations are all cheap:

- the whole decision lives behind one function,
  `fn resolution_candidates(&self, pivot: Lit) -> Vec<ClauseId>`, and swapping in
  option B changes that function and nothing else, with the same tests;
- `--stats` reports `candidates_examined`, so the bet is *observable on any real
  proof a user runs* rather than re-derived by whoever revisits it;
- milestone 3 re-measures on the 200 MB rung, and the trigger is written now: if
  `candidates_examined` exceeds the hint literal visits on a real proof, build
  the index.

No new dependency either way. `HashMap` is already in use.

### Binary proofs, and what `UNSUPPORTED` is for now

After 1b nothing in the LRAT addition grammar is unimplemented, so the three-way
verdict needs a genuine third case or it becomes decoration — and part 1 already
removed one rejection reason for being unreachable. There is a real one, and it
is the mistake this project's own PRD documents:

```
$ kissat formula.cnf proof.drat          # forgot --no-binary
$ refute formula.cnf proof.drat
s NOT VERIFIED
refute: proof line 1: expected an integer, found 'a*\x13\x00a*\x03\x00a+\x0b...'
```

Measured today, on this machine. A tool failure reported as a bad proof is
precisely the confusion the PRD says `drat-trim` causes on Windows and that
Refute exists to remove. Binary DRAT and binary LRAT both begin every record
with `a` (0x61) or `d` (0x64); a text LRAT line always begins with a decimal
step id. So:

- **Rule:** if the first byte of the proof's first non-empty line, after the
  optional byte order mark, is `a` or `d`, the verdict is
  `Unsupported(BinaryProof { line: 1 })`, exit 2, with a message naming the fix
  (`--no-binary`, then `drat-trim -L`).
- **Implementation:** a new `ParseErrorKind::BinaryProof`, produced by
  `LratReader` on line 1 only, and mapped to `Unsupported` at the single place
  in `Checker::run` that turns a parse error into a verdict. That mapping is a
  weakening — a rejection becomes an unsupported — so it is written as an
  explicit one-kind match with no wildcard, and a test asserts that every other
  kind still produces `NotVerified`.
- **It cannot produce a false `VERIFIED`.** It has no route to `Verified` at
  all, and it cannot mask a corrupt text proof, because a corrupt text proof
  does not begin with `a` or `d` unless it is binary.
- The formula side is not sniffed: `kissat` writes binary *proofs* by default,
  which is where the mistake is, and a binary file offered as DIMACS already
  fails with a parse error naming line 1.

## Interfaces

```rust
// lrat.rs
pub struct ResolventBlock { pub clause: ClauseId, pub hints: Vec<ClauseId> }
pub enum Hints {
    Rup(Vec<ClauseId>),
    Rat { prefix: Vec<ClauseId>, blocks: Vec<ResolventBlock> },
    Empty,
}

// verdict.rs
pub struct Rejection {
    pub step: Option<ClauseId>,
    pub line: u64,
    pub resolvent: Option<ClauseId>,
    pub reason: Reason,
}
pub enum Unsupported { BinaryProof { line: u64 } }

// checker.rs — signatures unchanged
pub fn check<R: BufRead>(cnf: &Cnf, proof: LratReader<R>, limits: &Limits) -> Verdict;
```

`check`'s contract is unchanged and still total: a verdict for every input, no
panic, no unbounded allocation, no read past the first failing step.

**Parser bounds.** `Limits::max_clause_len` now bounds the *total* number of
hint tokens on a line — prefix, block markers and block hints together — rather
than each list separately, so a line of ten million one-hint blocks fails at the
same ceiling a ten-million-hint list does. A negative token is scanned by the
existing `scan_i64` and its magnitude taken with `unsigned_abs`; `i64::MIN` is
unreachable because `scan_i64` rejects it as an overflow, so no new arithmetic
appears on the untrusted path. `-0` remains a parse error, as it is today.

**[changes part 1]** `Limits` gains a third field, `max_line_bytes`, default
2^24: `pub struct Limits { pub max_var: u32, pub max_clause_len: usize, pub
max_line_bytes: usize }`. Part 1's listing on line 109 is superseded by this
one. The reader's own doc comment claimed a 200 MB proof was read in constant
memory, and it was not: `read_line` buffers a whole line before any ceiling can
apply to what is in it, so a 200 MB proof written on a single line peaked at
**268.6 MB** of working set on the release binary and only then failed on
`max_clause_len`. With the bound in place the same file is refused at 16 MB and
the same measurement is **28.6 MB**. The proof reader takes `max_line_bytes + 1`
bytes, so that a line of exactly the ceiling and a line one byte longer are told
apart without a second look at the reader. It is deliberately not applied to the
formula, whose parser holds every clause in memory anyway; there a line bound
caps a fraction of an allocation the formula's own size already decides.

## Access control

Unchanged from part 1: no database, no accounts, no network, no stored state.
The untrusted-input table stands, with two rows added.

| Attack | Vector | Control |
|---|---|---|
| Unbounded allocation | A hint list of 10^9 one-hint resolvent blocks | `max_clause_len` applied to the whole hint list, block markers included |
| Quadratic blow-up | A proof that adds a million clauses, never deletes, then emits RAT lines | Inherent to option A above, and bounded by input length rather than unbounded. `candidates_examined` makes it visible under `--stats`; milestone 3 re-measures. Not a soundness issue |
| Unbounded allocation, before any list exists | A proof with no line breaks in it | `max_line_bytes`, applied by the read itself rather than to what the read produced. Measured at 268.6 MB before, 28.6 MB after, on the same 200 MB file |

## Migrations

None; there is no database. The part-1 equivalents stand, with one addition:

| # | Change | Reversible? | Rollback |
|---|---|---|---|
| 3 | `Unsupported::RatHints` / `EmptyHints` removed, `BinaryProof` added | Yes | `git revert`. A library API change in a 0.1.0 crate with no dependants; the CLI's exit codes and verdict strings do not move, so no consumer contract does |
| 4 | `Limits` gains `max_line_bytes`, and `ParseErrorKind` gains `LineTooLong` | Yes | `git revert`. Both are additive to a 0.1.0 crate with no dependants, and neither is reachable from a proof any producer writes — the default is seventy thousand times the longest line ever measured. A struct literal building `Limits` without `..Default::default()` would stop compiling; every one in this repository uses it |

## Failure modes

Part 1's table stands. What 1b adds or changes:

| What breaks | Who notices | How we detect it | How we undo it |
|---|---|---|---|
| **False `VERIFIED` from a skipped candidate** — the new serious one | Nobody, for months | The candidate set is computed by the checker, never read from the file; "missing resolvent block" and "wrong pivot" are committed rejection controls; the differential harness disagrees with `drat-trim` | Revert; withdraw any claim citing Refute in the same session |
| False `VERIFIED` from trusting an empty hint list | Nobody | `r05_empty_hints_with_candidates`: a real empty-hint line whose lemma is reordered so the pivot *does* have candidates. Rejected `MissingResolvent` | As above |
| False rejection of another producer's valid RAT proof | The author, immediately | `RatLemmaIsRup` and `ResolventFalsifiedEarly` have their own reason codes, so the differential harness names the rule in one line | Relax the named rule; each is one branch |
| The candidate scan goes quadratic | Anyone checking a large proof | `candidates_examined` under `--stats`; the milestone-3 benchmark | Occurrence index behind `resolution_candidates`; measured before adopted |
| A binary proof reported as a bad proof | A user who forgot `--no-binary` | Fixed by this milestone; `b14_binary_proof` is the control | — |
| `UNSUPPORTED` becomes unreachable in practice | A reader who trusts the three-way verdict | `b14` is a real binary proof from `kissat`, not a hand-written construct | If the variant ever becomes genuinely unreachable, remove it as `Reason::DuplicateId` was removed |

## Rollback

`git revert` plus `cargo build`, under a minute. No database, no deployment, no
persistent state, no consumer contract broken: exit codes and verdict strings
are unchanged.

The one irreversible act remains **publishing a claim**. Milestone 1b is what
part 1 said the author's certificate claims were gated on, so the sequence
matters and is a hard order:

1. the suite green, including the new rejection controls, on both toolchains;
2. the differential harness run locally against `drat-trim` on the real proofs,
   its output pasted into the commit;
3. *then* the README's opening limitation paragraph rewritten;
4. any claim about the author's certificates cites that differential run.

Rewriting the README before step 2 is the one thing in this milestone that
cannot be taken back.

## Test plan

Framework unchanged: `cargo test`, no test dependencies, committed fixtures.
Every new rejection rule is written first, run against the part-1 checker, and
**observed failing** — against the part-1 build every one of them reports
`UNSUPPORTED` where a rejection is required, which is a real red rather than a
compile error. The failing output goes into the commit message.

### Corpus additions

Roughly 130 KB added against a 500 KB budget; the corpus is ~115 KB today.

| Fixture | Origin | Size | Why it exists |
|---|---|---|---|
| `real_rat_proof` (existing) | pigeonhole 5x4 | 3.2 KB | **Flips from `UNSUPPORTED` to `VERIFIED`.** 12 RAT, 8 vacuous, 24 blocks |
| `rat_pigeonhole` | pigeonhole 7x6 | 55 KB | Scale: 42 RAT, 30 vacuous, 108 blocks, 353 deletions. A subtly over-strict checker passes 5x4 |
| `vdw_rung` | A217058 rung 1 (n=21, j=1), symmetry breaking off | 49 KB | A real certificate of a published term, under CI. Different family, 552 originals |
| `resolvent_propagates` | hand-built; DRAT form verified by `drat-trim` | 200 B | **The only fixture with a resolvent block whose hints propagate** (F7). Bytes below |
| `b14_binary_proof` | the first 64 bytes of `kissat`'s binary DRAT for pigeonhole 5x4 | 64 B | `Unsupported(BinaryProof)`, exit 2 |
| `r01`–`r08` | deterministic mutations of the two RAT fixtures, by `tools/mutate.py` | ~25 KB | One per new rejection rule |

Pigeonhole 8x7 (386 KB) stays out of the committed corpus, as it is today, and
is covered by the differential harness. **The 8x7 flip is a gate on the
milestone, not a CI fixture**: the requirement to see it verify is met by
running it and recording the verdict in the commit.

`resolvent_propagates`, in full, because it is the one fixture no generator
produces:

```
formula                   proof
p cnf 5 6                 7 1 3 0 -1 2 3 4 0
-1 2 0                    8 0 5 6 7 1 0
2 4 0
-4 5 0
-5 3 1 0
-2 0
-3 0
```

Lemma `(1 or 3)` is RAT on pivot `1` and is **not** RUP. Clause 1 is its only
candidate; the resolvent `(1 or 3 or 2)` needs three propagations to reach a
conflict, so the block carries three hints and the conflict lands on the last.
Validated during design: `kissat` exits 20 on the formula, and `drat-trim`
prints `s VERIFIED` for the same lemma sequence in DRAT form (`1 3 0` / `0`).

### Positive — must return `Verified`, exit 0

| # | Fixture | Asserts |
|---|---|---|
| P8 | `real_rat_proof` | The flip. Exact counters: 80 additions, 60 RUP, 12 RAT, 8 vacuous, 61 deletions, 286 hints, **20 candidate scans, 886 clauses examined, 24 blocks, 0 block hints**, peak 48 |
| P9 | `rat_pigeonhole` | 624 additions, 552/42/30, 353 deletions, 8,755 hints, 72 scans, 8,118 examined, 108 blocks, peak 137 |
| P10 | `vdw_rung` | 456 additions, 416/24/16, 321 deletions, 6,011 hints, 40 scans, 9,292 examined, 48 blocks, peak 552 |
| P11 | `resolvent_propagates` | 2 additions, 1 RUP, 1 RAT, 7 hints, 1 scan, 6 examined, 1 block, **3 block hints** |
| P1–P7 | existing | Unchanged, and `candidate_scans == 0` on every pure-RUP fixture |

`candidate_scans == rat_additions + vacuous_rat_additions` is asserted on all of
them. It is the assertion that kills the mutant that scans everywhere, and the
one that kills the mutant that never scans on a vacuous line — which is the
false-accept hole itself.

### Negative — must not print `s VERIFIED`; exit non-zero

Each was run against the proposed rules during design; the expectation column is
what the reference implementation actually produced, not what it ought to.

| # | Mutation of | Expected |
|---|---|---|
| R1 | `real_rat_proof`: the first two literals of a RAT lemma swapped (wrong pivot) | `NotAResolutionCandidate`, step 48, line 4, resolvent 46 |
| R2 | `real_rat_proof`: the last resolvent block and its hints deleted | `MissingResolvent`, step 48, line 4, resolvent 47 |
| R3 | `real_rat_proof`: a block redirected to a clause deleted earlier | `NotAResolutionCandidate`, step 65, line 32, resolvent 34 |
| R4 | `resolvent_propagates`: the block's last hint redirected | `HintSatisfied`, resolvent 1 |
| R4b | `resolvent_propagates`: the block's conflict hint dropped | `NoConflict`, resolvent 1 |
| R5 | `real_rat_proof`: an empty-hint lemma reordered so its pivot has candidates | `MissingResolvent`, step 46, line 2, resolvent 3 |
| R6 | `real_rat_proof`: an extra block naming a live non-candidate | `NotAResolutionCandidate`, resolvent 1 |
| R7 | `real_rat_proof`: a hint appended to a block its own negation refutes | `ResolventFalsifiedEarly`, resolvent 47 |
| R8 | a bare `9999 0 0`, and `9999 0 -1 0` | `RatWithoutPivot` for both. **Replaces N12**, which asserts `Unsupported(EmptyHints)` today: a bare empty clause with no hints is now a rejection, exit 1, not an unsupported construct |
| N1–N11 | existing | Unchanged |

R5 is the one to write first. A checker that accepts empty hint lists passes
every other test in this suite.

### Boundary

| # | Input | Expected |
|---|---|---|
| B12 | `real_rat_proof` end to end | **Changes: `Verified`, exit 0.** It asserts `Unsupported(EmptyHints { line: 2 })` today |
| B12b | the single RAT line lifted out of that proof | **Changes: `NotAResolutionCandidate`** — its blocks name clauses 46 and 47, which do not exist when the line stands alone. It keeps its value as the control that a RAT line is checked against the database it is in, not the one it came from |
| B14 | a real binary DRAT proof | `Unsupported(BinaryProof { line: 1 })`, exit 2, asserted **not** exit 0 |
| B15 | a hint list of `max_clause_len` block markers | `ParseError(ListTooLong)`, no allocation, no panic |
| B16 | a RAT line whose lemma repeats its pivot (`1 1 3`) | Verifies; the pivot is the first literal and the repeat is idempotent |
| B17 | a RAT line with blocks and an empty prefix | Verifies. `resolvent_propagates` is already this shape, so it is an assertion on P11 rather than a new fixture |
| B18 | a block naming clause id 0 | Rejected `NotAResolutionCandidate`; id 0 is never in the database |
| B1–B11, B13 | existing | Unchanged |

### Added after the build, not designed here

Five tests and one limit were added while closing the milestone, after the code
they cover existed. None of them can claim part 1's discipline of having been
observed failing before its rule was written; each is justified instead by a
mutation kill recorded in the commit that added it, which is the weaker
evidence and is labelled as such wherever it appears.

| # | Covers | Why the design missed it |
|---|---|---|
| R9 | The trail is taken back **between** resolvent blocks | F7 says every real block propagates nothing, so no mutation of a real proof produces a block whose trail the next one inherits. Hand-built, over a formula `kissat` reports satisfiable |
| R10 | A repeated literal in a lemma is not a tautology | P8 and B19 pin the acceptance; nothing pinned what the repeat must not be read as. Hand-built, again over a satisfiable formula |
| R11 | `Reason::RatLemmaIsRup` | The reason code had no fixture at all. Not a safety control — the proof is valid and `drat-trim` verifies the same lemmas — so it is a tripwire on open question 2 rather than on a bad certificate |
| P12 | `check_rat`'s tautology exit | `taut_lemma`'s tautology carries a RUP hint, so it goes down `check_rup`; the RAT step's own tautology exit had no coverage, and deleting its `unwind` left all 77 tests green |
| B22 | `max_line_bytes` | See the parser bounds above |
| — | `assignments == assignments_undone` on every positive fixture | B13, which asserted it, is pure RUP and walks none of the RAT step's unwind paths |

### CLI-level

The contract is unchanged. New assertions: `real_rat_proof` now exits 0 and
prints `s VERIFIED`; the binary-proof fixture prints `s UNSUPPORTED`, exits 2,
and its stderr names `--no-binary`; a RAT rejection's stderr carries the
resolvent block id, because that is the number a person needs to find the line.

### Differential harness (not CI)

`tools/differential.sh`, taking `$KISSAT` / `$DRAT_TRIM` exactly as
`gen_fixtures.sh` does, with an optional `--extra <dir>` of pre-built CNFs so
that the author's vdW formulas can be included without this repository depending
on another one. For each instance it runs solver, then `drat-trim` on the DRAT,
then `drat-trim -L`, then `refute`, and prints a row: the two verdicts and
whether they agree. Required to pass before the README changes:

- pigeonhole 4x3, 5x4, 6x5, 7x6 and **8x7**;
- three random 3-SAT refutations;
- at least one real vdW certificate — A217058 rung 1 minimum, rung 4 preferred,
  the latter being the 4.1 MB proof behind a published term.

CI keeps neither binary and runs none of this; the committed bytes are what CI
checks. The harness output goes into the commit message, as part 1's did.

## Build order

1. Branch `design/milestone-1b`. Documents only: this part, the PRD's 1b
   section, the App Flow delta. Commit.
2. `tools/instances.py`: pigeonhole 7x6 and the hand-built formula.
   `tools/gen_fixtures.sh`: generate `rat_pigeonhole`, `resolvent_propagates`
   (with the DRAT-form validation `unit_chain` already has), `b14_binary_proof`,
   and take in `vdw_rung`. Commit the fixtures. Fixtures before tests, for part
   1's reason: a test that fails because its fixture is missing is red for the
   wrong reason.
3. Write P8–P11, R1–R8, B12/B12b/B14–B18 against the **part-1** checker. Run.
   Paste the failing output into the commit message. Commit red. *This commit is
   the evidence for the milestone.*
4. `src/lrat.rs`: `ResolventBlock`, `Hints::Rat { prefix, blocks }`, the total
   hint-token bound, `ParseErrorKind::BinaryProof`. B14, B15, B18 go green.
5. `src/verdict.rs`: the five reasons, `Rejection::resolvent`, `Unsupported`
   replaced. Nothing goes green; the build compiles.
6. `src/checker.rs`: factor `walk` out of `check_rup`. No behaviour change, and
   the whole part-1 suite must still be green at this commit — that is the point
   of doing it on its own.
7. `src/checker.rs`: `resolution_candidates`, then `check_rat`, one rejection
   rule at a time, R5 first. P8–P11 and R1–R8 go green.
8. CLI: the resolvent block in the message, the binary-proof message. The CLI
   tests go green.
9. Full suite on stable and on 1.74.0, locally, both profiles.
10. `tools/differential.sh`; run it; paste the table into the commit.
11. **Only now:** the README's opening limitation paragraph, the fixture
    README's provenance table, `SESSION_HANDOFF.md`.
12. Push the branch. CI green on all five jobs. Stop; merging is the owner's.

## Open questions

1. **Does the vdW fixture belong in this repository?** `vdw_rung` is 49 KB of
   CNF and LRAT derived from the author's `MathRecords` work. It puts a real
   certificate of a published term under CI, which is the whole point of the
   project, but it couples two repositories' artefacts and its provenance line
   has to name the generator. The alternative leaves every vdW check to the
   differential harness, where nothing is committed. **Needed before step 2.**
2. **Is `RatLemmaIsRup` a rejection or an acceptance?** Strict is specified, on
   the same evidence and the same reasoning as part 1's `EarlyConflict`: real
   `drat-trim` output never does it. It is the only new rule with a plausible
   false-rejection risk against a different LRAT producer. The decision changes
   one branch; the question is whether Refute's stated audience is `drat-trim`
   output alone. *Not a blocker — strict is the fail-closed default, and the
   differential harness would expose a disagreement immediately.*
3. **`Limits::max_var` (2^26)** — unchanged from part 1's open question 2, and
   still milestone 4's to settle.

---

# Part 3 — milestone 2: direct DRAT

**Status:** draft · **Date:** 2026-08-14 · **Supersedes:** nothing in parts 1
and 2. The LRAT checker is not modified by this milestone, and the whole 79-test
suite must stay green at every commit in the build order.

Parts 1 and 2 check the file `drat-trim` writes. Part 3 checks the file the
solver writes, so that `drat-trim` is not in the chain at all — neither as
checker nor as producer. That is one new reader, one new clause store, one
propagation engine, one new rejection reason and one format decision, and it is
the largest single increase in what a bug in this repository could cost.

## The measurement, first

Same discipline as parts 1 and 2. Everything below was counted on 2026-08-14
with a throwaway reference checker, over the raw `kissat --no-binary` output for
nine instances — six generated by `tools/instances.py`, three built by the
author's certificate generator with symmetry breaking off. No number here is
read from a paper.

| proof | originals | additions | RUP | RAT | deletions | `.drat` bytes | peak live |
|---|---:|---:|---:|---:|---:|---:|---:|
| pigeonhole 5x4 | 45 | 91 | 71 | 20 | 75 | 2,148 | 61 |
| pigeonhole 6x5 | 81 | 206 | 176 | 30 | 164 | 6,368 | 123 |
| pigeonhole 7x6 | 133 | 702 | 630 | 72 | 487 | 22,055 | 348 |
| pigeonhole 8x7 | 204 | 3,366 | 3,240 | 126 | 2,319 | 152,641 | 1,251 |
| random 3-SAT 80/370 | 370 | 1,894 | 1,894 | 0 | 946 | 79,095 | 1,318 |
| random 3-SAT 100/460 | 460 | 763 | 763 | 0 | 148 | 21,329 | 1,075 |
| A217058 n=21, j=1 | 552 | 559 | 519 | 40 | 633 | 20,423 | 478 |
| A217058 n=25, j=2 | 755 | 2,388 | 2,328 | 60 | 1,695 | 111,029 | 1,448 |
| **A217058 n=33, j=4** | 1,249 | **31,195** | — | — | **26,988** | **2,508,578** | **10,400** |

The a(4) rung's RUP/RAT split is the one cell not measured: the reference
checker propagates naively and does not finish it in a sensible time, which is
itself a fact about why the real one needs watched literals. `drat-trim` reports
`195 RAT lemmas in core` for the same file.

### The nine structural facts the design rests on

Each was counted, not assumed. "0 of N" means the reference checker looked for
it in every step of every proof above and never found it.

| # | Fact | Evidence |
|---|---|---|
| G1 | **The pivot is the lemma's first literal, as written.** Every addition that is not RUP is RAT on it | 348 of 348 |
| G2 | **The RAT lines are the same lines `drat-trim` calls RAT.** The count of non-RUP additions in the raw proof equals the count of RAT-plus-empty-hint lines in `drat-trim -L`'s output of the same proof, and the candidate count equals its resolvent-block count, on every instance where part 2 measured both | 5x4 20/24, 6x5 30/45, 7x6 72/108, 8x7 126/196, n=21 40/48, n=25 60/72 |
| G3 | The proof contains a bare `0`. In every real proof measured it is the last line — **but that does not hold in general**, and the rule the checker relies on is the weaker one that does: the *first* empty clause ends the run, and nothing after it is read | 9 of 9 measured; falsified on 2026-08-14 by differential fuzz case 92 of seed 20260814, where `kissat` wrote two further additions after the empty clause |
| G4 | The empty clause is RUP where it stands. Nothing else could justify it: it has no pivot | 9 of 9 |
| G5 | **Duplicate live clauses occur.** The same clause is added while a copy is live, so deletion must remove exactly one | 39 on the a(4) rung, 4 on 8x7, 3 on random 80/370; largest multiplicity 3 |
| G6 | No deletion names a clause that is not live | 0 of 33,000-odd |
| G7 | **No deletion names a unit clause**, so honouring such a deletion — which `drat-trim` refuses to do — costs nothing measurable | 0 of 33,000-odd |
| G8 | Unit lemmas are rare: at most 87 in a proof, over at most one unit clause in the formula | a(4) rung |
| G9 | `kissat` writes the proof with the platform's line endings, so a proof generated on Windows is CRLF throughout | measured, and the reason `b08_crlf` exists on the LRAT side |

G5 is the one a checker gets wrong quietly. Deletion in DRAT names literals, not
an identifier; a store that keys clauses by their literal set and deletes "the"
clause deletes both copies, and the proof then fails later for a reason that
looks like a corrupt certificate.

G7 and G8 together are why this design has no persistent root-level trail; see
the decision below.

G2 is the strongest evidence available that the two milestones are checking the
same mathematics by different routes, and it is free: the corpus already holds
`<name>.lrat` for several instances, so committing `<name>.drat` beside it gives
CI a differential test between Refute's two checkers that needs no binaries.

## Format detection, normative

The CLI takes a proof path and no format. A user who has both files in a
directory will hand over the wrong one, and a checker that reads an LRAT file as
DRAT — or the reverse — must not do so silently.

**Detection is exact, not heuristic**, because the two grammars are disjoint on
a per-line basis:

```
DRAT step  := "d" lit* 0  |  lit* 0            ; exactly one 0, and it ends the line
LRAT step  := id "d" id* 0  |  id lit* 0 id* 0 ; an addition has two 0-terminated groups
```

An LRAT addition needs two terminators; a DRAT addition permits one and rejects
anything after it, because both readers are line-oriented for part 1's reason.
An LRAT deletion's second token is `d`; a DRAT deletion's first token is. An
LRAT identifier is strictly positive; a DRAT line commonly starts with a
negative literal — `-154 0` is the first line of the a(4) rung. So no line is
accepted by both grammars.

Measured, rather than argued: over the 49 committed `.lrat` proofs and the 9
`.drat` proofs above, **no proof's first step is accepted by the other
grammar**, and no proof's first step is accepted by both. Four `.lrat` fixtures
are accepted by neither, and each is a case the rule below already handles: two
are empty files, one is the binary-proof fixture, and one is
`hostile_escape_proof`, whose first line is a terminal escape sequence.

```
detect(first 1024 bytes of the proof):
  if looks_binary            -> Unsupported(BinaryProof { line: 1 })
  take the first non-empty line within the peeked bytes
  match (LRAT accepts it, DRAT accepts it):
    (true,  false) -> Lrat
    (false, true)  -> Drat
    _              -> Lrat        ; neither, no line at all, or (unobserved) both
```

The last arm is the one that matters, and it is chosen so that **nothing about
milestone 1's behaviour changes**. A file neither grammar accepts is read by the
incumbent LRAT reader, which reports exactly the parse error it reports today —
so `hostile_escape_proof` keeps its reason and its escaped message, an empty
file keeps `NoEmptyClause`, and no existing test moves. A file both grammars
accepted would go the same way; none exists, so no reason code is invented for
it, on part 1's rule that a verdict nothing can produce is decoration.

**Mis-routing cannot produce a false `VERIFIED`.** Each checker is sound for its
own grammar: if the DRAT checker verifies a file, that file *read as DRAT* is a
valid refutation of the formula, whatever its author meant it to be. The cost of
a wrong guess is a confusing rejection, never a wrong acceptance. That is why
detection is allowed to have a default arm at all.

**Never the file extension.** An extension is a claim by whoever named the file,
and this project's whole posture is that a claim is not evidence.

`--drat` and `--lrat` skip detection entirely. Nothing in the library dispatches
on a path; `check_readers` peeks, and `check_readers_with_format` does not.

### The binary sniff, widened

Part 2's rule — first byte `a` (0x61) or `d` (0x64) means binary — was written
when the only text format was LRAT, whose lines never begin with either. **A
text DRAT deletion line begins `d `.** Left alone, the rule would report a
perfectly good text DRAT proof as binary whenever its first line is a deletion.

```
looks_binary := a 0x00 byte occurs in the first 1024 bytes
             OR (byte 0 is 0x61 or 0x64 AND byte 1 is not 0x20 or 0x09)
```

Binary DRAT terminates every record with 0x00 and text proofs contain none, so
the first clause is decisive and the second is a cheap belt on short files. Both
can only produce `Unsupported`, which has no route to `Verified`; the residual
failure — a binary proof whose first record is longer than 1024 bytes and whose
second byte happens to be a space — is a worse message, not a worse verdict.
`b17_binary_proof` satisfies both clauses and keeps its verdict.

## The DRAT step, normative

```
check_add(lits):
  ; the trail is empty here: every step unwinds itself completely
  if assume_negated(lits): accept                  ; tautology, as in parts 1 and 2
  if propagate() == Conflict: accept               ; RUP
  if lits.is_empty(): unwind(0); reject NoConflict ; the empty clause has no pivot
  pivot = lits[0]                                  ; FILE order; see part 2's trap
  base = trail.len()                               ; negated lemma + everything RUP implied
  for cand in candidates(-pivot):                  ; every live clause holding it
      refuted = false
      for l in clause(cand):
          if l == -pivot: continue                 ; resolved away
          match value(l):
              TRUE  -> refuted = true; break       ; the resolvent refutes itself
              FALSE -> continue
              UNSET -> assign(-l)
      if !refuted: refuted = (propagate() == Conflict)
      unwind(base)                                 ; EVERY candidate starts from base
      if !refuted: unwind(0); reject RatCheckFailed{pivot} @ cand
  unwind(0); accept
```

Then, and only then: the lemma enters the database; if it is empty, the run is
over and the verdict is `Verified`.

It is deliberately the same program as part 2's `check_rat` with the file's
claims deleted. There, the producer named the candidates and the checker
verified that the naming was exactly right; here there is no naming, so the
enumeration *is* the check. Every strictness rule part 2 needed to police a
hint list — `MissingResolvent`, `NotAResolutionCandidate`,
`ResolventFalsifiedEarly`, `RatLemmaIsRup` — has no counterpart here, because
there is nothing to police. That is a real simplification and worth saying: the
DRAT checker has **one** new rejection reason, and the four strictness rules
that carry a false-rejection risk against another producer do not exist on this
path at all.

Four things in that pseudocode are load-bearing:

- **`unwind(base)` after every candidate.** Milestone 1b shipped the same line
  with no test pinning it, and deleting it left 77 tests green while the checker
  printed `s VERIFIED` on a formula `kissat` reports satisfiable. Here it is
  worse, because there is no file to disagree with. Its fixture is written
  first, in step 8 of the build order, before the rule it pins.
- **`base` includes the failed RUP propagation.** It is part 2's F6 by another
  name: the candidate's resolvent is checked from the negated lemma plus
  everything unit propagation already derived from it. Dropping it reads
  perfectly well and rejects every real proof.
- **The candidate loop has no early exit.** No `break` on the first success, no
  skipping of candidates whose resolvent is trivially refuted — the loop must
  visit every live clause holding the negated pivot, because RAT is a claim
  about all of them.
- **The empty clause is checked, not accepted.** `0` on a line is the one step
  that cannot be RAT, so if propagation does not conflict it is a rejection.
  A checker that treats the last line as a formality accepts every file that
  ends in `0`, which is every file.

### Why it is sound

Unchanged from part 2 and repeated because this is where it is now enforced:
a clause `C` is RAT on pivot `p` in `C` with respect to `F` when for every `D`
in `F` holding `-p`, the resolvent `C or (D \ {-p})` is implied by unit
propagation. Adding such a `C` preserves satisfiability. `F` is the live
database, and a superset of an unsatisfiable set is unsatisfiable, so checking
against the smaller live formula still refutes the larger one.

The three places to lose it are part 2's three, with one changed:

1. **The candidate set must be complete** — and it is now enumerated from an
   occurrence index rather than a scan, which moves the failure mode from "the
   file lied" to "the index is stale". That is the reason the index is a
   measured decision below rather than a convenience, and the reason deletion
   maintains it eagerly.
2. **The vacuous case must be proved, not assumed.** A lemma whose pivot has no
   live candidate is accepted after the enumeration returns nothing — the same
   code, the same loop, zero iterations. There is no separate path for it and
   therefore nothing for the two to drift apart on.
3. **Propagation must derive only what is implied.** New in this milestone, and
   the one with no analogue in parts 1 and 2, where the producer's hints made
   every propagation a named clause. A watched-literal bug that assigns a
   literal no clause forces makes conflicts easier to reach and every check
   easier to pass. It is the reason the fuzz harness's satisfiable-formula class
   is unconditional: a formula with a model has no refutation, so any
   `s VERIFIED` on one is a defect, whatever the mutation was.

### Strictness decisions, and what each costs

| Rule | Alternative | Why |
|---|---|---|
| A deletion removes exactly one copy of a duplicated clause | Remove every copy | G5: duplicates are real. Removing all copies is a false rejection later; removing none makes deletion a no-op and hides a deleted-then-used mutation |
| A deletion naming no live clause is counted, not rejected | Reject | Part 1's rule, unchanged and for the same reason: deletion only removes tools from the checker, so it can cause a later rejection but never a false `VERIFIED` |
| A deletion naming a unit clause is honoured | Ignore it, as `drat-trim` does | Refute is the stricter of the two, as it already is on the LRAT side. G7 measures the cost at zero, and the permissive rule exists to protect backward checking, which this checker does not do |
| A comment or any non-integer token in a proof line is a parse error | Skip `c` lines | Measured at zero occurrences; `kissat` writes none. Fail closed on a file nobody has been observed to write, and say so in the README |
| Anything after the `0` that ends a step is a parse error | Whitespace-delimited parsing across lines | Part 1's rule. It is also half of what makes the two grammars disjoint, so relaxing it would weaken format detection as well |
| A proof that continues after the empty clause is not read | Read to EOF | Part 1's rule. The run is over; nothing after it can change a verdict |

## Data model

No database, no migrations. Parts 1 and 2's table stands for the LRAT path. The
DRAT path adds one structure, which is **not shared with the LRAT checker**.

| Structure | Type | Notes |
|---|---|---|
| `Store::lits` | `Vec<Lit>` | One arena for every live clause's literals. 626,008 entries at the a(4) rung's peak, 2.5 MB |
| `Store::clauses` | `Vec<ClauseMeta>` | `{ start: u32, len: u32, live: bool }`, indexed by the checker's own identifier. Identifiers are dense and strictly increasing: originals are `1..n` in file order, lemmas continue from `n+1` in proof order, which is exactly the LRAT numbering a reader already understands |
| `Store::watches` | `Vec<Vec<u32>>` by literal code | Two per clause of length >= 2 |
| `Store::occ` | `Vec<Vec<u32>>` by literal code | Every clause, every literal. The RAT candidate index; see the decision below |
| `Store::bykey` | `HashMap<Box<[Lit]>, Vec<u32>>` | Sorted, deduplicated literals to live identifiers, **a list and not a single value**, because of G5. Queried with `&[Lit]`, which `Box<[Lit]>: Borrow<[Lit]>` allows without allocating |
| `Store::units` | `Vec<u32>` | Live clauses of length 1, enqueued at the start of every propagation. A one-literal clause cannot hold two watches |
| `Store::empties` | `usize` | Live clauses of length 0. Non-zero means propagation conflicts immediately, which is how a formula containing a bare `0` is refuted in one step — part 1's `empty_clause_in_cnf`, on the DRAT path |
| trail, assignment | as part 1 | Reused verbatim in shape: `Vec<u8>` by variable, `Vec<u32>` of assigned variables, unwound in proportion to what was assigned |

**Every per-variable and per-literal vector is sized from the formula's largest
variable and grown on demand.** Never from the `p` line, never from
`Limits::max_var`. Part 1 learned this on the assignment vector, where nineteen
bytes of header bought a 64 MB allocation; there are now three more vectors with
the same exposure, and `watches` and `occ` are vectors of vectors, so the
mistake would cost 24 bytes per literal code rather than one.

**Clauses enter and leave the database only while the trail is empty.** That is
what makes the watched-literal invariant trivially true at insertion, and it is
checkable: `assignments == assignments_undone` at the end of every run, asserted
on every positive fixture exactly as part 2 asserts it.

### The occurrence-index decision, and why it goes the other way here

Part 2 measured this and chose the scan. The measurement is different on the
DRAT path and the decision flips, which is worth being explicit about rather
than quietly inconsistent.

| proof | RAT lines | mean live clauses | A: clauses scanned | B: index slot updates |
|---|---:|---:|---:|---:|
| pigeonhole 8x7 | 126 | 666 | 83,857 | 40,096 |
| A217058 n=25, j=2 | 60 | 844 | 50,618 | 26,674 |
| A217058 n=33, j=4 | 195+ | 6,717 | 1,309,853+ | 626,008 |

**B, by measurement**, and by a factor that grows with the proof. The reason is
G2's other half: `drat-trim`'s LRAT deletes far harder than the raw proof does,
because trimming discards whole lemmas. Checking the LRAT of pigeonhole 8x7, the
live database averages about 159 clauses; checking the raw DRAT of the same
instance it averages 666, and on the a(4) rung it averages 6,717 and peaks at
10,400. The scan is priced per RAT line times live clauses; the index is priced
per literal ever inserted or deleted, which the file's own size bounds.

The a(4) rung's scan figure is a lower bound: 195 is `drat-trim`'s count of RAT
lemmas *in the core*, and forward checking sees every lemma, so the real scan
cost is larger and the index looks better still.

The mitigations are part 2's, unchanged, because the whole decision again lives
behind one function:

- `fn resolution_candidates(&mut self, pivot: Lit) -> &[u32]` is the only place
  that knows how candidates are found; swapping the scan back in changes that
  function and nothing else, with the same tests;
- `--stats` reports `occurrence_updates` and `candidates_examined`, so the bet
  is observable on a reader's own proof;
- the trigger is written down now: if `occurrence_updates` exceeds the scan cost
  the same run reports — RAT lines times mean live clauses — on a real proof,
  go back to the scan.

### No persistent root-level trail

The obvious optimisation is to keep the assignments forced by unit clauses
across steps instead of re-deriving them every time. `drat-trim` does exactly
that, and it is why `drat-trim` ignores deletions of unit clauses: a kept trail
is only sound while nothing retracts what is in it, so it needs a reason clause
per assigned literal and a backtrack whenever a reason is deleted.

Refute does not, on measurement rather than taste. G8: the a(4) rung's formula
has one unit clause and its proof adds 87 more over 31,195 steps, so propagating
from scratch re-does at most 88 assignments per step. The saving is small, the
machinery is a reason array plus a retraction path, and **the unsound version of
it — keeping the trail without tracking reasons — is a false `VERIFIED` waiting
for a proof that deletes the clause that forced a unit.** Milestone 3 may
revisit it with a measurement on the 200 MB rung; it must not be adopted without
one.

## Interfaces

```rust
// format.rs
pub enum Format { Lrat, Drat }
/// Pure function over peeked bytes. No I/O, no allocation, trivially testable.
pub fn detect(head: &[u8]) -> Result<Format, Unsupported>;

// drat.rs — streaming; never holds the file
pub enum DratStep {
    Add    { lits: Vec<Lit>, line: u64 },
    Delete { lits: Vec<Lit>, line: u64 },
}
pub struct DratReader<R: BufRead> { /* ... */ }
impl<R: BufRead> Iterator for DratReader<R> { type Item = Result<DratStep, ParseError>; }

// verdict.rs
/// Evidence that a checked step derived the empty clause.
///
/// No `Default`, no `Clone`, no public constructor, and one field of `()` so it
/// cannot be built by a struct literal outside the module that defines it.
pub(crate) struct EmptyClauseDerived(());
/// The one function in the library that produces a `Verdict::Verified`.
pub(crate) fn verified(_: EmptyClauseDerived) -> Verdict;

pub enum Reason { /* ... existing ... */
    /// A candidate clause holding the negated pivot whose resolvent with the
    /// lemma is not implied by unit propagation. `resolvent` names the
    /// candidate, in the checker's own numbering.
    RatCheckFailed { pivot: Lit },
}

// checker.rs / drat/checker.rs
pub fn check_readers<F: BufRead, P: BufRead>(f: F, p: P, l: &Limits) -> Outcome;   // detects
pub fn check_readers_with_format<F: BufRead, P: BufRead>(
    f: F, p: P, l: &Limits, format: Format) -> Outcome;                            // does not
pub struct Outcome { pub verdict: Verdict, pub warnings: Vec<Warning>,
                     pub stats: Stats, pub format: Format }                        // one new field
```

`check_readers` keeps its signature and its contract: total, a verdict for every
input, no panic, no unbounded allocation, no read past the first failing step.

New counters, on the same terms as part 2's — they exist to be asserted exactly:

```rust
propagations: u64,            // literals assigned by propagation
watch_visits: u64,            // clauses inspected in watch lists
occurrence_updates: u64,      // index slots written or cleared: the bet, made countable
rup_additions: u64,
tautological_additions: u64,
rat_candidates_checked: u64,
```

`rup_additions + rat_additions + tautological_additions == additions` on every
run, and `assignments == assignments_undone` at the end of every run. `Stats` is
one flat struct shared by both checkers; the fields the other checker does not
use stay zero, and the CLI prints the DRAT line only when the DRAT checker ran.

### The trust boundary, restated for two checkers

Part 1's property is unchanged: **`s VERIFIED` is printed only when a checked
sequence of steps derives the empty clause from the parsed formula.** Its
enforcement changes shape, because there are now two checkers with a legitimate
claim to have derived the empty clause.

- `Verdict::Verified` is constructed at **one** site in the library, in
  `verdict.rs`, inside `fn verified(EmptyClauseDerived) -> Verdict`.
- `EmptyClauseDerived` is constructed at **two** sites, one per checker, each
  immediately after the step that added the empty clause returned `Ok`.
- `tests/trust_boundary.rs` asserts both counts and both file names, and keeps
  its existing assertions that the variant is never imported, has no `Default`
  and no `From`, and that the CLI's match has no wildcard arm.

This is the one place where a milestone-1 test is deliberately changed rather
than added to, so it happens in a commit of its own, with the reasoning in the
message: the guard gets strictly stronger — it now counts two things instead of
one — and the alternative, letting `Verdict::Verified` appear twice, is exactly
the drift the guard was written to catch.

## Access control

Unchanged: no database, no accounts, no network, no stored state. The untrusted
input table stands, with three rows added.

| Attack | Vector | Control |
|---|---|---|
| Unbounded allocation | A proof line naming variable 2^31, sizing three per-literal vectors of vectors at 24 bytes each | Every such vector is sized from the formula and grown on demand, and every literal is bounded by `Limits::max_var` at parse time. The parser refuses the literal; nothing resizes to meet it |
| Memory growth | A DRAT proof that only ever adds | Inherent and bounded by the file's own length: the store holds one arena entry per literal read. `--stats` reports peak live clauses, as it does today. The a(4) rung peaks at 10,400 clauses and 626,008 literals |
| Quadratic blow-up | A proof whose every line is RAT against a database that never shrinks | The index makes candidate lookup proportional to the candidate count rather than the database size, which is the mitigation; the resolvent checks themselves remain proportional to propagation. `occurrence_updates` makes it visible |

## Migrations

None; there is no database. The equivalents table gains three rows.

| # | Change | Reversible? | Rollback |
|---|---|---|---|
| 5 | `Outcome` gains `format`; `Reason` gains `RatCheckFailed`; `Verdict::Verified` moves behind `verdict::verified` | Yes | `git revert`. A 0.1.0 crate with no dependants. The CLI's exit codes and verdict strings do not move, so no consumer contract does |
| 6 | `<name>.drat` fixtures land beside the existing `<name>.cnf` / `<name>.lrat` pairs | Yes | `git revert`; regenerable by `tools/gen_fixtures.sh` given the two binaries |
| 7 | `refute` accepts `--drat`, `--lrat`, and an optional leading `check` verb | Yes | `git revert`. All three are additive: every command line that works today works unchanged and means the same thing |

**The `check` verb.** `refute check <cnf> <proof>` is accepted when, and only
when, there are exactly three positional arguments and the first is `check`.
`refute <cnf> <proof>` is unchanged, `--` still ends the flags, and a file
genuinely called `check` is reachable as `refute -- check b.drat` or by any path
with a separator in it. This is open question 2; the compatible form is what is
specified, and deleting it is one commit.

## Failure modes

Parts 1 and 2's tables stand. What part 3 adds:

| What breaks | Who notices | How we detect it | How we undo it |
|---|---|---|---|
| **False `VERIFIED` from a propagation bug** — the new serious one, and the first in this project with no producer-supplied hint to contradict it | Nobody, for months | The satisfiable-formula class of `tools/fuzz.py` is unconditional: a formula with a model has no refutation. Every committed hand-built negative is over a formula `kissat` reports satisfiable, as `r09`–`r11` already are | Revert; withdraw any claim citing Refute in the same session |
| False `VERIFIED` from a stale occurrence index | Nobody | A candidate missed is a RAT condition never checked. `d02` covers a dropped candidate; the index is maintained eagerly on delete so there is no lazy path to get wrong | As above |
| False `VERIFIED` from the trail leaking between candidates | Nobody | `d09`, hand-built over a satisfiable formula, written before the rule. This is the milestone-1b hole, reproduced deliberately | As above |
| Deletion removing every copy of a duplicated clause | The author, on a real certificate | `d10`; and the a(4) rung would fail outright, since 39 of its additions duplicate a live clause | Fix the store; the rule is one line in `bykey` |
| False rejection because propagation is incomplete | The author, immediately | Every positive fixture; the differential harness | Fix; no strictness rule to relax, because this path has none |
| An LRAT file checked as DRAT, or the reverse | A user with two files in one directory | Detection is exact and measured on the whole corpus; forcing the wrong format with `--drat` / `--lrat` is a committed boundary test | Fix detection. Note the verdict is never wrong, only the message |
| A binary DRAT proof reported as a text parse error | A user who forgot `--no-binary` | Widened sniff; `b17` and the new deletion-first fixture cover both clauses | — |
| The DRAT checker is slower than a person will wait | Anyone checking a real certificate | The a(4) rung is the gate: `drat-trim -f` does it in 0.589 s, `drat-trim` backward in 0.377 s, and Refute's time is reported beside them. A ratio above 50x is a finding to investigate before the README claims anything about scale | Milestone 3 owns scale. The two candidates are already written down: the root trail with reason tracking, and a cheaper propagation loop |

## Rollback

`git revert` plus `cargo build`, under a minute. No database, no deployment, no
persistent state, no consumer contract broken: exit codes and verdict strings do
not move, and every command line that works today works afterwards.

The one irreversible act remains **publishing a claim**, and this milestone's
claim is the largest the project has made — that a published upper bound has
been re-checked with `drat-trim` out of the chain. The order is hard:

1. the suite green, including every new control, on both toolchains;
2. the mutation-kill pass complete, with a named test dying for every new
   rejection rule;
3. `tools/differential.sh` run locally, Refute against `drat-trim -f`, on the
   pigeonhole ladder, the random instances and the vdW rungs, its table in the
   commit;
4. `tools/fuzz.py` run to 10,000 cases with zero false accepts, its summary in
   the commit;
5. *then* the README's opening rewritten;
6. any claim about the author's certificates cites that differential run and
   names the rung.

Rewriting the README before step 4 is the one thing in this milestone that
cannot be taken back.

## Test plan

Framework unchanged: `cargo test`, no test dependencies, committed fixtures, CI
with neither binary.

### What "red first" means here, and what it does not

Every DRAT fixture handed to the milestone-1b binary is routed to the LRAT
reader and rejected with a parse error. So:

- **The positives are genuinely red.** `P13`–`P18` demand `Verified` and get a
  rejection.
- **The negatives are green for the wrong reason** if they only assert "not
  verified". Every negative therefore asserts its **exact reason**, which today
  is a parse error and after the milestone is `RatCheckFailed`, `NoConflict` or
  `NoEmptyClause`. Written that way they are red as well, and the commit that
  records the red says which kind each one is.
- **Where a rule cannot be pinned by a red test written first**, part 2's weaker
  evidence applies and is labelled as such: revert the rule's line, run the
  suite, record which test died. The mutation-kill table below is mandatory
  content of the milestone, not a nice-to-have — it exists because three rules
  shipped unpinned in 1b and the suite was green.

### Corpus additions

About 45 KB against a 500 KB budget standing at 372 KB.

| Fixture | Origin | Size | Why |
|---|---|---|---|
| `tiny_unsat.drat` | `kissat --no-binary` on the existing formula | ~0.2 KB | The end-to-end happy path, and half of the two-checker agreement test |
| `deletes_originals.drat` | ditto, pigeonhole 4x3 | ~1 KB | Deletions of original clauses on the DRAT path |
| `real_rat_proof.drat` | ditto, pigeonhole 5x4 | 2.1 KB | 91 additions, 71 RUP, 20 RAT, 24 candidates, 75 deletions, peak 61. The smallest real proof carrying RAT |
| `rat_pigeonhole.drat` | ditto, pigeonhole 7x6 | 22 KB | 702 additions, 630 RUP, 72 RAT, 108 candidates, 487 deletions, peak 348. Scale: a subtly over-strict checker passes 5x4 |
| `empty_clause_in_cnf.drat` | hand-built, one line: `0` | 2 B | The formula is already refuted; the store's `empties` path |
| `d01`–`d08` | deterministic mutations of `real_rat_proof.drat` by `tools/mutate.py`, one per class the fuzz harness generates | ~16 KB | The classes, pinned in CI where the fuzzer does not run |
| `d09_trail_leak_between_candidates` | hand-built over a **satisfiable** formula | < 1 KB | The rule 1b shipped unpinned. `s VERIFIED` here is a false accept against a formula with a model |
| `d10_duplicate_clause_deleted_once` | hand-built over a **satisfiable** formula | < 1 KB | G5. A store that deletes both copies, or neither, verifies this |
| `b29_deletion_first.drat` | a real proof with its first addition removed so a `d` line leads | ~2 KB | The binary sniff's widened rule. Under part 2's rule this file is reported binary |
| `b30_crlf.drat` | `kissat` output from a Windows run, CRLF preserved | ~0.2 KB | G9, and the CI guard that greps for CR gains a second file |

Pigeonhole 8x7 (152 KB) and the vdW rungs stay out of CI and are covered by the
differential harness, as in part 2. `.gitattributes` already marks the fixture
directory `-text`; the generator writes LF for every fixture except `b30_crlf`,
whose bytes are the point of it, so a re-run on either platform is
byte-identical.

### Positive — must return `Verified`, exit 0

| # | Fixture | Asserts |
|---|---|---|
| P13 | `tiny_unsat.drat` | Verifies; exact counters |
| P14 | `real_rat_proof.drat` | 91 additions, 71 RUP, 20 RAT, 24 candidates checked, 75 deletions, 0 unknown deletions, peak 61 |
| P15 | `rat_pigeonhole.drat` | 702 additions, 630 RUP, 72 RAT, 108 candidates, 487 deletions, peak 348 |
| P16 | **`<name>.cnf` verifies under both `<name>.lrat` and `<name>.drat`** | The two checkers agree, in CI, on committed bytes, with no binaries. Run for every name that has both |
| P17 | `deletes_originals.drat` | Deletion on the DRAT path |
| P18 | `empty_clause_in_cnf.drat` | A one-line proof of a formula that already holds the empty clause |
| all | every positive | `assignments == assignments_undone`; `rup + rat + tautological == additions` |

### Negative — must not print `s VERIFIED`; exit non-zero, exact reason asserted

| # | Mutation of `real_rat_proof.drat` | Expected |
|---|---|---|
| D1 | One addition line dropped | `RatCheckFailed` or `NoConflict` — whichever the reference run produces, asserted exactly |
| D2 | A later addition's candidate set changed by dropping the clause a RAT step needs | `RatCheckFailed`, naming the candidate |
| D3 | One literal of one addition flipped | rejection, exact reason recorded |
| D4 | Two additions swapped | rejection, exact reason recorded |
| D5 | A `d` line inserted for a clause a later step needs | rejection, exact reason recorded |
| D6 | Truncated before the last line | `NoEmptyClause` |
| D7 | The final `0` line removed | `NoEmptyClause` |
| D8 | Checked against a **satisfiable** formula | rejection — the control that matters most; a pipeline that passes here certifies a false upper bound |
| D9 | `d09_trail_leak_between_candidates` | rejection; the formula has a model |
| D10 | `d10_duplicate_clause_deleted_once` | rejection; the formula has a model |

D1 to D5 are the classes `tools/fuzz.py` generates. They are committed as fixed
mutants as well so that CI, which has no binaries, still exercises each class on
every commit.

### Boundary

| # | Input | Expected |
|---|---|---|
| B23 | Empty proof file, auto-detected | `NotVerified(NoEmptyClause)`, exit 1 — **identical to B1**, and the test says so: with no first line the format is unobservable, and both readings agree |
| B24 | A `d` line naming a clause never present | Accepted, counted in `--stats` |
| B25 | A `d` line naming a live **unit** clause | Honoured; a later step that needs it is rejected. The documented difference from `drat-trim` |
| B26 | A DRAT line longer than `max_line_bytes` | `ParseError(LineTooLong)`, no allocation, no panic |
| B27 | A DRAT addition repeating a literal (`1 1 3`) | Verifies; the pivot is the first literal and the repeat is idempotent |
| B28 | A DRAT proof whose first line is `d ...` | Detected as DRAT, **not** as binary |
| B29 | A CRLF DRAT proof | Parses |
| B30 | `b17_binary_proof` under the widened sniff | `Unsupported(BinaryProof { line: 1 })`, exit 2, unchanged |
| B31 | A `.drat` file forced with `--lrat` | Rejected with an LRAT parse error, exit 1, asserted **not** exit 0 |
| B32 | A `.lrat` file forced with `--drat` | Rejected, exit 1, asserted **not** exit 0 |
| B33 | `hostile_escape_proof.lrat`, auto-detected | **Unchanged verdict, unchanged reason, unchanged escaped message.** The regression guard on detection's default arm |
| B34 | A DRAT proof with a `c` comment line | `ParseError`, per the strictness table |
| B35 | A lemma naming a variable the formula never mentions | The per-variable vectors grow; verdict unaffected |
| B1–B22 | existing | Unchanged |

### The mutation-kill table

Mandatory. For each rule: the change to make in the source, and the test that
must fail when it is made. A rule with no test that dies is an untested rule,
and the fixture for it is written in the same step, not later.

| Rule | Source mutation | Test that must die | **Measured 2026-08-14** |
|---|---|---|---|
| The trail is unwound to `base` between candidates | delete `unwind(base)` | D9 | D9, D10 |
| Every candidate is checked | `break` out of the candidate loop after the first | D2 | D2, D9, D10, P14, P15 |
| Candidates that are trivially refuted still count | drop the `rat_candidates_checked` bump | D2 | **P14, P15 only.** The prediction was wrong, and so was its framing: skipping a trivially refuted candidate does not change a verdict, only a counter, so this row is not a safety control and the exact-counter assertions on the two positives are the whole of it |
| Candidates come from the live database only | stop maintaining `occ` on delete | D5 | D5, D1, D2, two store unit tests. A second mutation — make `delete` a no-op — kills twelve, including B25 |
| A deletion removes exactly one copy | let the store hold only one clause per literal set | D10 | D10, P15, one store unit test. **P15 dying is evidence for G5 on a committed fixture**: the raw 7x6 proof really does duplicate a live clause |
| The empty clause must be RUP | accept a bare `0` without propagating | D8 | **B25, and P13–P17.** D8 survives, because that proof fails at a RAT step long before its empty clause. B25 is the real kill and it is a false accept: the mutant verifies a formula with a model. P13–P17 die on `rup + rat + tautological == additions`, which is the identity earning its place |
| EOF without an empty clause is a rejection | return the verdict of the last step at EOF | D7 | D6, D7, **and the trust boundary**, which counted a third witness site. The guard fired at a mutation it was not written for |
| The lemma's negation is fully assumed | assume one fewer literal | P14 (false rejection; recorded as the direction it fails in) | ten tests, including D1, D2, D5, D6, D7 and D8 — so it is *not* purely a false-rejection mutation, and the row understated it |
| Unit clauses propagate | drop the `units` enqueue | P13 (false rejection) | fourteen tests, including B24, B27, B29, B35 and a store unit test |
| Detection routes by grammar | force `Format::Drat` | B32, B33 | 61 tests |
| The binary sniff does not swallow a leading `d` line | restore part 2's rule | B28 | B28, B24, B25, D10, two `format` unit tests |

**No mutation survived.** Twelve applied, twelve killed, and the three rows
whose predicted victim was wrong are corrected above rather than quietly
re-aimed.

Two of those mutations fail in the *false rejection* direction, and the table
says so. A control that only ever fires in that direction is a weaker control,
and the honest thing is to label it rather than to present the whole table as if
every row guarded against a false accept.

### CLI-level

Unchanged contract, run against the built binary. New assertions: a `.drat` pair
exits 0 and prints `s VERIFIED`; `--drat` and `--lrat` are accepted and are not
usage errors; `refute check a.cnf b.drat` behaves exactly as `refute a.cnf
b.drat`; `--stats` prints the DRAT counter line only for a DRAT run; a
`RatCheckFailed` rejection's stderr names the pivot and the candidate.

### Differential harness (not CI)

`tools/differential.sh` gains a DRAT column. For each instance it already runs
solver then `drat-trim -L` then `refute`; it now also runs **`drat-trim -f`** on
the raw `.drat` and `refute` on the same file, and prints both pairs of
verdicts. `--extra <dir>` learns one rule: if a `.drat` sits beside a `.cnf` in
that directory, use it rather than re-solving, which is exactly the shape the
author's certificate generator writes with its `--keep` option.

**`-f`, not the default.** Backward mode is not a valid oracle for a forward
checker: it only checks the lemmas it keeps. Measured on 24 single-literal
mutants of two real proofs, one is `s VERIFIED` backward and `s NOT VERIFIED`
forward. Comparing against backward mode manufactures disagreements that are
nobody's bug and hides the ones that are.

Required to pass before the README changes: the pigeonhole ladder to 8x7, three
random 3-SAT refutations, and the A217058 rungs at n=21, n=25 and **n=33**, the
last being the 2.5 MB raw certificate behind a published term.

### The fuzz harness

`tools/fuzz.py`, Python 3 and the standard library, beside `instances.py` and
`mutate.py`. Locations of the binaries by `$KISSAT` / `$DRAT_TRIM` / `$REFUTE`
or by flag, never in a tracked file. Deterministic: `--seed S --cases N`, and
every case reproducible on its own with `--case K`.

Per case: generate a small random instance (3-SAT and 4-SAT, 6 to 24 variables,
clause ratio 3.6 to 5.2, with every seventeenth case a small pigeonhole and
every twenty-third carrying a deliberate duplicate clause and a unit clause);
solve; on UNSAT take the raw `.drat`, on SAT keep the formula for the classes
that need one. Then compare Refute with `drat-trim -f` on the clean proof and on
one mutant per class.

Classes: **dropped line, reordered lines, flipped literal, deleted-then-used
clause, truncated proof, missing empty clause**, plus **wrong formula** and
**satisfiable formula** from milestone 1's controls.

The verdict rules, which are the whole design of the harness:

| Refute | `drat-trim -f` | Meaning |
|---|---|---|
| `VERIFIED` | `NOT VERIFIED` | **Hard failure.** A false accept. Keep the artefacts, stop the run |
| `NOT VERIFIED` | `VERIFIED` | Refute is stricter. Allowed only if its reason is on the documented strict list, and counted either way; anything else is a hard failure |
| same | same | Pass |

and two classes are unconditional, because for them rejection is a theorem
rather than an observation: **missing empty clause** and **truncated**, where
nothing was derived, and **satisfiable formula**, where a model exists. In those
three, `s VERIFIED` is a hard failure whatever `drat-trim` says.

**The harness does not assert that every mutant is rejected**, and this is the
correction that matters most in this document. Measured on 24 single-literal
mutations of two real proofs, **5 remain valid proofs** and `drat-trim -f`
verifies them: the flipped literal landed in a lemma nothing later depends on.
An assertion that every mutant is rejected would be red on correct behaviour,
and would be weakened by whoever hit it first. The harness counts harmless
mutants and reports the rate; a rate that suddenly goes to zero is itself a
signal that the mutator has stopped mutating.

The gate is the PRD's: 10,000 cases, zero false accepts, every strictness
divergence attributable to a named rule.

## Build order

1. Branch `design/milestone-2`. Documents only: this part, the PRD's milestone-2
   section, the App Flow delta. Commit.
2. `tools/gen_fixtures.sh`: emit `<name>.drat` for the existing instances,
   normalised to LF, plus the CRLF fixture and the deletion-first fixture.
   `tools/mutate.py`: the ten `d` fixtures. Commit the fixtures. Fixtures before
   tests, for part 1's reason.
3. Write P13–P18, D1–D10 and B23–B35 against the **milestone-1b** binary. Run.
   Paste the failing output into the commit message, marking which are red
   because the format is unsupported and which because the reason differs.
   Commit red. *This commit is the evidence for the milestone.*
4. `src/format.rs`: `detect`, the widened binary sniff, `--drat` / `--lrat`, the
   optional `check` verb. B28, B31, B32, B33 go green; DRAT files now route to a
   reader that does not exist, so this commit lands with a stub that rejects.
5. `src/drat.rs`: the reader. B26, B29, B34 go green.
6. `src/verdict.rs`: `EmptyClauseDerived`, `verdict::verified`,
   `Reason::RatCheckFailed`, and the trust-boundary test's new counts. **On its
   own**, with the reasoning in the message, and the whole existing suite green
   at this commit.
7. `src/drat/store.rs`: arena, watches, occurrence index, `bykey`, units,
   empties. Unit tests on the store alone: insert, delete one of two copies,
   candidate enumeration, growth past the formula's largest variable.
8. `src/drat/checker.rs`: propagation, then RUP, then the candidate loop — one
   rule at a time, **D9 first**, because the trail leak is the rule this project
   has already shipped unpinned once. P13–P18 and D1–D10 go green.
9. CLI: the DRAT `--stats` line, the pivot and candidate in the rejection
   message. The CLI tests go green.
10. Full suite on stable and on 1.74.0, locally, both profiles.
11. The mutation-kill pass, every row of the table, output recorded.
12. `tools/differential.sh` with the DRAT column; run it, including the vdW
    rungs by `--extra`; paste the table.
13. `tools/fuzz.py`; run 10,000 cases; paste the summary.
14. **Only now:** the README's opening, the fixture README's provenance rows,
    `SESSION_HANDOFF.md`.
15. Push the branch. CI green on all five jobs. Stop; merging is the owner's.

## Open questions

1. **Does a real vdW certificate belong in the committed corpus, in DRAT form?**
   PRD milestone-2 question 1. The n=21 rung is 20 KB of proof and 7 KB of
   formula, and it would put a published term under CI with `drat-trim` out of
   the chain — which is the project's whole purpose — at the cost of coupling
   two of the author's repositories' artefacts. Milestone 1b left the same
   question open and did not commit one. **Needed before step 2.**
2. **Does `refute` grow a `check` subcommand?** PRD milestone-2 question 2. The
   compatible form is specified above and is what step 4 builds. *Not a blocker.*
3. **Is the `check` verb's collision rule acceptable** — that a file literally
   called `check`, passed as the first of three positional arguments, is read as
   the verb? It is reachable as `refute -- check b.drat`, and no other spelling
   changes meaning. Raised because it is the only ambiguity the CLI has ever
   had. *Not a blocker; the alternative is not accepting the verb at all.*
4. **`Limits::max_var`** — unchanged from parts 1 and 2, and still milestone 4's
   to settle. It now bounds three more vectors, all grown on demand, so it is
   still a ceiling on what a literal may be rather than a decision about an
   allocation.

---

# Part 4 — milestone 3: scale and memory

**Status:** draft · **Date:** 2026-08-14 · **Supersedes:** nothing. Not one
acceptance rule in parts 1 to 3 moves. This milestone changes *how much the
checker holds while it applies them*, and nothing else. The 128-test suite must
be green at every commit in the build order, and the three-way verdict and the
single-construction-site rule for `Verified` are untouched.

## The measurement, first — and this time it changed the milestone

Same discipline as parts 1 to 3, and the same lesson, now paid for a third
time. Part 3's "solver ceiling" was a misdiagnosis and its batching experiment
made things measurably worse. So the artefact was built and profiled **before a
single data structure was proposed**, and the profile moved the milestone.

### The artefact, and how to rebuild it

The whole A217058 ladder comes from the author's certificate generator in
another of the author's repositories, with symmetry breaking off:

    python vdw/drat_certify.py --seq A217058 --ladder 0-7 --keep <dir>

Nothing from it is committed here. `<dir>` is a directory the reader chooses;
no path from any machine appears in a tracked file, as with
`tools/differential.sh --extra`. Rebuilding the whole ladder including the a(7)
rung took **56 s of solver time** on the machine these numbers come from, so it
is a re-measurable gate rather than a stored artefact.

### The ladder

Raw `kissat --no-binary` DRAT, checked by the release binary at `main`
(`0941aa5`). Peak working set by polling `PeakWorkingSet64` every 5 ms —
the same method part 2 used for `max_line_bytes`. Times are the whole process,
including reading the file.

| rung | n, j | `.drat` bytes | additions | deletions | peak live | `refute` | peak WS |
|---|---|---:|---:|---:|---:|---:|---:|
| a(0) | 18, 0 | 4,874 | 173 | 207 | 207 | 0.07 s | 4.6 MB |
| a(1) | 21, 1 | 20,423 | 559 | 633 | 571 | 0.08 s | 4.8 MB |
| a(2) | 25, 2 | 111,029 | 2,388 | 1,695 | 1,448 | 0.09 s | 5.3 MB |
| a(3) | 29, 3 | 668,068 | 11,067 | 7,836 | 4,218 | 0.19 s | 7.3 MB |
| a(4) | 33, 4 | 2,508,578 | 31,195 | 26,988 | 10,400 | 0.82 s | 14.1 MB |
| a(5) | 36, 5 | 8,059,616 | 87,354 | 77,770 | 17,108 | 3.10 s | 29.3 MB |
| a(6) | 40, 6 | 15,582,333 | 154,759 | 143,153 | 21,354 | 6.91 s | 43.4 MB |
| **a(7)** | **42, 7** | **87,490,047** | **763,382** | **750,578** | **40,631** | **63.90 s** | **182.6 MB** |

The a(4) row reproduces part 3's measurement to the unit — 31,195 additions,
26,988 deletions, 10,400 peak live, 626,008 occurrence updates — which is the
check that this ladder is the same population part 3 measured.

### The two acceptance artefacts already pass

| what | figure |
|---|---|
| a(7) raw `.drat`, 87,490,047 B, 1,513,960 lines | `s VERIFIED`, 63.90 s, **182.6 MB** |
| a(7) `.lrat`, 117,547,684 B, 851,744 lines, produced by `drat-trim -L` | `s VERIFIED`, 1.59 s, **5.5 MB** |
| `drat-trim` backward on the raw proof | `s VERIFIED`, 21.9 s |
| `drat-trim -f` on the raw proof | `s VERIFIED`, 33.5 s, **141.2 MB** |
| the LRAT is 574,437 of the raw proof's 763,383 lemmas | `drat-trim`'s own count |

**The PRD's "~200 MB LRAT" is 117.5 MB**, and the milestone's stated gate — "the
a(7) rung checks within memory budget" — is already met on both files by the
code at `main`, against any budget one would have written. What is not met is
that **no budget exists**, and that the DRAT path holds 182.6 MB to check about
10 MB of live data. Milestone 3 is therefore: state the budget, delete the
garbage that makes it hard to state, and pin both with tests that can fail.

### Where the bytes go

Counted with a throwaway instrumented build that walked every structure in the
store at the end of the run and reported capacities, not estimates. It accounts
for **179.8 of the 182.6 MB** measured from outside the process, so the model is
the process.

| structure | a(4) rung | a(6) rung | **a(7) rung** | live at a(7) |
|---|---:|---:|---:|---:|
| `lits` — the clause arena | 2.0 MB | 8.0 MB | **64.0 MB** | 0.5 MB |
| `bykey` table + keys + id lists | 4.4 MB | 20.8 MB | **96.5 MB** | 1.6 MB |
| `clauses` — one `ClauseMeta` per clause ever added | 0.5 MB | 2.6 MB | **11.4 MB** | 0.2 MB |
| `occ` heap | 0.7 MB | 2.1 MB | 5.1 MB | 5.1 MB |
| `watches` heap | 0.3 MB | 1.0 MB | 2.6 MB | 2.6 MB |
| `watches` + `occ` slot vectors (96 B per variable) | 0.02 MB | 0.02 MB | 0.02 MB | 0.02 MB |
| `assign`, `trail`, `units` | 0.001 MB | 0.001 MB | 0.001 MB | 0.001 MB |
| **total accounted** | **7.9 MB** | **34.5 MB** | **179.8 MB** | **~10 MB** |
| live clauses of clauses ever added | 5,456 / 32,444 | 13,390 / 156,543 | **14,757 / 765,335** | |
| `bykey` entries with no live clause under them | 26,630 / 32,057 | 142,468 / 155,817 | **748,090 / 762,728** | |

Three facts, and each is a decision rather than a law:

- **The arena never shrinks.** `Store::delete` marks `live = false` and leaves
  the literals where they are. 98 % of the a(7) rung's clauses are dead at the
  end and 99.2 % of the arena is theirs.
- **`bykey` never drops a key.** `delete` pops an identifier out of the
  `Vec<u32>` and leaves the entry, so the map keeps a `Box<[Lit]>` copy of the
  literals of every distinct clause the proof ever contained — a *second* copy
  of the arena, plus a `Vec` header each. It is the largest single item.
- **`clauses` grows with every addition**, at 12 bytes each, whether the clause
  survives or not.

None of this is visible from the LRAT path, which is why it was never seen: the
LRAT checker's database is a `HashMap<ClauseId, Clause>` and `delete` calls
`remove`, so it is already proportional to the live database. It checks the
*larger* file — 117.5 MB against 87.5 MB — in 5.5 MB.

### The four experiments

Each was built on top of the previous one, run on the real ladder, and reverted.
Every one of them produced **identical counters** — additions, deletions, peak
live clauses, assignments, propagations, watch visits, candidates checked — and
the same verdict on every artefact, which is the evidence that they changed what
was held and not what was checked.

| # | change | a(7) footprint | a(7) peak WS | a(7) time |
|---|---|---:|---:|---:|
| — | `main` today | 179.8 MB | 182.6 MB | 63.90 s |
| **A** | drop a `bykey` entry when its last live copy goes | 84.8 MB | — | — |
| **B** | A, plus compact the arena when dead literals exceed live | 22.2 MB | **35.0 MB** | **57.88 s** |
| **D** | B, but no occurrence index at all: scan the live clauses | 17.1 MB | **26.9 MB** | **51.57 s** |
| **E** | B, plus a *lazy* occurrence index: no work on delete, filtered at query, purged at compaction | 18.5 MB | **31.1 MB** | **54.00 s** |

Smaller instances, three runs each, steady state (the first run of a freshly
linked binary is consistently 0.5–0.8 s slower and those readings are discarded):

| variant | a(4) | a(6) | pigeonhole 10x9 |
|---|---|---|---|
| `main` | 0.82 s / 14.1 MB | 6.91 s / 43.4 MB | 0.70 s / 14.4 MB |
| B | 0.84 s / 9.0 MB | 6.87 s / 17.0 MB | — |
| D | 0.74 s / 8.3 MB | 6.12 s / 14.3 MB | 0.70 s / 8.7 MB |
| E | 0.78 s / 9.7 MB | 6.22 s / 17.3 MB | 0.62 s / 9.8 MB |

**Compaction does not cost time; it saves it.** 63.90 s to 57.88 s on the same
proof with the same counters. That was not predicted — part 3's batching
experiment predicted a saving and measured a loss — and it is recorded here as a
measurement, not as a reason.

### The occurrence-index trigger part 3 wrote down has fired

Part 3 chose the index over the scan, and wrote the trigger for reversing it:
*"if `occurrence_updates` exceeds the scan cost the same run reports — RAT lines
times mean live clauses — on a real proof, go back to the scan."*

It has, and by more than the counter admits. `occurrence_updates` counts one
per literal inserted or cleared. It does not count the **linear search** that
each clearing performs: `delete` runs `slot.iter().position(...)` over an
occurrence list that holds every live clause containing that literal. Counted
directly:

| proof | occurrence list entries compared during deletion | watch visits | RAT additions | candidates the index was asked for |
|---|---:|---:|---:|---:|
| a(4) rung | **200,595,972** | 41,879,267 | 195 | 234 |
| a(7) rung | **31,076,047,076** | 3,015,020,345 | 320 | 384 |

**Thirty-one billion comparisons to answer 320 queries.** The index's
maintenance is an order of magnitude larger than propagation, which is the thing
the checker is supposed to be doing. Part 3's cost model priced an index update
at O(1) when the implementation makes it O(list length), and it priced the scan
at "RAT lines times mean live clauses" using the *peak*; the scan actually
examines 493,935 live clauses on the a(4) rung and 998,560 on the a(7) rung.
Both halves of the comparison were wrong in the same direction.

Also measured, because it decides the shape of the fix: **RAT additions do not
scale with the proof.** 195 at the a(4) rung, 320 at the a(7) rung across
763,382 additions, 198 on pigeonhole 10x9 across 32,196. They come from the
solver's preprocessing, not from its learning, so they track the *formula*.
That makes the pure scan (D) extremely attractive on every real file and
quadratic on a hand-written proof whose every line is RAT — which is exactly the
adversarial input this project assumes it will be handed.

**The decision is E**, on the measurement: within 5 % of D on every real proof,
faster than D on the RAT-dense instance, and with no quadratic term for an
adversary to reach. D is recorded as the measured alternative and the trigger
for switching to it is written below.

### Three things measurement killed

- **Lowering `Limits::max_line_bytes`.** Longest line: **155 bytes** in the
  87.5 MB DRAT, **3,848 bytes** in the 117.5 MB LRAT, against a 16 MB ceiling.
  The ceiling costs nothing on any real file — the reader's line buffer keeps
  the capacity of the longest line it has seen, not the ceiling — and lowering
  it could only begin rejecting a legitimate proof with one very long clause.
  **Unchanged**, now with a measurement behind it.
- **Making the per-literal vectors cheaper.** `watches` and `occ` are
  `Vec<Vec<u32>>`: 96 bytes per variable whether used or not. At 421 variables
  that is **0.02 MB**, four hundredths of one per cent of the a(7) rung's
  footprint. **Unchanged.** The 96 bytes matter only against `max_drat_var`,
  which is a ceiling and not a budget; see below.
- **Holding the proof, or memory-mapping it.** The reader streams and is
  measured to hold one line. Nothing to fix.

### One thing measurement proved about the test suite

Experiment B changes the a(7) rung's peak working set by a factor of 5.2 and
**leaves all 128 tests green**. Experiment E changes three of them, and all
three are exact-count assertions on `--stats` output, not verdicts. So: the
existing suite pins nothing whatsoever about memory, and this milestone's
controls have to be counters that a memory regression moves. That is the
milestone's own instance of the rule that a green suite says nothing about which
rules are pinned.

## The budget, stated

The gate says "within memory budget", so here is the budget. It is stated three
ways on purpose, because each catches a different kind of regression.

**1. Per artefact, measured from outside the process.** Peak working set, 64-bit
release build, polled every 5 ms:

| artefact | budget | measured today | projected after this milestone |
|---|---:|---:|---:|
| a(7) rung, raw `.drat` (87.5 MB) | **64 MB** | 182.6 MB — **fails** | 31.1 MB (experiment E) |
| a(7) rung, `.lrat` (117.5 MB) | **16 MB** | 5.5 MB — passes | unchanged; the LRAT path is not touched |
| a(4) rung, raw `.drat` (2.5 MB) | **16 MB** | 14.1 MB — passes, barely | 9.7 MB |

64 MB is chosen as roughly twice what experiment E needs, so that the a(8) rung
— three to five times larger, and the next thing the author will point at this —
has somewhere to go before the budget is a lie. It is **open question 1**: the
number that matters for milestone 4 is a browser's, and the owner sets it.

**2. Structurally, as the property the per-artefact numbers are consequences
of.** The store's memory is proportional to the **live** database, plus a fixed
cost per addition, and never to the proof's bytes:

    held ~= 96 B x (largest variable)
          + O(1) x (literals in live clauses)
          + 12 B x (additions ever made)
          + the line being decoded

No general formula is fitted to three data points here, deliberately. The first
term is the ceiling discussed below; the second is what compaction and the
`bykey` prune deliver; the third is `clauses`, which this milestone does **not**
reclaim, and which is 11.4 MB of the a(7) rung's 31.1 MB.

**3. In CI, as counters**, because a peak working set is not portable and not
assertable. See the test plan: at the end of a run, no `bykey` entry has no live
clause under it, and the dead part of the arena does not exceed the live part.

**What the budget is not.** `Limits::max_drat_var` is 2^22 and a variable costs
96 bytes, so a fourteen-byte proof line naming variable 4,194,303 still buys
about **400 MB** — six times the budget, from a file that fits in a tweet. That
is a *ceiling*, which exists so the process cannot be made to abort on a failed
allocation, and it is not a budget. It stays where it is, because lowering it to
fit the budget would reject legitimate industrial instances with millions of
variables, and raising the budget to fit it would make the budget meaningless.
The residual is stated here rather than hidden, exactly as part 3 states the
residual in its binary sniff.

## Data model

No database, no migrations. Parts 1 to 3's tables stand. Three fields of
`drat::store::Store` change behaviour, one field is added, and one field of
`Limits` is added.

| Structure | Change | Why |
|---|---|---|
| `Store::lits` | Compacted when the dead part exceeds the live part: live clauses' literals are copied into a fresh `Vec` in identifier order and every live `ClauseMeta::start` is rewritten | 64.0 MB of which 0.5 MB live, at the a(7) rung |
| `Store::bykey` | The entry is removed when its `Vec<u32>` becomes empty | 96.5 MB of which 1.6 MB live; 748,090 of 762,728 entries dead |
| `Store::occ` | **Lazy.** `delete` no longer touches it. `resolution_candidates` filters the list by liveness *and* by containment of the negated pivot, writes the filtered list back, and returns it. Compaction purges every list | 31 billion comparisons at the a(7) rung to answer 320 queries |
| `Store::dead_lits`, `Store::live_lits` | New. Literals in dead and in live clauses, maintained on add and delete | The compaction trigger, and the CI assertion |
| `Store::compactions` | New. How many compactions ran | `--stats`, and a control: a fixture that must compact asserts this is non-zero |
| `Store::clauses` | **Unchanged.** One 12-byte `ClauseMeta` per clause ever added, dead or alive | Reclaiming it means an identifier that is not a slot index; see the decision below |
| `Limits::max_dead_arena_lits` | New. Compact when `dead_lits` exceeds `live_lits` and this floor. Default 1024 | Setting it to 0 forces compaction on every deletion, which is how the fuzz harness reaches this code on small proofs |

**Identifiers do not move.** Compaction rewrites `ClauseMeta::start` and never
the identifier, so every rejection message names the same clause it names today,
`next_id` still counts every clause ever added, and the LRAT numbering a reader
knows is unchanged. This is the property that makes the change invisible from
outside, and the reason 128 tests can be required to stay green rather than
re-baselined.

### Compaction, normative

    compact():                       ; only from delete(), with the trail empty
      fresh = Vec::with_capacity(live_lits + slack)
      for meta in clauses:                      ; identifier order
          if !meta.live: meta.start = 0; meta.len = 0; continue
          fresh.extend(lits[meta.start .. meta.start + meta.len])
          meta.start = position where those literals just landed
      lits = fresh
      dead_lits = 0
      for slot in occ: retain the ids whose meta is live

Four things are load-bearing:

- **The trail is empty.** Compaction is reachable only from `delete`, which the
  checker calls only between steps, and every step unwinds itself completely.
  `assignments == assignments_undone` already asserts that on every positive
  fixture; this design depends on it, so the identity stops being a nicety.
- **The order within a clause is preserved.** The first two literals in the
  arena are the watched ones, however often `visit` has swapped them. A
  compaction that sorted, deduplicated or reordered would silently break the
  watch invariant. It copies the slice.
- **Nothing outside `ClauseMeta::start` refers to the arena.** `watches`, `occ`,
  `units` and `bykey` all hold identifiers. That is what makes the remap local,
  and it is the reason this is a 25-line change rather than a rewrite.
- **A dead clause's `len` is zeroed.** Not required — dead clauses are already
  unreachable through every index — but it converts any future stale reference
  from "reads someone else's literals" into "reads nothing", which is the
  fail-closed direction.

### The lazy occurrence index, normative

    add(id, lits):      for each literal, push id       ; unchanged
    delete(id, lits):   nothing                          ; was: linear search + swap_remove
    resolution_candidates(pivot):
      want = -pivot
      kept = [ id in occ[want] : clauses[id].live and lits(id) contains want ]
      occ[want] = kept
      return kept
    compact():          occ[l].retain(id -> clauses[id].live)  for every l

**Completeness is what soundness rests on**, and it is unchanged in shape:
every clause containing `want` was pushed when it was added, and an identifier
leaves a list only when its clause is dead. The filter is a *predicate over the
store*, not a trust in the list: a candidate is returned because it is live and
because it really does contain the negated pivot, both re-derived at query time.
An entry that should not be there is dropped; an entry that should be there was
never removed.

Measured consequence: with compaction purging the lists, the a(7) rung's queries
examine **384 list entries in total** — one per candidate — against
31,076,047,076 comparisons for the eager version.

**Why not the pure scan (D).** It is simpler, and it was measured faster on the
vdW ladder — 51.6 s against 54.0 s at the a(7) rung. It is also
`RAT lines x live clauses`, and while every real proof measured has a few
hundred RAT lines whatever its length, a hand-written proof of N all-RAT lines
against N live clauses is quadratic, and this project's stated posture is that
its input is adversarial. The trigger for reversing this, written down as part 3
wrote its own: **if a real proof reports `occurrence_entries_filtered` greater
than `rat_additions x peak_live_clauses`, the index is losing and the scan
should replace it** — one function, `resolution_candidates`, exactly as before.

### Why the metadata array is not reclaimed

`Store::clauses` is 11.4 MB of the a(7) rung's projected 31.1 MB and 12 bytes
per addition ever made. Reclaiming it means recycling slots, and a slot is
currently the identifier. Decoupling them costs: a `u64` reported identifier
stored per live clause, a free list, and — because the occurrence index is now
lazy — a stale entry that can point at a *reused* slot. That last one is
survivable (the query-time filter defines candidates by predicate, so a reused
slot holding a live clause that contains the negated pivot is a genuine
candidate) but it needs deduplication to keep `rat_candidates_checked` honest,
and it is exactly the kind of interaction that produces a subtle false accept.

Not worth it at this scale, and the trigger is written down rather than left to
taste: **when `12 B x additions` exceeds a quarter of the stated budget on an
artefact the owner actually holds — about 1.4 M additions, which is roughly the
a(8) rung — reclaim the metadata array, in its own milestone, with its own
measurement.**

## Interfaces

No public signature changes. `Stats` gains counters, on the same terms as parts
2 and 3: they exist to be asserted exactly.

```rust
// checker.rs
pub struct Stats { /* ... existing ... */
    /// Bytes the clause store holds at the end of the run: arena capacity,
    /// metadata, watch and occurrence lists, and the deletion index. Measured
    /// from capacities, so it is what the process asked the allocator for and
    /// not what it is using.
    pub store_bytes: usize,
    /// Of the arena, the bytes belonging to clauses that are no longer live.
    pub dead_arena_bytes: usize,
    /// Of the arena, the bytes belonging to live clauses.
    pub live_arena_bytes: usize,
    /// Distinct clauses held by the deletion index. Equal to the number of
    /// distinct live clause bodies; never to the number ever added.
    pub deletion_index_entries: usize,
    /// Arena compactions performed.
    pub compactions: u64,
    /// Occurrence-list entries examined by candidate queries, replacing the
    /// deletion-side maintenance that `occurrence_updates` no longer counts.
    pub occurrence_entries_filtered: u64,
}

// limits.rs
pub struct Limits { /* ... existing ... */
    /// Compact the clause arena once the literals of dead clauses exceed both
    /// the literals of live ones and this floor. Zero compacts on every
    /// deletion, which is what the fuzz harness sets.
    pub max_dead_arena_lits: usize,
}
```

`occurrence_updates` keeps its name and **narrows its meaning to insertions
only**, because deletion no longer touches the index. That is a deliberate
re-baseline of three existing assertions and it happens in a commit of its own
with the reasoning in the message — the same treatment part 3 gave the
trust-boundary test. The alternative, keeping a counter that counts something
the code no longer does, is decoration in a place this project does not put it.

`--stats` gains one line, printed only for a DRAT run, beside the two that
already are:

    refute: <n> KB held, <n> KB live arena, <n> KB dead arena, <n> compactions,
            <n> deletion index entries, <n> occurrence entries filtered

## Access control

Unchanged: no database, no accounts, no network, no stored state. The untrusted
input table gains two rows and corrects one.

| Attack | Vector | Control |
|---|---|---|
| Memory exhaustion by a proof that only ever adds | Nothing is ever deleted, so nothing is ever reclaimed | Unchanged and inherent: the live database *is* the proof. Now bounded and visible — `store_bytes` reports it, and `12 B x additions` is the only term that grows with a proof whose clauses all survive |
| Quadratic time from an all-RAT proof | Every line RAT against a database that never shrinks | The occurrence index, whose cost is per literal inserted and per candidate returned. This is the reason the pure scan was rejected despite being faster on every real file |
| ~~Unbounded allocation from `Vec<Vec<u32>>` at 24 bytes a slot~~ | A proof line naming variable 2^22 | Unchanged, and restated honestly: `max_drat_var` bounds it at about 400 MB, which is a ceiling and not the budget. Part 3's row said "bounded"; it is bounded at six times the budget this milestone states |

## Migrations

None; there is no database. The equivalents table gains two rows.

| # | Change | Reversible? | Rollback |
|---|---|---|---|
| 8 | `Store` reclaims: arena compaction, `bykey` pruning, lazy `occ`. `Limits` gains `max_dead_arena_lits`; `Stats` gains six counters | Yes | `git revert`. No public signature moves, no verdict string moves, no exit code moves. A 0.1.0 crate with no dependants |
| 9 | `occurrence_updates` narrows to insertions; three `--stats` assertions re-baselined | Yes | `git revert`. It is a counter on a diagnostic line, not a contract |

## Failure modes

Parts 1 to 3's tables stand. What part 4 adds:

| What breaks | Who notices | How we detect it | How we undo it |
|---|---|---|---|
| **False `VERIFIED` from a compaction that remaps a clause to the wrong literals** — the serious one, and new | Nobody, for months | `s01`, a store unit test that records every live clause before a forced compaction and compares after; `d13`, a hand-built proof over a **satisfiable** formula with `max_dead_arena_lits = 0` so every deletion compacts; and the full fuzz run with forced compaction, where a formula with a model has no refutation whatever the mutation was | Revert; withdraw any claim citing Refute in the same session |
| False `VERIFIED` from the lazy index dropping a live entry | Nobody | `d14`: a candidate that must be found after enough deletions to force a purge, over a satisfiable formula. The mutation-kill row for the retain predicate | As above |
| False rejection from `bykey` pruning an entry with a live copy left | The author, on a real certificate | Prunes only when the list is empty; `s02` deletes one of three copies and asserts the other two still delete. The a(4) rung would fail outright, since 39 of its additions duplicate a live clause | Fix; the rule is one branch |
| Compaction runs so often it dominates | Anyone with a large proof | `compactions` in `--stats`, and the ladder table. Measured at 44 compactions and a net *saving* of 6 s on the a(7) rung | Raise `max_dead_arena_lits` |
| The budget is quietly exceeded by a bigger artefact | The author, when it swaps | `store_bytes`, and the scale harness re-run on the ladder. The counter controls fail in CI long before the working set does | The metadata trigger above; then a milestone |
| A stale occurrence entry survives into a reused identifier | Nobody | Cannot happen: identifiers are never reused. This is written down because it is precisely what reclaiming the metadata array would introduce | — |

## Rollback

`git revert` plus `cargo build`, under a minute. No database, no deployment, no
persistent state, no consumer contract: exit codes, verdict strings and every
command line are untouched, and the only observable difference is three numbers
on a `--stats` line.

The irreversible act is unchanged and is **publishing a claim** — here, that the
largest certificate behind a published term has been checked by a second
implementation inside a stated budget. The order is hard, and it is part 3's
order with the fuzz run promoted, because the store it exercised has been
rewritten:

1. the whole suite green on stable and on 1.74.0, in debug and release;
2. the mutation-kill pass complete, with a named test dying for every new rule;
3. `tools/differential.sh` re-run, including the vdW rungs by `--extra`;
4. **`tools/fuzz.py` re-run to 10,000 cases with `max_dead_arena_lits = 0`, zero
   false accepts** — not the milestone-2 run quoted, a new one;
5. the scale harness run on the whole ladder, its table recorded with the method
   and the machine's role stated;
6. *then* any claim about the a(7) rung, in the README or anywhere else.

Quoting milestone 2's fuzz result for milestone 3's store is the one thing here
that cannot be taken back.

## Test plan

Framework unchanged: `cargo test`, no test dependencies, committed fixtures, CI
with neither binary. **The corpus is at 496 KB of its 500 KB budget, so this
milestone adds no fixture bytes**: every new control is an inline formula and
proof in `tests/drat.rs`, or a unit test on the store, both of which the file
already uses.

### What "red first" means here

A memory rule cannot be made red by demanding a verdict, because every variant
measured produces the same verdict. So:

- **The counter controls are genuinely red.** `store_bytes`,
  `dead_arena_bytes`, `deletion_index_entries` and `compactions` do not exist,
  so the tests do not compile until the counters do, and once they compile they
  fail on the numbers the current store produces. The commit that records the
  red states which is which.
- **The safety controls are green for the wrong reason** if they only assert a
  rejection: today there is no compaction to break, so `d13` and `d14` pass
  before the code exists. They are therefore written against a store with
  `max_dead_arena_lits = 0`, and the commit that adds compaction is the one that
  makes them meaningful. Their real evidence is the mutation-kill table, and the
  table is mandatory content of the milestone.

### Positive — must return `Verified`, exit 0

| # | Input | Asserts |
|---|---|---|
| P19 | `rat_pigeonhole.drat` with `max_dead_arena_lits = 0` | Verifies. **Same verdict and same counters** as with the default: additions, deletions, peak live, assignments, propagations, watch visits and candidates checked all unchanged. This is the whole safety argument for compaction, made assertable |
| P20 | every committed `.drat` fixture, with `max_dead_arena_lits = 0` | Verdict identical to the default run, fixture by fixture. Cheap, and it is the regression net for all of milestones 2 and 3 |
| P21 | `vdw_a217058_n21.drat` | Verifies, and `deletion_index_entries` equals the live clause count at the end, not the 559 additions |
| all | every positive | `assignments == assignments_undone` and `rup + rat + tautological == additions`, unchanged |

### Negative — must not print `s VERIFIED`; exit non-zero, exact reason asserted

| # | Input | Expected |
|---|---|---|
| D13 | Hand-built proof over a **satisfiable** formula, enough deletions to force several compactions at `max_dead_arena_lits = 0`, whose last step is only justified if a compacted clause kept its literals | `RatCheckFailed` or `NoConflict`, asserted exactly. A compaction that mixes two clauses' literals verifies this |
| D14 | Hand-built proof over a **satisfiable** formula where the RAT candidate that refutes the lemma is added early, survives a purge, and must still be found | `RatCheckFailed`, naming the candidate. A purge that drops live entries verifies this |
| D15 | A clause added three times and deleted twice, then a lemma that is only RAT because the third copy is still live, over a **satisfiable** formula | Rejection. A `bykey` prune that drops the entry while copies remain verifies this |
| D1–D12 | existing | Unchanged, and re-run with `max_dead_arena_lits = 0` |

### Boundary

| # | Input | Expected |
|---|---|---|
| B37 | `max_dead_arena_lits = 0` on a proof with **no** deletions | No compaction, `compactions == 0`, verdict unchanged |
| B38 | A proof that deletes every clause it adds, then adds the empty clause | Verdict unchanged; `dead_arena_bytes <= live_arena_bytes` at the end; `deletion_index_entries` back to the formula's live clauses |
| B39 | Deletion of a clause whose literals are the same set in a different order, after a compaction | Deleted. Compaction must not disturb the normalised key |
| B40 | A unit clause deleted after a compaction | Honoured, as B25 requires; the `units` list holds identifiers and compaction does not touch it |
| B41 | The longest line in the committed corpus, with `max_line_bytes` at its default | Parses. The measurement that kept the ceiling where it is: 155 bytes of real DRAT against 16 MB |
| B42 | Every literal of a live clause is still exactly what it was, across a forced compaction, for a fixture with 700 additions and 487 deletions | Equality, clause by clause. The store unit test |
| B1–B36 | existing | Unchanged |

### The mutation-kill table

Mandatory. For each rule: the change to make in the source, and the test that
must fail when it is made. **The measured column is filled in during the build
and any wrong prediction is corrected there rather than quietly re-aimed** —
part 3 got three of eleven wrong and said so.

| Rule | Source mutation | Test that must die | Measured |
|---|---|---|---|
| Compaction preserves each clause's literals and their order | copy `meta.len - 1` literals | B42, P19, P20 | *(build)* |
| Compaction rewrites `start` for every live clause | rewrite only the first | B42, D13, P19 | *(build)* |
| Compaction runs only with the trail empty | call it from inside the candidate loop | P19, `assignments == assignments_undone` | *(build)* |
| The dead arena is bounded | never compact | B38's counter assertion | *(build)* |
| `bykey` drops only empty entries | drop the entry on any deletion | D15, P21, a store unit test | *(build)* |
| `bykey` still holds live copies | prune on the first delete of a duplicated clause | D15, and the a(4) rung outright | *(build)* |
| The occurrence purge keeps live entries | invert the retain predicate | D14, P14, P15 | *(build)* |
| The query filter keeps entries that contain the pivot | drop the containment check | nothing should die — it is belt, not brace; if something does, the design is wrong about which | *(build)* |
| The query filter drops dead entries | keep them | D14 or a counter; recorded either way | *(build)* |
| `store_bytes` counts the arena | report only metadata | B38 | *(build)* |

Two of these are expected to fail in the **false rejection** direction and the
table will say which, as part 3's does. A row whose mutation kills nothing is a
finding, not a formality: it means the rule is unpinned, and the fixture for it
is written in the same step.

### Differential harness and fuzz (not CI)

- `tools/differential.sh` unchanged in shape, re-run in full. The vdW rungs
  arrive by `--extra`, and the ladder now goes to the a(7) rung.
- `tools/fuzz.py` gains `--force-compaction`, which sets
  `max_dead_arena_lits = 0` through the CLI, and the 10,000-case gate is re-run
  under it. Random proofs are small and delete little, so without the flag the
  new code path is reached by almost none of the 10,000 cases — a harness that
  never enters the code it is guarding is decoration.

### The scale harness

`tools/scale.sh`, beside `differential.sh` and reading the same
`$KISSAT` / `$DRAT_TRIM` / `$REFUTE` or flags, never a path in a tracked file.
Given a directory of `.cnf` / `.drat` pairs it prints one row per artefact:
bytes, additions, peak live clauses, wall clock, peak working set, and the same
for `drat-trim -f` beside it. Peak working set is read from the OS per platform;
where it cannot be, the column says so rather than guessing. Nothing about this
runs in CI, and nothing it measures is a test.

## Build order

1. Branch `design/milestone-3`. Documents only: this part, the PRD's milestone-3
   section, the App Flow delta. **Commit.**
2. `Stats` gains the six counters, computed from the store as it is today, and
   `--stats` prints the new line. No behaviour change. The counters report the
   waste: this commit is what makes the problem visible from the command line.
3. Write P19–P21, D13–D15 and B37–B42 against **this** binary. Run. Paste the
   failing output into the commit message, marking which are red because the
   counters are wrong and which because the rule does not exist yet. **Commit
   red. This commit is the evidence for the milestone.**
4. `Limits::max_dead_arena_lits`, plumbed to the CLI as a hidden flag for the
   fuzz harness. Nothing consumes it yet.
5. **`bykey` pruning.** One branch. D15 and P21 go green. Re-measure the ladder
   and record the footprint: expected 179.8 MB to 84.8 MB at the a(7) rung.
6. **Arena compaction**, with the store unit tests (B42, `s01`, `s02`) written
   first in this commit. B38, D13 go green. Re-measure: expected 84.8 MB to
   22.2 MB, and *record the time*, which experiment B measured as a 6 s saving
   and which must be reported whichever way it comes out.
7. **The lazy occurrence index**, and the `occurrence_updates` re-baseline **in
   its own commit** with the reasoning in the message. D14 goes green. Re-measure.
8. Full suite on stable and on 1.74.0, both profiles.
9. The mutation-kill pass, every row, output recorded, wrong predictions
   corrected in the table.
10. `tools/scale.sh`; run it on the whole ladder; paste the table.
11. `tools/differential.sh` re-run, including the rungs by `--extra`.
12. `tools/fuzz.py --force-compaction`, 10,000 cases, zero false accepts, summary
    pasted.
13. **Only now:** the budget's measured figures into this document beside the
    projections, the README's scale paragraph, `SESSION_HANDOFF.md`.
14. Push the branch. CI green on all five jobs. Stop; merging is the owner's.

Steps 5, 6 and 7 are separable and each is measured on its own. **If step 7
measures worse than step 6 on the ladder, it is dropped** and the eager index
stays, with the numbers recorded — the same treatment part 3's batching
experiment got.

## Open questions

1. **Is 64 MB the right budget?** PRD milestone-3 question 1. It is set by what
   the a(7) rung needs with headroom, not by a browser, and milestone 4's
   ceiling is the one that will matter. Changing it changes a number in this
   document and one assertion in the scale harness. **Blocks calling the
   milestone done, not starting it.**
2. **Does the ladder become a documented local gate?** PRD milestone-3
   question 2. Nothing above n=21 can be committed. Proposed: `tools/scale.sh`
   reading a directory, exactly as `differential.sh --extra` does. *Not a
   blocker.*
3. **Should `store_bytes` be reported for the LRAT path too?** It is 5.5 MB on
   the a(7) rung's LRAT and the path is not touched by this milestone, so the
   counter would be a second implementation of the same idea over a different
   database. Proposed: no, and the `--stats` line stays DRAT-only, as part 3's
   does. *Not a blocker; one commit either way.*
