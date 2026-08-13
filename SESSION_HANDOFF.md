# Session handoff

**State:** milestone 1 is built, reviewed, green and **on `main`** (4b6bb9e).
Milestone 1b is in design on branch `design/milestone-1b`: `docs/TDD.md` part 2,
the PRD's 1b section and the App Flow delta are written. No code has changed.

Build order steps 1 to 10 in `docs/TDD.md` part 1 are done, and the findings from
the test, security and release reviews that followed are fixed. The suite is 53
tests: 8 positive, 12 corruption controls, 19 boundary, 10 CLI, 4 trust
boundary.

## Exact next step

Build order **step 2 of `docs/TDD.md` part 2**: extend `tools/instances.py` and
`tools/gen_fixtures.sh`, generate the new fixtures, commit them.

It is gated on **part 2 open question 1** — whether a 49 KB vdW certificate
derived from the author's `MathRecords` work belongs in the committed corpus, or
whether every vdW check stays in the local differential harness. That answer is
the owner's; everything after it is mechanical.

The milestone-1 record below is kept as it was written.

## Verified in CI

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

## Verified on this machine

- `cargo test --no-fail-fast` — 53 passed, 0 failed, on stable 1.97.1
- `cargo test --no-fail-fast` on **1.74.0** — 53 passed, so `rust-version` in
  `Cargo.toml` is measured. This settles TDD open question 3.
- `cargo clippy --all-targets -- -D warnings` — clean, with the panic floor
  now in `Cargo.toml` so the binary and the tests are inside it
- `cargo fmt --all --check` — clean
- The release binary on the real `random_unsat` artefact: `s VERIFIED`, exit 0,
  agreeing with `drat-trim` on the same files
- Pigeonhole 8x7 from `drat-trim -L`: `s UNSUPPORTED` on proof line 2

## What the reviews found, and what changed

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

## Not verified

- A local ref, `backup/pre-filter-branch`, still points at the pre-rewrite
  history and so still contains the `.pyc`. It is local only, was never pushed,
  and the branch that was pushed is clean at every commit. Delete it with
  `git branch -D backup/pre-filter-branch` when the rewrite is trusted.
- Behaviour on a proof larger than ~100 KB. The largest artefact checked end to
  end is 97 KB / 980 lemmas. Milestone 3 is where 200 MB gets tested.
- Anything about RAT. Milestone 1b.

## Two findings from the build worth carrying forward

1. **B12 as specified in the TDD is not reachable.** It asked for a real
   `drat-trim` proof to report `Unsupported(RatHints)`. In every instance
   measured — pigeonhole 5x4, 6x5, 7x6, 8x7 — the *first* unsupported construct
   is an empty hint list, on line 2, every time, because the RAT blocks resolve
   against exactly those lemmas. B12 now asserts what the real file does, and a
   new fixture `b12b_rat_hints` carries a single RAT line copied verbatim out of
   a real proof so that the `RatHints` path is exercised at all.

2. **The design's measurement reproduces exactly.** Pigeonhole 8x7 gave 2,747
   RUP additions, 70 RAT, 56 empty-hint, 1,459 deletions, ids 205 to 3571.

## Open questions still needing the owner

1. **Ship milestone 1 publicly while most real proofs report `UNSUPPORTED`?**
   The README is written as though the answer is yes — the limitation is the
   second thing on the page, with the measured table and the exact output a
   reader will get. If the answer is no, the change is confined to that section.
2. **`Limits::max_var` default of 2^26** (67M variables). It no longer decides
   an allocation on its own — the assignment vector is sized from the formula —
   so this is now only a ceiling on what a literal may be. Milestone 4 can
   lower it per platform.
3. **Playground certificate set (milestone 4).** Unchanged from `docs/PRD.md`.

Question 2 in the PRD — the LICENCE copyright line — **was not taken as this
paragraph used to claim.** It said the line read `Copyright (c) 2026 Refute
contributors`; `LICENCE` carries the owner's public name, which is what the PRD
records the owner deciding. The file wins and the paragraph is corrected here.
Nothing else about the repository's identity has changed: no email, no location,
no build path, no machine name appears in any tracked file.

## Deviations from the written build order, and why

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

## Reference binaries

Never named by path in a tracked file. `tools/gen_fixtures.sh` reads `$KISSAT`
and `$DRAT_TRIM`, or takes `--kissat` / `--drat-trim`. Generated fixtures are
committed so CI needs neither. A re-run of the generator reproduces the whole
corpus byte-identically; that was checked, twice, while adding to it.
