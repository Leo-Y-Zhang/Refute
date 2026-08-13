# Refute

An independent forward checker for LRAT unsatisfiability proofs, in Rust, with
no dependencies.

A SAT solver answering UNSAT is an assertion, not a proof. Refute exists to be a
second opinion on the certificate — written separately from `drat-trim`, so that
a soundness bug in one is not a soundness bug in both.

## Read this before using it

**Refute checks every addition line a text `drat-trim -L` file contains.** That
means RUP steps with hints, RAT steps with resolvent blocks, and the empty hint
list — which is not an absence but a claim, that the lemma's pivot has no
resolution candidate, and which Refute accepts only after establishing that for
itself from its own clause database.

Milestone 1 checked the RUP steps alone, which on a measured pigeonhole 8x7
proof was 96 % of addition lines and, in practice, nothing: the first line it
could not check arrived on line 2 of almost every real file. Milestone 1b closed
that. The evidence is a differential run against `drat-trim` on proofs too large
to commit, most recently:

| instance | LRAT | `drat-trim` | `refute` |
|---|---:|---|---|
| pigeonhole 4x3 to 7x6 | 1.4 KB to 55 KB | `s VERIFIED` | `s VERIFIED` |
| pigeonhole 8x7 | 386 KB | `s VERIFIED` | `s VERIFIED` |
| 3 random 3-SAT refutations | 7 KB to 97 KB | `s VERIFIED` | `s VERIFIED` |
| a mixed van der Waerden certificate, n=21 | 42 KB | `s VERIFIED` | `s VERIFIED` |
| the same family, n=25 | 257 KB | `s VERIFIED` | `s VERIFIED` |

Reproduce it with `tools/differential.sh`; it needs `kissat` and `drat-trim`,
which CI does not have.

**What is still not checked.** Binary proofs. `kissat` writes binary DRAT unless
told `--no-binary`, and handing one to a text checker is a common mistake, so it
has its own answer rather than a parse error:

```
$ refute formula.cnf proof.drat
s UNSUPPORTED
refute: proof line 1: this is a binary proof; refute reads text LRAT.
Re-run kissat with --no-binary, then drat-trim with -L
```

Also unchecked, and out of scope here: DRAT itself, backward checking, trimming,
core extraction, binary LRAT, parallelism.

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
| 2 | `s UNSUPPORTED` | The proof is binary, not text LRAT. Nothing was checked |
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

**A RAT step must name the resolution candidate set exactly**: no live clause
holding the negated pivot left uncovered, and no block naming anything else.
Every resolvent block in the 703 measured is refuted by the negation of its own
resolvent, so a checker that skipped the trivially refuted ones would accept the
deletion of any real block — which is to say it could not tell a proof from a
proof with a block removed. Two narrower rules follow the same reasoning as
milestone 1's `EarlyConflict`, are strict because real output never does the
thing, and each carries its own reason code so a disagreement with another
producer is localised in one line: a RAT step whose hint prefix already reaches
a conflict is rejected, and so are hints on a block that its own negation
already refutes.

Deletion is otherwise permissive — deleting an identifier that was never added
is counted under `--stats`, not rejected. Deletion only ever removes tools from
the checker, so a spurious one can cause a later rejection but can never cause a
false `VERIFIED`.

## Build and test

```
cargo build --release
cargo test
```

74 tests: 12 proofs that must verify, 21 corruption controls that must not, 25
boundary cases, 12 on the command line contract, 4 on the trust boundary.

Every corruption control was written and run before the rule that catches it
existed, and observed failing there. Milestone 1's were run against a `check()`
that returned `Verified` unconditionally; milestone 1b's were run against the
milestone-1 checker, which reported `s UNSUPPORTED` where a rejection was
required. The failing output is in the commit that introduced them. A rejection
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
