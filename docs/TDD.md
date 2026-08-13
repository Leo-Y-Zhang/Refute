# TDD — Refute milestone 1: forward LRAT checker

**Status:** draft
**Date:** 2026-08-13 · **PRD:** [PRD.md](PRD.md) · **Repo:** Refute

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
    DuplicateId(ClauseId), NoEmptyClause,
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
  if id <= last_added_id -> reject NonMonotonicId
  if db contains id      -> reject DuplicateId
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
| B13 | 100k-variable formula, 50k-step proof | Completes; asserts the trail unwind is not O(vars) per step (time bound in the test) |

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

1. **Ship M1 publicly with `UNSUPPORTED` common, or hold for 1b?** PRD Q1. Changes
   the README's framing, not this design. *Recommendation: ship, loudly limited.*
2. **`Limits::max_var` default of 2^26 (67M vars, 64 MB assignment vector).** The
   author's vdW formulas use a few thousand variables. 2^26 is generous; 2^22
   would be safer for a browser. Needs one decision, and M4 can override it per
   platform. *Does not block the build — the type exists either way.*
3. ~~**`rust-version = "1.74"` is a guess** until CI runs that toolchain.~~
   **Closed during the build.** The whole suite was run on 1.74.0 locally —
   42 passed, 0 failed — before the CI job was written, so the floor is
   measured. CI runs the same leg so it stays measured.
