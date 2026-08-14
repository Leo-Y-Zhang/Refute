# Session handoff

**State:** milestones 1, 1b and 2 are on `main` (`0941aa5`). **Milestone 3 —
scale and memory — is BUILT on `feat/milestone-3`**, build order steps 2
through 13 of `docs/TDD.md` part 4, 151 tests green on stable and on 1.74.0 in
both profiles. Not merged: merging is the owner's.

*This paragraph has been wrong twice, in the same direction both times, because
it gets written before the merge and not after it. So it says what `git log`
says and nothing else.*

## Exact next step

**Push the branch, read the five CI jobs, and stop.** That is build order step
14 and it is the only step left. Merging `feat/milestone-3` into `main` is the
owner's decision, and two questions below want an answer first — neither blocks
the merge mechanically, and one of them is the number the README now prints.

## What milestone 3 did, in one line each

The a(7) rung — an 87.5 MB raw DRAT refutation of a published term — took
**182.6 MB of peak working set to check about 10 MB of live data**. It now
takes **31.2 MB**, inside a stated 64 MB budget. It is also **12.8 s faster** —
51.1 s against 63.9 s — which was not the trade anyone expected to get.

- **`Stats` gained six counters** and `--stats` a line, because a memory rule
  cannot be pinned by a verdict: every store variant the design measured
  returned the same verdict on every artefact, and a change that moved peak
  memory by a factor of five left all 128 tests green.
- **`Limits::max_dead_arena_lits`**, default 1,024, with an undocumented
  `--max-dead-arena-lits=N` for the fuzz harness. The one field in `Limits`
  that is not a guard: no input can reach it.
- **The deletion index drops a key when its last copy goes.** It held one entry
  per distinct clause the proof *ever* contained — a second copy of the arena,
  96.5 MB of the 179.8 MB accounted. 179.8 MB to 86.7 MB.
- **The arena compacts** when its dead half is the larger one. 86.7 to 22.5 MB,
  and 9.4 s *faster* on the a(7) rung, measured by alternating two binaries on
  the same file three times each. The design predicted 6 s.
- **The occurrence index went lazy.** Deletion stopped clearing entries — each
  clearing was a linear search, 31 billion comparisons on that rung to answer
  384 queries — and the query filters what it finds. 22.5 to 18.7 MB
  accounted, 31.2 MB peak, another 4.5 s.

*The two time figures are from separate alternating runs, in different windows,
so they do not sum to the 12.8 s end to end and are not meant to. What each one
is evidence for is its own step, which is what the build order asks: 9.4 s is
compaction against the prune alone, and 4.5 s is the lazy index against
compaction with the eager one.*

## The three things worth carrying forward

1. **One line decided the lazy index, and it was not in the design.** `retain`
   does not give capacity back, so the purge left every occurrence list sized
   to every clause ever added. Without a `shrink_to_fit` the lazy index
   measured **worse** than the eager one it replaced — 34.1 MB against 31.7 —
   and the build order's own rule would have dropped it. With it: 31.2 MB, and
   faster. The rule that saved it is the build order's insistence that step 7
   be measured separately from step 6.
2. **Two rejection messages moved**, which the design said would not happen.
   `D1` names candidate 48 where it named 79, `D5` names 49 where it named 80:
   same pivot, same reason, same verdict, same candidate set. The loop stops at
   the first candidate whose resolvent is not implied, and eager deletion
   `swap_remove`d from the list, so the order was one nobody could state. It is
   now insertion order, so the candidate named is the lowest-numbered one that
   fails. An improvement, and still a change; both tests carry the old number.
3. **The mutation pass found two rules pinned by nothing, and both are fixed.**
   `B37` was widened to a proof with RAT candidates and no deletions, and now
   kills a compaction called from inside the candidate loop. `store_bytes`
   counting the arena took three attempts: the arena and the occurrence index
   are the same size by construction, so no `>=` against a single reported
   figure separates them, and the step-6 control that did discriminate stopped
   working when step 7 taught the purge to shrink. The one that works measures
   both terms independently from the containers and requires the total to cover
   both. All three attempts are written up in the TDD, because the two that
   failed both looked right.

Also worth knowing: the trail-empty precondition the design calls load-bearing
is **not** a soundness rule in this design. Compacting mid-propagation left
every verdict correct. It is a cost rule, and the TDD now says so.

## Verified on this machine

- `cargo test --no-fail-fast` — **151 passed, 0 failed**, on stable 1.97.1 and
  on 1.74.0, in debug and in release. The MSRV leg caught a real break that
  stable could not see: `size_of` reached the prelude in 1.80.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` —
  clean.
- **`tools/differential.sh --extra <ladder>`** — sixteen instances, four
  columns each, `drat-trim` and `refute` agreeing on every one. The ladder now
  runs to the a(7) rung: 87,490,047 bytes of raw DRAT and 117,547,684 of the
  LRAT `drat-trim -L` derives from it, both `s VERIFIED` by both checkers.
- **`tools/scale.sh`** — new, and the instrument every figure above comes from.
  Its `drat-trim -f` column reproduces the design's independently measured
  141.2 MB peak, which is a figure this milestone cannot have moved, and its
  additions and peak-live columns reproduce the design's ladder table to the
  unit on all eight rungs.
- **`tools/fuzz.py --force-compaction`**, 10,000 cases at seed 20260814, and
  the rollback section makes this the gate no a(7) claim ships before:

      cases            10000 (2938 unsatisfiable, 7062 satisfiable)
      comparisons      30564
      harmless mutants 10768 (39.0% still verified by both)
      strict wins      257 (all on the documented list)
      false accepts    0

  A new run, not milestone 2's quoted: the store it exercised has been
  rewritten three times since. The harmless-mutant rate is 39.0 per cent
  against milestone 2's 39.3 on a fifth of the cases, which is the check that
  the mutator has not quietly stopped mutating.

  The harness has since gained a coverage counter — it passes `--stats` and
  reports how many comparisons really compacted, because a proof that deletes
  nothing cannot reach that code at any floor. The gate above ran before that
  line existed, so the coverage was measured on its own run at the same seed:

      cases            1200 (355 unsatisfiable, 845 satisfiable)
      comparisons      3685
      harmless mutants 1316 (39.5%)
      compacted        463 of 3685 (12.6%) really entered the compaction
      false accepts    0

  ⚠ A 40-case sample taken first said 22.9 per cent, which is what a 40-case
  sample is worth. **12.6 per cent is the figure to quote**; three documents
  briefly carried the larger one and were corrected. The counter changes no
  verdict — it only lets the summary say what it exercised.

## Still open, and needing the owner

1. **Is 64 MB the right budget?** PRD milestone-3 question 1, and it is now the
   number the README prints. It is set by what the a(7) rung needs with
   headroom, not by a browser; milestone 4's WASM ceiling is the one that will
   matter. Changing it changes a number in three documents and no code.
2. **Does the rung ladder become a documented local gate?** PRD milestone-3
   question 2. `tools/scale.sh` exists and takes a directory, exactly as
   `differential.sh --extra` does. Nothing above n=21 can be committed — the
   a(7) rung alone is 87 MB against a 500 KB corpus budget.
3. **Should `store_bytes` be reported on the LRAT path too?** TDD part 4
   question 3. Proposed: no, and the `--stats` line stays DRAT-only. One commit
   either way.
4. Milestone 2's and 1b's open questions are unchanged and are below.

Nothing below this line is owed; everything below it is the milestone-2 record,
kept as written.

## Exact next step (milestone 2, complete)

**Run `tools/fuzz.py` to 10,000 cases with zero false accepts, then — and only
then — rewrite the README's opening.** That order is the rollback section of
`docs/TDD.md` part 3 and it is the one thing in this milestone that cannot be
taken back. **2,000 cases have been run at seed 20260814: 603 unsatisfiable
and 1,397 satisfiable formulas, 6,220 comparisons, zero false accepts, 48
strict wins all on the documented list, and 39.3 per cent of mutants still
valid proofs that both checkers verify.** The gate is 10,000.

Until that is done, **the README still describes milestone 1b**, and
deliberately: it says Refute is an LRAT checker, which is now an understatement
rather than a falsehood, and understating what a proof checker does is the safe
direction to be wrong in.

## What milestone 2 built

Build order steps 2 through 12 of `docs/TDD.md` part 3, in five commits.

- **`src/format.rs`** — detection by reading the first line, never the
  extension. The acceptance test is the two readers' own parsers, so "the
  grammars are disjoint" is a statement about the code that will read the file.
  The default arm is the incumbent LRAT reader, so no milestone-1 message
  moved. The binary sniff is widened — a NUL byte in the first kilobyte, or
  `a`/`d` not followed by a space or tab — and lives in one place both readers
  call.
- **`src/drat.rs`** — the reader. **`src/drat/store.rs`** — arena, watched
  literals, occurrence index, duplicate-aware deletion, propagation.
  **`src/drat/checker.rs`** — RUP, then the RAT candidate loop.
- **`src/verdict.rs`** — `EmptyClauseDerived` and `verdict::verified`. Two
  checkers, one door: the variant is built in one place and the *evidence* is
  built in two, both counted by `tests/trust_boundary.rs`.
- **20 fixtures**, 41 KB, generated by the same two scripts as the LRAT corpus.
  Re-running the generator reproduced all 99 existing fixtures byte for byte.
- **`tools/fuzz.py`** — new. **`tools/differential.sh`** — a second pair of
  columns, `drat-trim -f` and `refute` on the raw proof.

## The evidence, and what it is worth

**The checker reproduced every number the design measured independently**, with
a throwaway reference checker, before any of it was written: 91/71/20/24/75 and
peak 61 on pigeonhole 5x4, 702/630/72/108/487 and peak 348 on 7x6, and on the
A217058 a(4) rung 31,195 additions, 26,988 deletions, 10,400 peak live clauses
and **626,008 occurrence updates** — the last being the figure the whole
index-versus-scan decision rests on.

It also **filled in the one cell part 3 left blank**: the a(4) rung is 31,000
RUP additions and 195 RAT, and 195 is exactly what `drat-trim` reports as RAT
lemmas in core.

**The differential harness agrees on all eleven instances**, on both pairs of
columns: the pigeonhole ladder to 8x7, three random 3-SAT refutations, and
A217058 at n=21, n=25 and **n=33** — the 2.5 MB raw certificate behind a
published term, checked with `drat-trim` in the chain neither as checker nor as
producer. Reproduce with:

    tools/differential.sh --extra <dir of .cnf and .drat pairs>

where the pairs come from `MathRecords/vdw/drat_certify.py --keep <dir>`.

**Speed against the yardstick:** the a(4) rung takes `drat-trim -f` 0.787 s,
`drat-trim` backward 0.607 s, and Refute 1.128 s. That is **1.43x**, against a
gate of 50x.

**The mutation-kill pass killed all twelve mutations**, and corrected three
rows of its own table where the predicted victim was wrong. The measured column
is in part 3.

**The fuzzer found one hard failure, and it was the harness's**, not the
checker's: `kissat` does not always stop at the empty clause, so removing the
last step left one in the file and the mutant asserted the opposite of its
name. That falsifies **G3** in part 3 as a general claim — corrected there —
and it is the reason the rule the checker actually relies on is the weaker one:
the first empty clause ends the run, and nothing after it is read.

## Still open, and needing the owner

1. **Does a real van der Waerden certificate go into the committed corpus, in
   DRAT form?** PRD milestone-2 question 1. **Not committed**, because the
   question was not answered: n=21 is 20 KB of proof and 7 KB of formula
   against a 500 KB budget now standing at some 413 KB. Every vdW check in this
   milestone is in the differential harness, which runs nothing in CI. Saying
   yes later costs a `--keep` run; saying no after committing costs a history
   rewrite.
2. **Does `refute` grow a real `check` subcommand?** PRD question 2. Built as
   the compatible form specified: `check` is accepted only as the first of
   exactly three positional arguments, so every command line that worked before
   works now and means the same thing. A real subcommand tree is one commit
   either way.
3. **Is the `check` verb's collision rule acceptable** — a file literally called
   `check`, passed as the first of three positionals, is read as the verb? It
   is reachable as `refute -- check b.drat`. *Not a blocker.*

## The two questions milestone 1b left open

They were not answered at merge time and are still open. Neither blocks
milestone 2.

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
