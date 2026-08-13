# Session handoff

**State:** design complete, no code yet.

Design documents live in `docs/`:

- `docs/PRD.md` — what Refute is, who for, roadmap, what is out of scope
- `docs/TDD.md` — checker algorithm, trust boundary, limits, test plan, build order
- `docs/APP_FLOW.md` — CLI states and exit codes; playground (milestone 4)
- `docs/DESIGN_BRIEF.md` — terminal output and playground design

**Exact next step:** build order step 1 in `docs/TDD.md` — `cargo init --lib`,
`Cargo.toml` (edition 2021), MIT `LICENCE`, commit.

**Do not skip build order step 3.** The negative tests are written and run against
a stub that returns `Verified` for everything, and their failing output goes into
that commit message. A rejection test that was never seen red does not count.

**Two open questions are recorded at the end of `docs/PRD.md` and `docs/TDD.md`.**
Neither blocks step 1. Question 1 (ship milestone 1 publicly while most real
proofs report `UNSUPPORTED`) changes the README's opening and needs the owner.

**Reference binaries** are never named by path in a tracked file. The fixture
generator reads `$KISSAT` and `$DRAT_TRIM`, or takes `--kissat` / `--drat-trim`.
Generated fixtures are committed so CI needs neither binary.
