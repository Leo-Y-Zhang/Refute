# Refute

An independent forward checker for LRAT unsatisfiability proofs, in Rust, with
no dependencies.

A SAT solver answering UNSAT is an assertion, not a proof. Refute exists to be a
second opinion on the certificate — written separately from `drat-trim`, so that
a soundness bug in one is not a soundness bug in both.

## Read this before using it

**Milestone 1 checks RUP steps with hints. It does not check RAT steps, and most
real proofs contain them.** On a measured `drat-trim -L` file for pigeonhole 8x7
— 204 clauses, 2,873 core lemmas — the addition lines break down as:

| line kind | count | share |
|---|---:|---:|
| RUP, all-positive hints | 2,747 | 96.0 % |
| RAT resolvent blocks (negative hints) | 70 | 2.4 % |
| empty hint list | 56 | 2.0 % |

That 4.4 % is not spread evenly. **In every instance measured, 5x4 through 8x7,
the first construct Refute cannot check is on line 2 of the proof.** Run it on a
real `drat-trim` file today and you will get:

```
$ refute pigeonhole.cnf pigeonhole.lrat
s UNSUPPORTED
refute: proof line 2: addition with an empty hint list; milestone 1 checks RUP
hints only. Use drat-trim for RAT proofs until milestone 1b
```

That is the honest state of the tool. RAT hint blocks are milestone 1b and are
the immediate next piece of work, not a nice-to-have. Until then Refute is a
complete checker for pure-RUP proofs — random 3-SAT refutations, and any proof
whose lemmas all carry positive hints — and nothing more.

**`s UNSUPPORTED` is not a pass.** It exits 2. A caller grepping for `VERIFIED`
would also match `NOT VERIFIED`, so test the exit code, never the string alone.

## Use

```
refute <formula.cnf> <proof.lrat> [--stats]
```

`--help` and `--version` are answered only when one of them is the whole command
line. Beside anything else they are a usage error and exit 3, because the exit
code is the verdict and a flag must never stand in for a check that did not
happen. `--` ends the flags, so a file called `--help` can still be checked.

Produce the inputs with any solver that emits DRAT, then convert:

```
kissat --no-binary formula.cnf proof.drat
drat-trim formula.cnf proof.drat -L proof.lrat
refute formula.cnf proof.lrat
```

`drat-trim` is currently in the trust chain as the *producer* of the LRAT. It is
not in the chain as the *checker*, which is the point. Milestone 2 checks DRAT
directly and removes it entirely.

### Exit codes

| Code | Stdout | Meaning |
|---|---|---|
| 0 | `s VERIFIED` | A checked sequence of steps derived the empty clause |
| 1 | `s NOT VERIFIED` | The proof was read and found wanting, or would not parse |
| 2 | `s UNSUPPORTED` | The proof uses a construct this milestone does not check |
| 3 | — | Bad arguments, or a file that would not open. Nothing was checked |

3 is deliberately distinct from 1. Conflating "no proof" with "bad proof" hides
a typo in a CI script for as long as it takes someone to notice their gate has
been passing on a missing file.

Output is plain ASCII: no colour, no Unicode, no progress animation. A verdict
survives `refute a.cnf b.lrat > log.txt`.

## What it will not do

- **Produce proofs.** Refute never runs a solver. Solving and checking stay in
  different programs, or the second opinion is worth nothing.
- **Beat `drat-trim` on time.** Different algorithm, different input. Getting an
  LRAT file at all means running `drat-trim -L` first, so any pipeline using
  Refute includes `drat-trim`'s work and cannot be faster than it. A benchmark
  table that compares `refute` on LRAT against `drat-trim` on DRAT and calls the
  difference a speedup is dishonest.
- **Trim proofs, extract cores, read binary LRAT, or run anything in parallel.**

## Where Refute differs from `drat-trim`

**A leading UTF-8 byte order mark is skipped**, once, at the start of either
file. Windows editors write one on save; neither format mentions it. Skipping
cannot produce a false `VERIFIED` — the mark carries no clause, no hint and no
identifier — while rejecting fails a file that is otherwise exactly right, with
a message naming a token its author cannot see. Refute is the more permissive
of the two here, and measurably so: `kissat` refuses such a formula outright
with `parse error: expected 'c' or 'p' at start of line`. A mark anywhere other
than the first line is still an error.

**Deletions of unit clauses are honoured.** `drat-trim` ignores them, a rule
that protects backward checking. Refute checks forward with hints and needs no
such exception: if a later step needs a deleted unit, its hint lookup fails and
the proof is rejected. Refute is the stricter of the two here, on purpose.

Deletion is otherwise permissive — deleting an identifier that was never added
is counted under `--stats`, not rejected. Deletion only ever removes tools from
the checker, so a spurious one can cause a later rejection but can never cause a
false `VERIFIED`.

## Build and test

```
cargo build --release
cargo test
```

53 tests: 8 proofs that must verify, 12 corruption controls that must not, 19
boundary cases, 10 on the command line contract, 4 on the trust boundary.

Every corruption control was written and run against a `check()` that returned
`Verified` unconditionally, and observed failing, before the checking code
existed. The failing output is in the commit that introduced them. A rejection
test that has never been seen red proves nothing — and neither does a test that
passes with the defect it describes, which is why the trail's unwinding is
asserted by counting rather than by a stopwatch, and why the escaping test runs
a fixture carrying real escape bytes.

Minimum supported Rust is 1.74.0, and CI runs the whole suite on it, so that is
a measured claim rather than a hopeful one. Linters are pinned to 1.97.1.

### Regenerating the fixtures

Fixtures are committed, not generated at test time: CI has neither binary, and a
suite that skips itself when a tool is missing is how a checker ends up never
having been run. To re-derive them:

```
KISSAT=/path/to/kissat DRAT_TRIM=/path/to/drat-trim tools/gen_fixtures.sh
```

The script is deterministic; a re-run produces byte-identical files. No build
location appears in any tracked file. See `tests/fixtures/README.md` for the
provenance of each one.

## The property this is all in service of

`s VERIFIED` is printed only when a checked sequence of steps derives the empty
clause from the parsed formula. It is enforced structurally, not by discipline:
`Verdict::Verified` has no public constructor, no `Default` and no `From`, and
is built at exactly one site in the library. A test reads the source and fails
the build if a second site ever appears.

A false `VERIFIED` is the only serious defect this project can have. It would
launder a wrong theorem into a published one. Every rule resolves toward
rejecting when the answer is unclear.

## Documents

`docs/PRD.md`, `docs/TDD.md`, `docs/APP_FLOW.md`, `docs/DESIGN_BRIEF.md`.

## Licence

MIT. See `LICENCE`.
