# TDD — Refute: forward LRAT checker

**Status:** part 1 built · part 2 draft
**Date:** 2026-08-13 · **PRD:** [PRD.md](PRD.md) · **Repo:** Refute

Part 1 is milestone 1 — RUP steps with hints — and is built, reviewed and green.
Nothing in it is amended below; [part 2](#part-2--milestone-1b-rat-hint-blocks)
adds RAT hint blocks and states, per rule, where it changes a part 1 decision.

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
