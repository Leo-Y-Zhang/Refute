# Refute

An independent forward checker for DRAT and LRAT unsatisfiability proofs, in
Rust, with no dependencies.

A SAT solver answering UNSAT is an assertion, not a proof. Refute exists to be a
second opinion on the certificate — written separately from `drat-trim`, so that
a soundness bug in one is not a soundness bug in both.

## Read this before using it

**Refute checks two formats.** Text DRAT, as a solver writes it, with no hints
at all: every addition is checked by unit propagation from the live database,
and a step that is not RUP is checked as RAT against a candidate set Refute
builds itself. And text LRAT, as `drat-trim -L` writes it: RUP steps with
hints, RAT steps with resolvent blocks, and the empty hint list — which is not
an absence but a claim, that the lemma's pivot has no resolution candidate, and
which Refute accepts only after establishing that for itself.

The format is detected from the head of the proof, and the two grammars are
disjoint on every file in the corpus. Nothing is guessed silently: `--drat` and
`--lrat` force the choice, and a file forced into the wrong reader is rejected
rather than reinterpreted.

**Checking DRAT directly is what takes `drat-trim` out of the trust chain.**
Against an LRAT file, `drat-trim` is still the producer even when it is not the
checker. Against the raw proof the solver wrote, it is neither.

Milestone 1 checked the RUP steps alone, which on a measured pigeonhole 8x7
proof was 96 % of addition lines and, in practice, nothing: the first line it
could not check arrived on line 2 of almost every real file. Milestone 1b closed
that. The evidence is a differential run against `drat-trim` on proofs too large
to commit, most recently:

| instance | raw DRAT | LRAT | `drat-trim -f` vs `refute` | `drat-trim -L` vs `refute` |
|---|---:|---:|---|---|
| pigeonhole 4x3 | 1.6 KB | 1.4 KB | both `s VERIFIED` | both `s VERIFIED` |
| pigeonhole 5x4 | 2.1 KB | 3.2 KB | both `s VERIFIED` | both `s VERIFIED` |
| pigeonhole 6x5 | 6.4 KB | 10 KB | both `s VERIFIED` | both `s VERIFIED` |
| pigeonhole 7x6 | 22 KB | 55 KB | both `s VERIFIED` | both `s VERIFIED` |
| pigeonhole 8x7 | 153 KB | 386 KB | both `s VERIFIED` | both `s VERIFIED` |
| random 3-SAT, 60 vars | 6.9 KB | 7.3 KB | both `s VERIFIED` | both `s VERIFIED` |
| random 3-SAT, 80 vars | 79 KB | 97 KB | both `s VERIFIED` | both `s VERIFIED` |
| random 3-SAT, 100 vars | 21 KB | 68 KB | both `s VERIFIED` | both `s VERIFIED` |

The oracle is `drat-trim -f`, never its default backward mode. Backward
checking only checks the lemmas it keeps, so it verifies mutated proofs a
forward checker rejects, and is not a valid oracle for one.

Reproduce it with `tools/differential.sh`; it needs `kissat` and `drat-trim`,
which CI does not have, and every row above reproduces from this repository
alone. Certificates built elsewhere go in with `--extra <dir>`, which is how
the author's own are checked without this repository depending on another one.

**On this project's own results.** The refutations behind twenty-four new terms
of [A250026](https://oeis.org/A250026), checked from the raw proof the solver
wrote, with `drat-trim` in the chain neither as checker nor as producer:

| certificate | CNF | raw DRAT | `refute` | `drat-trim -f` |
|---|---:|---:|---|---|
| a(37) = 45 | 3.3 KB | 12.5 KB | `s VERIFIED` | `s VERIFIED` |
| a(47) = 57 | 9.4 KB | 192 KB | `s VERIFIED` | `s VERIFIED` |
| a(54) = 68 | 7.5 KB | 197 KB | `s VERIFIED` | `s VERIFIED` |
| a(59) = 72 | 7.8 KB | 173 KB | `s VERIFIED` | `s VERIFIED` |

And the whole published ladder of
[A217058](https://oeis.org/A217058), raw DRAT and the LRAT `drat-trim -L`
derives from it, both checkers on both files, all agreeing:

| rung | raw DRAT | LRAT | additions | peak live clauses |
|---|---:|---:|---:|---:|
| a(0), n=18 | 4.9 KB | 5.6 KB | 173 | 207 |
| a(2), n=25 | 111 KB | 257 KB | 2,388 | 1,448 |
| a(4), n=33 | 2.5 MB | 4.1 MB | 31,195 | 10,400 |
| a(6), n=40 | 15.6 MB | 22.5 MB | 154,759 | 21,354 |
| **a(7), n=42** | **87.5 MB** | **117.5 MB** | **763,382** | **40,631** |

The ladder is rebuilt rather than stored — 56 s of solver time — so it is a
gate that can be re-run, not a number that has to be believed.

**What is still not checked.** Binary proofs. `kissat` writes binary DRAT unless
told `--no-binary`, and handing one to a text checker is a common mistake, so it
has its own answer rather than a parse error:

```
$ refute formula.cnf proof.drat
s UNSUPPORTED
refute: proof line 1: this is a binary proof; refute reads text DRAT and text
LRAT. Re-run kissat with --no-binary
```

Also unchecked, and out of scope here: backward checking, trimming, core
extraction, binary LRAT, parallelism.

**A variable is not free on the DRAT path.** It costs about ninety-six bytes
there against about one on the LRAT path, because the watch and occurrence
vectors carry a slot per literal whether anything is pushed into it or not, so
that path stops at 2^22 variables rather than the shared 2^26. Measured peak
working set for a proof line naming one large variable: 393 MB just under the
ceiling, 5 MB just above it. The largest instance in this project's corpus
declares 1,209 variables.

**There is a memory budget, and it is measured rather than asserted.** For the
largest artefact this project holds — the raw 87.5 MB DRAT refutation behind
the a(7) rung of [A217058](https://oeis.org/A217058) — **at most 64 MB of peak
working set**, and at most 16 MB for the same refutation as LRAT. Measured by
polling the OS every 5 ms, on a release build:

| artefact | budget | measured |
|---|---:|---:|
| a(7) rung, raw `.drat`, 87.5 MB | 64 MB | **31.2 MB**, 51 s |
| a(7) rung, `.lrat`, 117.5 MB | 16 MB | **5.5 MB**, 2.1 s |
| `drat-trim -f`, same raw proof | *(not ours)* | 141.2 MB, 34 s |

The store holds the **live** clause database and a fixed 12 bytes per addition
ever made — never the proof's length. It was not always so: the same proof
peaked at 182.6 MB before this was measured, of which 170 MB was clauses the
proof had already deleted. `--stats` reports the figure on your own proof, so
this is checkable rather than quotable, and `tools/scale.sh` re-measures the
whole table.

Nothing above is in CI, which has no solver. What *is* in CI is the property
those numbers are consequences of, asserted as counters on committed fixtures:
at the end of a run the deletion index holds no more entries than the run's
peak live clause count, and the dead part of the arena is no larger than the
live part. Both were written before the code that satisfies them and observed
failing there — 1,063 index entries against 571 peak live clauses, and 7,596
dead arena bytes against 7,040 live.

**`s UNSUPPORTED` is not a pass.** It exits 2. A caller grepping for `VERIFIED`
would also match `NOT VERIFIED`, so test the exit code, never the string alone.

## Use

```
refute [check] <formula.cnf> <proof.lrat|proof.drat> [--drat|--lrat] [--stats]
```

The format is detected from the proof, so the two positional arguments are
usually all that is needed. `check` is optional and changes nothing; it is
there so that a subcommand spelling does not become a breaking change later.

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

On that route `drat-trim` is in the trust chain as the *producer* of the LRAT,
though not as the *checker*, which was the point. Handing Refute the solver's
own proof takes it out of the chain altogether:

```
kissat --no-binary formula.cnf proof.drat
refute formula.cnf proof.drat
```

which is the route the A250026 certificates above were checked by.

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
cargo test --workspace
```

165 tests: 13 proofs that must verify and 24 corruption controls that must not
on the LRAT path, 35 on the DRAT path, 26 boundary cases, 16 on what the clause
store holds, 13 on the command line contract, 8 on the trust boundary, 19 unit
tests inside the library, and 11 in the WebAssembly wrapper described below.

`--workspace` because there are two crates now, and the second one exists for a
reason worth reading: [the playground](#the-playground).

**N1–N12 and R1–R8 were written and run before the rule that catches them
existed, and observed failing there.** Milestone 1's were run against a
`check()` that returned `Verified` unconditionally; milestone 1b's were run
against the milestone-1 checker, which reported `s UNSUPPORTED` where a
rejection was required. The failing output is in the commit that introduced
them.

**The tests added after that point cannot claim it, and do not.** R9–R11, P12,
B22, the binary-proof mapping guard, the assertion that a pure-RUP proof never
scans for candidates, and the strengthened counter assertions were all written
against code that already worked. Each is justified by a weaker piece of
evidence instead, and by a specific one: the line of the rule it covers was
reverted, the suite was run, and the failing output is quoted in the commit that
added the test. A test that has never been seen red proves nothing, and saying
which kind of red it was seen in is the difference between evidence and a
slogan.

Neither does a test that passes with the defect it describes, which is why the
trail's unwinding is asserted by counting rather than by a stopwatch, and why
the escaping test runs a fixture carrying real escape bytes.

Minimum supported Rust is 1.74.0, and CI runs the whole suite on it, so that is
a measured claim rather than a hopeful one. Linters are pinned to 1.97.1.

### The fuzz gate

The corpus pins one example of each corruption class on every commit.
`tools/fuzz.py` generates thousands, on formulas nobody chose, and compares
every verdict against `drat-trim -f`:

```
KISSAT=... DRAT_TRIM=... REFUTE=... tools/fuzz.py --cases 10000 \
    --force-compaction
```

`--force-compaction` drops the clause store's compaction floor to zero, so the
arena is reclaimed as soon as its dead half is the larger one rather than
after a thousand dead literals. Random proofs are small and delete little, so
without it most of these cases never enter that code at all — and a harness
that does not enter the code it is guarding is decoration. It does not make
*every* case reach it either: a proof that deletes nothing has nothing to
reclaim at any floor, so the summary reports what fraction really compacted.
Measured at **12.6 %** — 463 of 3,685 comparisons — on a separate 1,200-case
run, since the counter was written after the gate below had started.

Most recent run — 10,000 cases at seed 20260814:

| | |
|---|---:|
| cases | 10,000 — 2,938 unsatisfiable, 7,062 satisfiable |
| comparisons | 30,564 |
| mutants that are still valid proofs | 10,768 (39.0 %) |
| Refute stricter, for a reason on the documented list | 257 |
| **false accepts** | **0** |

Three classes are unconditional, because for them rejection is a theorem and
not an observation: a proof with no empty clause, a truncated proof, and a
**satisfiable** formula. `s VERIFIED` on any of those is a hard failure
whatever `drat-trim` says, and the run stops there.

The harness deliberately does **not** assert that every mutant is rejected.
Two in five remain valid proofs — the flipped literal landed in a lemma
nothing later depends on — and an assertion that they all fail would be red on
correct behaviour and weakened by whoever hit it first. The rate is reported
instead, because a rate that suddenly goes to zero means the mutator stopped
mutating.

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

## The playground

**<https://leo-y-zhang.github.io/Refute/>** — drop a formula and a proof, get a
verdict. Nothing is installed and nothing is uploaded.

Published 2026-08-14, after the checks below passed against the artefact that
was deployed and then again against the deployment itself: five preloaded
examples driven in a real browser, each verdict read out of the page and equal
to the one the command-line tool gives, and every network request the page and
its worker made recorded from the DevTools protocol — twenty-one of them, all
on this page's own origin.

To run it from a clone instead:

```
rustup target add wasm32-unknown-unknown
cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm
node tools/serve_page.mjs
```

Then open the address it prints. `file://` will not do: a page cannot start a
worker or fetch a sibling from it.

`--lan` binds every interface and prints the addresses a phone on the same
network can reach, which is how the page gets measured on a phone before it is
published rather than after. Loopback is the default; opening a port to the
network should be something someone typed on purpose.

The module is **73,165 bytes**, has no dependencies, and **imports nothing at
all** — which is the mechanism behind the one claim the page makes about
itself. A module with no imports cannot call out, because there is nothing to
call: no fetch, no clock, no random source, no host function of any kind. Your
files stay in the tab. `node tools/wasm_shape.mjs` asserts that on every push,
and `tools/browser_check.mjs` records every request the page and its worker
make and fails if one leaves the origin.

The export boundary is a second crate, `refute-wasm`, and that is not
housekeeping. `unsafe_code = "forbid"` in this package blocks every form a
WebAssembly export can take — `#[no_mangle]`, `#[export_name]` and
`unsafe extern` alike, all three measured. The alternatives were to weaken a
property this README states, or to take a dependency that ships its own
`unsafe`. Instead the exports live one crate away, `refute` is not edited at
all, and the wrapper contains **zero `unsafe` blocks** of its own: it hands
JavaScript an offset and JavaScript dereferences it, which is JavaScript's
business. `tests/trust_boundary.rs` asserts both halves.

The page checks the same way the CLI does, and that is tested rather than
asserted:

```
cargo build
node tools/wasm_agreement.mjs
```

Every committed formula-and-proof pair goes through the module and through the
native binary, and the two verdicts must match — 67 pairs, all three verdicts,
on every push and on both toolchains. The expectations are not written down
anywhere: they come from the binary, at run time. Add `--extra <dir>` to check
certificates too large to commit. The A217058 a(4) rung, 2,508,578 bytes, takes
**8.4 MB of linear memory and 0.85 s** in the browser.

### What it refuses, and why it is not a bug

The page refuses any file over **32 MB**, before it instantiates anything. A
tab has to hold the whole proof in memory before the checker can read a byte of
it, where the CLI streams it and holds one line; `memory.grow` has no inverse,
so a transient peak is permanent for the life of the instance. The refusal is a
decision taken with the file size in hand — one byte over the ceiling allocates
nothing at all — and it names the exact `refute` command instead of offering a
retry, because a retry after an out-of-memory is a lie.

32 MB was measured on a desktop, and no phone has been measured. So the page
does not rely on the ceiling being right: if a device cannot hold the files, the
reserve fails, and that failure is caught and shown as *Not enough memory on
this device* with the command to run instead. A wrong ceiling is a legible
failure rather than a blank tab.

WebAssembly costs about a fifth in speed, not an order of magnitude: 1.21x
native on the largest artefact measured.

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

Proprietary source-available — see [LICENCE](LICENCE). You may read it, run it, and publish what you find, including a refutation. No reuse, modification, redistribution, or use as machine-learning training data.
