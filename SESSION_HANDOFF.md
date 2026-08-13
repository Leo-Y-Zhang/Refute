# Session handoff

**State:** milestone 1 is built and green. Branch `design/milestone-1`, nothing
pushed, `main` untouched.

Build order steps 1 to 10 in `docs/TDD.md` are done. The suite is 42 tests:
6 positive, 12 corruption controls, 14 boundary, 7 CLI, 3 trust boundary.

## Exact next step

Run the CI workflow once on a branch and read the result. It has never
executed — `.github/workflows/ci.yml` is written and its individual commands
were run locally on Windows, but the Ubuntu jobs and the 1.74.0 matrix leg on
Linux are unverified until a real run. **That push is the owner's call, and it
is a branch push, not `main`.**

## Verified locally, on this machine

- `cargo test --no-fail-fast` — 42 passed, 0 failed, on stable 1.97.1
- `cargo test --no-fail-fast` on **1.74.0** — 42 passed, so `rust-version` in
  `Cargo.toml` is measured, not asserted. This settles TDD open question 3.
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo fmt --all --check` — clean
- The built binary run against real `kissat` + `drat-trim -L` artefacts outside
  the fixture set: three random 3-SAT refutations verified, agreeing with
  `drat-trim`; pigeonhole 8x7 reported `s UNSUPPORTED` on proof line 2.

## Not verified

- Any CI run at all. No workflow has executed.
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
   a real proof so that the `RatHints` path is exercised at all. The design's own
   argument is stronger for it: the empty hint list is not an edge case, it is
   the first thing a real proof hits.

2. **The design's measurement reproduces exactly.** Pigeonhole 8x7 gave 2,747
   RUP additions, 70 RAT, 56 empty-hint, 1,459 deletions, ids 205 to 3571.

## Open questions still needing the owner

1. **Ship milestone 1 publicly while most real proofs report `UNSUPPORTED`?**
   The README is written as though the answer is yes — the limitation is the
   second thing on the page, with the measured table and the exact output a
   reader will get. If the answer is no, the change is confined to that section.
2. **`Limits::max_var` default of 2^26** (67M variables, a 64 MB assignment
   vector). Generous for a browser. 2^22 would be safer, and milestone 4 can
   override it per platform. Nothing blocks on it; the type exists either way.
3. **Playground certificate set (milestone 4).** Unchanged from `docs/PRD.md`.

Question 2 in the PRD — the LICENCE copyright line — was taken as
`Copyright (c) 2026 Refute contributors`, its proposed value. No personal name
appears anywhere in the repository.

## Deviations from the written build order, and why

- **The CLI arrived at step 2, not step 8**, reduced to argument handling, file
  opening, exit codes and the verdict strings. Without a binary there is nothing
  to observe the step-3 tests failing against, and that observation is the point
  of the exercise. `--stats` still landed at step 8.
- **Fixtures (step 4) were generated before the tests (step 3).** A test that
  fails because its fixture does not exist is red for the wrong reason. Doing it
  in this order makes every failure in the evidence commit attributable to the
  stub accepting what it should reject.
- **`check_readers` was added** beyond the TDD's interface list, so that the rule
  "a formula we cannot read is a proof we cannot accept" lives in the library
  where the suite covers it, rather than in `main` where it would not.
- **A sixth positive fixture** (`random_unsat`, 980 real RUP lemmas) was added.
  Every other positive fixture is tens of steps, and a subtly over-strict
  checker would pass all of them.

## Reference binaries

Never named by path in a tracked file. `tools/gen_fixtures.sh` reads `$KISSAT`
and `$DRAT_TRIM`, or takes `--kissat` / `--drat-trim`. Generated fixtures are
committed so CI needs neither.
