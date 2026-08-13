# Session handoff

**State:** milestone 1 is on `main` (4b6bb9e). Milestone 1b is **designed on
`design/milestone-1b` (d7532c6) and built on `feat/milestone-1b`**, which
branches from it. The suite is 79 tests, green on stable 1.97.1 and on 1.74.0;
`clippy --all-targets -D warnings` and `cargo fmt --check` are clean. The
branch is pushed. Nothing is merged, and `main` has not been touched.

## Exact next step

**The owner's decision: merge `feat/milestone-1b` into `main`, or send it
back.** Everything below it is done — the release blockers are closed, CI has
run on the real code, and the two questions that were open are still open and
still not blockers.

Answer them at merge time, not before:

## The two questions

1. **Does a real van der Waerden certificate belong in the committed corpus?**
   TDD part 2, open question 1. `vdw_rung` would be about 49 KB of CNF and LRAT
   derived from the author's `MathRecords` work, and would put a real
   certificate of a published term under CI on every commit — which is the whole
   point of the project. It also couples two repositories' artefacts.
   **It was not committed.** Every vdW check in this milestone is in the
   differential harness, which is local and runs nothing in CI. The formula is
   regenerated from `MathRecords/vdw/vdw4.py` in seconds, so saying yes later
   costs nothing; saying no after it is committed costs a history rewrite.
2. **Is a RAT step whose hint prefix already conflicts a rejection?** TDD part 2,
   open question 2. Built strict, as specified, on milestone 1's `EarlyConflict`
   reasoning. It is the one new rule with a plausible false-rejection risk
   against a producer other than `drat-trim`, it never fires on any real file
   measured, and it has its own reason code (`RatLemmaIsRup`) so relaxing it is
   one branch. It now has a test —
   `r11_rat_lemma_that_is_already_rup` — whose comment says in as many words
   that relaxing the rule accepts a good proof rather than a bad one, so
   answering "acceptance" means deleting that test and the reason code
   together, deliberately.

## What the closing session added

Five release blockers, found by the tester and the release-manager
independently. In every one of them the code was right and the tests were the
hole: one line either way and this checker printed `s VERIFIED` on a formula
`kissat` reports satisfiable, with a fully green suite.

- **R9, R10, R11** — three rejection rules with no fixture. R9 and R10 are
  false-accept holes; R11 is a strictness rule with no coverage at all, which
  is what got `Reason::DuplicateId` deleted in milestone 1. Hand-built in
  `tools/mutate.py`, because no real proof carries the shape; `kissat` is run
  on each formula during generation, so the satisfiability claims are its.
- **P12 and the trail balance.** `assignments == assignments_undone` is now
  asserted on every positive fixture, not just on B13, whose proof is pure RUP.
  It caught nothing on its own: `check_rat`'s tautology exit is reachable only
  by a tautological lemma with an empty hint list, and no fixture had one.
  Deleting its `unwind` left all 77 tests green. P12 is that fixture.
- **B22 and `Limits::max_line_bytes`.** `src/lrat.rs` claimed a 200 MB proof
  was read in constant memory. Measured: 268.6 MB of working set on a 200 MB
  single-line proof, because a line is buffered before any ceiling applies to
  what is in it. Now bounded, and the same file measures 28.6 MB.
- **The README's discipline claim, narrowed.** It said every corruption
  control was written before the rule that catches it and observed failing
  there. True of N1–N12 and R1–R8; false of everything added after the red
  commit `73c970b`. The paragraph now says which is which, and R9–R11 are
  labelled as what they are — justified by a recorded mutation kill, which is
  the weaker evidence.
- **The README's van der Waerden rows.** They cannot be reproduced from this
  repository alone; they need `tools/differential.sh --extra` pointing at
  another of the author's repositories. Stated next to them.

Every test added in this session was written after the code it covers. None of
them claims part 1's discipline. Each one's commit message carries the mutation
it was observed failing against, with the real output.

## What was built

Build order steps 2 to 11 of `docs/TDD.md` part 2, in that order, one commit
each.

- **Fixtures first** (c850d45): `rat_pigeonhole` (pigeonhole 7x6, 55,003 bytes),
  `resolvent_propagates`, `b17_binary_proof`, and `r01`–`r08`. The corpus is
  208 KB of a 500 KB budget. Re-running the generator left every milestone-1
  fixture byte-identical.
- **Tests red** (73c970b): 72 tests, 22 failing, none of them a compile error.
  The failing output is in the commit message.
- **Parser** (6583562), **hint walk factored out** (cac0198), **the RAT step**
  (c44bca4), then the harness and the documents.

## Verified on this machine

- `cargo test --no-fail-fast` — 79 passed, 0 failed, on stable 1.97.1
- the same on **1.74.0** — 79 passed, so `rust-version` stays measured
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo fmt --all --check` — clean
- **`tools/differential.sh`**, the gate the TDD makes the README conditional on.
  Re-run after the closing session's changes; the first eight rows are from
  that re-run, and the two van der Waerden rows are from the build session and
  need `--extra`, so they were not re-run here:

| instance | LRAT | `drat-trim` | `refute` | agree |
|---|---:|---|---|---|
| pigeonhole 4x3 | 1,378 | `s VERIFIED` | `s VERIFIED` | yes |
| pigeonhole 5x4 | 3,167 | `s VERIFIED` | `s VERIFIED` | yes |
| pigeonhole 6x5 | 10,089 | `s VERIFIED` | `s VERIFIED` | yes |
| pigeonhole 7x6 | 55,003 | `s VERIFIED` | `s VERIFIED` | yes |
| **pigeonhole 8x7** | 386,428 | `s VERIFIED` | `s VERIFIED` | yes |
| random 3-SAT x 3 | 7,263 to 96,979 | `s VERIFIED` | `s VERIFIED` | yes |
| **A217058 rung, n=21 j=1** | 41,722 | `s VERIFIED` | `s VERIFIED` | yes |
| **A217058 rung, n=25 j=2** | 257,330 | `s VERIFIED` | `s VERIFIED` | yes |

  The last two are real certificates of the author's published work, built from
  `MathRecords/vdw/vdw4.py` with symmetry breaking off and passed in with
  `--extra`, so this repository still depends on nothing outside itself.

- The release binary on pigeonhole 8x7, which is the milestone's own gate:

      refute: 2873 additions, 1459 deletions, 63901 hints resolved, 294 peak
      live clauses, 0 unknown deletions, 81606 assignments, 81606 undone
      refute: 70 RAT additions, 56 vacuous, 196 resolvent blocks, 126 candidate
      scans, 20069 candidates examined, 196 candidates found
      s VERIFIED

  20,069 clauses examined is the number the design's occurrence-tracking table
  predicted for the scan it chose over an index, to the unit. Re-measured in
  the closing session, on the regenerated 8x7 proof: identical, including
  81,606 assignments and 81,606 of them undone.

- **Peak working set on a 200 MB proof written as a single line**, release
  binary, polled every 5 ms: 268.6 MB before `max_line_bytes`, 28.6 MB after.
  That is the measurement `src/lrat.rs`'s opening paragraph now rests on.

## What the build proved that the design could only argue

Two experiments, run on the finished tree and then reverted. Both are in
c44bca4's message with their real output.

1. **Remove the rule that every candidate must be covered**, and
   `r02_block_dropped` and `r05_empty_hints_with_candidates` print
   `s VERIFIED`. A corrupted certificate verifying. That is what the rule is
   for.
2. **Let a block name any live clause** instead of an uncovered candidate, and
   every R fixture is *still* rejected — by a different rule. The bare "is it
   rejected" assertions do not notice; the exact-reason assertions do. That is
   why the R series names its rule, step, line and resolvent block.

The closing session added four more, each in the commit that added the test.
Two of them print `s VERIFIED` on a satisfiable formula — `unwind(base)` taken
out from between the resolvent blocks, and a repeated literal read as a
tautology — and in both cases R9 or R10 was the *only* failure in the suite.
The third relaxes `RatLemmaIsRup` and correctly verifies a valid proof, which
is why R11's own comment says it is a tripwire and not a control. The fourth
removes `check_rat`'s tautology `unwind` and is caught by P12 alone.

## Deviations from the written build order, and why

- **`vdw_rung` is not committed.** Question 1 above. It is TDD P10.
- **Test numbers moved.** The TDD's P8–P11 are P9–P11 here, because a P8 already
  existed; its B14–B18 are B17–B21, because B14–B16 already existed.
- **B21 asserts a parse error**, where the TDD's boundary row asks for
  `NotAResolutionCandidate` on a block naming clause 0. A block naming clause 0
  has to be written `-0`, which scans as zero rather than as a negative and is
  rejected as a hint identifier. The TDD says both things in different rows; the
  parse error is the one the grammar produces, and asserting the other would be
  asserting something untrue.
- **R3 lands at step 49, proof line 6, resolvent 6**, not the design's 65/32/34.
  The mutation picks the first RAT line following any deletion; the reference
  implementation picked a different one. Same rule, deterministic either way.
- **`resolution_candidates` takes `&mut self`**, not the `&self` in the TDD's
  interface list, because it counts. Counting inside the function is what keeps
  `candidates_examined` meaningful if the occurrence index ever replaces it.
- **The binary sniff reads the first byte of the file**, not of the first
  non-empty line, and does it before UTF-8 decoding. A binary proof need not
  decode, and a failed decode is reported as an I/O error. The narrowing can
  only fail to recognise a binary proof; it has no route to `Verified`.
- **No commit was left in a state that prints `s VERIFIED` on a bad proof.**
  Experiment 1 above was run and reverted rather than committed red, which is a
  departure from part 1's practice of committing the red. The output is
  recorded in the commit message instead.

## Not verified

- **The 200 MB rung.** Largest artefact checked end to end is 386 KB, and the
  largest verified anywhere is the 257 KB vdW rung. The 200 MB file measured
  for `max_line_bytes` is a synthetic single line, not a proof. Milestone 3
  owns scale.
- **The two van der Waerden differential rows.** They stand from the build
  session and were not re-run in the closing one, because their formulas come
  from another repository via `--extra`. The eight rows that reproduce here
  were re-run and still agree.
- **Any producer of LRAT other than `drat-trim`.** Two of the new rules are
  strict on shapes only `drat-trim`'s behaviour has been measured against; that
  is question 2.
- A local ref, `backup/pre-filter-branch`, still points at the pre-rewrite
  history and still contains the `.pyc` described in the milestone-1 record. It
  is local only and was never pushed. Delete it with
  `git branch -D backup/pre-filter-branch` when the rewrite is trusted.

## Reference binaries

Never named by path in a tracked file. `tools/gen_fixtures.sh` and
`tools/differential.sh` both read `$KISSAT` and `$DRAT_TRIM`, or take
`--kissat` / `--drat-trim`. Generated fixtures are committed so CI needs
neither. `tools/differential.sh --extra <dir>` takes pre-built CNFs, which is
how a certificate from another repository is checked without this one depending
on it.

## Verified in CI (milestone 1b)

Run 31747764407 on `96bed67`, the first CI run on the milestone-1b code —
every green run before it was on a docs-only commit. All five jobs:

| Job | Result |
|---|---|
| `lint` — fmt and clippy on the pinned 1.97.1 | success |
| `test (ubuntu-latest, stable)` | success, 79 tests |
| `test (ubuntu-latest, 1.74.0)` | success, 79 tests |
| `test (windows-latest, stable)` | success, 79 tests |
| `test (windows-latest, 1.74.0)` | success, 79 tests |

The logs were read rather than the ticks. Each MSRV leg installed and reported
`rustc 1.74.0 (79e9716c9 2023-11-13)`; every test leg reported 26 + 12 + 24 +
13 + 4, which is the same split as the local runs; the CRLF fixture guard ran
on both Windows legs, which is the platform that would quietly rewrite it.

`main` was not touched and nothing was merged. The branch is
`feat/milestone-1b` at `96bed67`, pushed, tracking `origin`.

---

The milestone-1 record below is kept as it was written.

## Verified in CI (milestone 1)

The workflow has now run. Run 31729660983 on `9918064`, all five jobs green:

| Job | Result |
|---|---|
| `lint` — fmt and clippy on the pinned 1.97.1 | success |
| `test (ubuntu-latest, stable)` | success, 53 tests |
| `test (ubuntu-latest, 1.74.0)` | success, 53 tests |
| `test (windows-latest, stable)` | success, 53 tests |
| `test (windows-latest, 1.74.0)` | success, 53 tests |

The logs were read, not just the tick: each leg installed the toolchain it
claims (`rustc 1.74.0 (79e9716c9 2023-11-13)` on both MSRV legs) and reported
19 + 10 + 12 + 8 + 4 passing. The CRLF fixture guard ran on Windows, which is
the platform that would have quietly rewritten it.

## What the milestone-1 reviews found, and what changed

Ten fixes, each with its test observed failing first; the commit messages carry
the real failing output.

1. **A false rejection.** A repeated literal in a clause counted twice, so a
   hint that was unit was called non-unit. `drat-trim` verifies the same lemma
   sequence against the same formula — the fixture `dup_literal` is that
   disagreement, and every clause now enters the database deduplicated.
2. **`--help` anywhere in argv exited 0**, which under the documented contract
   is a pass for a proof never opened. Answered only as the whole command line
   now, with `--` to end the flags.
3. **A tracked `.pyc` carried a build location**, falsifying the README's claim
   that no tracked file does. Removed from every commit on the branch before it
   was ever pushed; `__pycache__/` is ignored.
4. **Terminal escape bytes were echoed verbatim** from an unreadable token, so
   a formula could repaint the verdict line above it. Escaped as `\xNN`, with
   two fixtures carrying the real payload.
5. **The panic floor covered the library only.** Moved to `Cargo.toml`, proved
   by an `unwrap` in the binary that passed before and fails now.
6. **B13's time bound passed with the defect it documented** — 14.9 s debug,
   0.10 s release, both inside 20 s. Replaced by exact counters.
7. **The header sized the assignment vector**: 19 bytes bought 64 MB.
8. **An I/O error named the line before the one that failed.**
9. **A UTF-8 BOM was a confusing parse error.** Skipped once, on line 1.
10. **The trust-boundary grep watched one door.** `Self::Verified` and a `use`
    of the variant now trip it; the second used to pass while hiding a real
    construction site.

Also: trailing tokens after a deletion are rejected as they are on an addition,
`Reason::DuplicateId` is gone because monotonicity already forbids what it
described, and `actions/checkout` is pinned to a commit.

## Two findings from the milestone-1 build worth carrying forward

1. **B12 as specified in the TDD was not reachable.** It asked for a real
   `drat-trim` proof to report `Unsupported(RatHints)`. In every instance
   measured the *first* unsupported construct was an empty hint list, on line 2,
   every time, because the RAT blocks resolve against exactly those lemmas.
   Milestone 1b checks both, so B12 now asserts that the same file verifies.

2. **The design's measurement reproduces exactly.** Pigeonhole 8x7 gave 2,747
   RUP additions, 70 RAT, 56 empty-hint, 1,459 deletions, ids 205 to 3571.

## Milestone-1 open questions still needing the owner

1. ~~**Ship milestone 1 publicly while most real proofs report `UNSUPPORTED`?**~~
   Answered: ship. The limitation it described is now closed, and the README's
   opening was rewritten after the differential harness agreed with `drat-trim`,
   in that order, which is the ordering rule the TDD's rollback section sets.
2. **`Limits::max_var` default of 2^26** (67M variables). It no longer decides
   an allocation on its own — the assignment vector is sized from the formula —
   so this is now only a ceiling on what a literal may be. Milestone 4 can
   lower it per platform.
3. **Playground certificate set (milestone 4).** Unchanged from `docs/PRD.md`.

Question 2 in the PRD — the LICENCE copyright line — **was not taken as an
earlier version of this paragraph claimed.** It said the line read `Copyright
(c) 2026 Refute contributors`; `LICENCE` carries the owner's public name, which
is what the PRD records the owner deciding. The file wins. Nothing else about
the repository's identity has changed: no email, no location, no build path, no
machine name appears in any tracked file.

## Deviations from the milestone-1 build order, and why

- **The CLI arrived at step 2, not step 8**, reduced to argument handling, file
  opening, exit codes and the verdict strings. Without a binary there is nothing
  to observe the step-3 tests failing against, and that observation is the point
  of the exercise. `--stats` still landed at step 8.
- **Fixtures (step 4) were generated before the tests (step 3).** A test that
  fails because its fixture does not exist is red for the wrong reason.
- **`check_readers` was added** beyond the TDD's interface list, so that the rule
  "a formula we cannot read is a proof we cannot accept" lives in the library
  where the suite covers it, rather than in `main` where it would not.
- **Three fixtures beyond the TDD's five positives.** `random_unsat` (980 real
  RUP lemmas) because every other positive fixture is tens of steps and a subtly
  over-strict checker passes all of them — which is exactly how the repeated
  literal was found; then `dup_literal` and the two `hostile_escape` pairs.
