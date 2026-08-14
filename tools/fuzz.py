#!/usr/bin/env python3
"""Differential fuzzing of Refute's DRAT checker against `drat-trim -f`.

Not CI, and not a substitute for the committed corpus. The corpus pins one
example of each mutation class on every commit; this generates thousands of
them, on formulas nobody chose.

The verdict rules are the whole design of this harness:

    refute       drat-trim -f   meaning
    VERIFIED     NOT VERIFIED   HARD FAILURE: a false accept. Keep the
                                artefacts and stop the run
    NOT VERIFIED VERIFIED       Refute is stricter. Allowed only when its
                                reason is on the documented strict list below,
                                and counted either way
    same         same           pass

and three classes are unconditional, because for them rejection is a theorem
rather than an observation: **missing empty clause** and **truncated**, where
nothing was derived, and **satisfiable formula**, where a model exists. In
those, `s VERIFIED` is a hard failure whatever drat-trim says.

**The harness does not assert that every mutant is rejected**, and this is the
correction that matters most. Measured on 24 single-literal mutations of two
real proofs, five remain valid proofs and `drat-trim -f` verifies them: the
flip landed in a lemma nothing later depends on. An assertion that every mutant
is rejected would be red on correct behaviour, and would be weakened by whoever
hit it first. So harmless mutants are counted and the rate reported -- and a
rate that suddenly goes to zero is itself a signal that the mutator has stopped
mutating.

Locations are never written into a tracked file:

    KISSAT=... DRAT_TRIM=... REFUTE=... tools/fuzz.py --cases 10000

Deterministic. `--seed S --cases N` is reproducible, and any single case is
reproducible on its own with `--case K`.

`--force-compaction` runs Refute with its arena compaction floor at zero.
Milestone 3 made the clause store reclaim what it holds, and the whole safety
claim for that is that it changes what is held and nothing that is decided --
which is exactly what this harness is shaped to test. Without the flag the
random proofs are too small and delete too little to reach the new code at all,
so the gate is re-run under it rather than quoted from before.
"""
import argparse
import os
import random
import subprocess
import sys
import tempfile

# Reasons Refute may give when drat-trim -f verified the same file. Anything
# else in that direction is a hard failure: it means Refute is stricter for a
# reason nobody wrote down, which is how a checker acquires a false rejection
# nobody can explain.
STRICT_LIST = (
    # drat-trim adds the empty clause itself once the formula propagates to a
    # conflict. Refute requires the proof to derive it. Nothing in the file
    # derived it, so this is the safe direction.
    "proof contains no empty clause",
    # Refute honours a deletion of a unit clause; drat-trim ignores one, to
    # protect the root-level trail it keeps across steps. Refute keeps no such
    # trail, so honouring it costs nothing.
    "not implied by unit propagation",
    "hints ran out without reaching a conflict",
)

# Classes where rejection is a theorem, so drat-trim's opinion does not enter
# into it.
UNCONDITIONAL = ("truncated", "no_empty_clause", "satisfiable_formula")


def write_cnf(path, num_vars, clauses):
    with open(path, "w", newline="\n") as handle:
        handle.write("p cnf %d %d\n" % (num_vars, len(clauses)))
        for clause in clauses:
            handle.write(" ".join(str(lit) for lit in clause) + " 0\n")


def instance(rng, case):
    """One small random formula. Every seventeenth is a pigeonhole; every
    twenty-third carries a deliberate duplicate clause and a unit clause."""
    if case % 17 == 0:
        holes = rng.randint(2, 4)
        pigeons = holes + 1

        def var(pigeon, hole):
            return pigeon * holes + hole + 1

        clauses = [[var(p, h) for h in range(holes)] for p in range(pigeons)]
        for hole in range(holes):
            for first in range(pigeons):
                for second in range(first + 1, pigeons):
                    clauses.append([-var(first, hole), -var(second, hole)])
        return pigeons * holes, clauses

    num_vars = rng.randint(6, 24)
    width = rng.choice([3, 3, 3, 4])
    ratio = rng.uniform(3.6, 5.2)
    count = max(1, int(num_vars * ratio))
    clauses = []
    for _ in range(count):
        picked = rng.sample(range(1, num_vars + 1), min(width, num_vars))
        clauses.append([v if rng.random() < 0.5 else -v for v in picked])
    if case % 23 == 0 and clauses:
        clauses.append(list(clauses[0]))
        clauses.append([rng.randint(1, num_vars)])
    return num_vars, clauses


def parse_drat(text):
    steps = []
    for raw in text.splitlines():
        tokens = raw.split()
        if not tokens:
            continue
        if tokens[0] == "d":
            steps.append(("d", [int(t) for t in tokens[1:-1]]))
        else:
            steps.append(("a", [int(t) for t in tokens[:-1]]))
    return steps


def render_drat(steps):
    out = []
    for kind, lits in steps:
        head = ["d"] if kind == "d" else []
        out.append(" ".join(head + [str(lit) for lit in lits] + ["0"]))
    return "\n".join(out) + "\n"


def mutants(rng, steps):
    """One mutant per class, as (class name, steps). Some are identical to the
    clean proof when the proof is too short to mutate; those are skipped."""
    adds = [i for i, s in enumerate(steps) if s[0] == "a"]
    out = []
    if len(adds) > 1:
        drop = rng.choice(adds[:-1])
        out.append(("dropped_line", steps[:drop] + steps[drop + 1:]))
    if len(adds) > 2:
        first, second = sorted(rng.sample(adds[:-1], 2))
        swapped = list(steps)
        swapped[first], swapped[second] = swapped[second], swapped[first]
        out.append(("reordered_lines", swapped))
    flippable = [i for i in adds if steps[i][1]]
    if flippable:
        at = rng.choice(flippable)
        lits = steps[at][1]
        where = rng.randrange(len(lits))
        flipped = list(steps)
        flipped[at] = ("a", lits[:where] + [-lits[where]] + lits[where + 1:])
        out.append(("flipped_literal", flipped))
    if len(adds) > 1:
        at = rng.choice(adds[:-1])
        out.append((
            "deleted_then_used",
            steps[:at + 1] + [("d", steps[at][1])] + steps[at + 1:],
        ))
    # Both of these have to remove EVERY empty clause, not the last step and
    # not half the file. `kissat` does not always stop at the empty clause: on
    # fuzz case 92 of seed 20260814 it wrote two more additions after it, which
    # is a counterexample to G3 in docs/TDD.md part 3 ("the proof ends with a
    # bare 0, always the last") -- true of all nine real proofs measured there,
    # and not true in general. Cutting blindly leaves the empty clause in the
    # file, and the mutant then asserts the opposite of its own name.
    empty = [i for i, s in enumerate(steps) if s == ("a", [])]
    first_empty = empty[0] if empty else len(steps)
    if first_empty > 2:
        out.append(("truncated", steps[: min(len(steps) // 2, first_empty)]))
    out.append(("no_empty_clause", [s for s in steps if s != ("a", [])]))
    return out


def verdict(binary, cnf, proof, forward=False, extra=()):
    args = [binary, cnf, proof] + (["-f"] if forward else []) + list(extra)
    result = subprocess.run(args, capture_output=True, text=True)
    for line in result.stdout.splitlines():
        if line.startswith("s VERIFIED"):
            return True, result.stderr
        if line.startswith("s "):
            return False, result.stderr
    return False, result.stderr


class Run:
    def __init__(self, args):
        self.args = args
        self.checked = 0
        self.harmless = 0
        self.strict = 0
        self.compacted = 0
        self.failures = []

    def compare(self, case, kind, cnf, proof):
        """One comparison. Returns False on a hard failure."""
        ours, stderr = verdict(self.args.refute, cnf, proof,
                               extra=self.args.refute_flags)
        theirs, _ = verdict(self.args.drat_trim, cnf, proof, forward=True)
        self.checked += 1
        # How much of the run entered the code the flag exists to reach. A
        # proof that deletes nothing cannot compact whatever the floor is, and
        # random proofs often delete nothing -- measured, on the harness's own
        # instance shapes: one 150-deletion refutation compacted once and one
        # 0-deletion refutation did not. Reporting the fraction is the
        # difference between a gate and a claim about a gate.
        for field in stderr.split(","):
            if field.strip().endswith("compactions"):
                if field.strip().split()[0] != "0":
                    self.compacted += 1
                break

        if kind in UNCONDITIONAL:
            if ours:
                self.failures.append(
                    "case %d %s: refute VERIFIED where rejection is a theorem"
                    % (case, kind))
                return False
            return True
        if ours and not theirs:
            self.failures.append(
                "case %d %s: refute VERIFIED, drat-trim -f NOT VERIFIED" % (case, kind))
            return False
        if theirs and not ours:
            if not any(reason in stderr for reason in STRICT_LIST):
                self.failures.append(
                    "case %d %s: refute stricter for an undocumented reason: %s"
                    % (case, kind, stderr.strip()))
                return False
            self.strict += 1
        if kind != "clean" and ours and theirs:
            self.harmless += 1
        return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=int, default=1000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--case", type=int, help="run this one case and stop")
    parser.add_argument("--kissat", default=os.environ.get("KISSAT"))
    parser.add_argument("--drat-trim", dest="drat_trim",
                        default=os.environ.get("DRAT_TRIM"))
    parser.add_argument("--refute", default=os.environ.get("REFUTE"))
    parser.add_argument("--force-compaction", action="store_true",
                        help="run refute with max_dead_arena_lits = 0, so "
                             "that the arena compacts as soon as its dead "
                             "half is the larger one")
    args = parser.parse_args()
    for name in ("kissat", "drat_trim", "refute"):
        if not getattr(args, name):
            sys.exit("set KISSAT, DRAT_TRIM and REFUTE, or pass --%s"
                     % name.replace("_", "-"))
    # Random proofs are small and delete little, so at the default floor of
    # 1,024 dead literals almost none of them reach the reclamation code that
    # milestone 3 added -- and a harness that never enters the code it is
    # guarding reports the same summary whether that code works or not.
    # `--stats` rides along so the summary can report how many comparisons
    # actually compacted. It changes nothing about the verdict; it only makes
    # the counter line available on stderr, which this harness already reads.
    args.refute_flags = (["--max-dead-arena-lits=0", "--stats"]
                         if args.force_compaction else [])

    cases = [args.case] if args.case is not None else range(args.cases)
    ran = 0
    run = Run(args)
    work = tempfile.mkdtemp()
    cnf = os.path.join(work, "f.cnf")
    other = os.path.join(work, "g.cnf")
    proof = os.path.join(work, "p.drat")
    unsat_seen = 0
    sat_seen = 0
    held = None  # the last (formula bytes, proof bytes) that verified

    for case in cases:
        ran += 1
        rng = random.Random((args.seed, case).__hash__())
        num_vars, clauses = instance(rng, case)
        write_cnf(cnf, num_vars, clauses)
        solved = subprocess.run([args.kissat, "-q", cnf], capture_output=True, text=True)

        if solved.returncode == 10:
            sat_seen += 1
            # A formula with a model has no refutation. Any proof at all is a
            # control, so the last one that verified is reused.
            if held:
                with open(proof, "w", newline="\n") as handle:
                    handle.write(held)
                if not run.compare(case, "satisfiable_formula", cnf, proof):
                    break
            continue
        if solved.returncode != 20:
            continue
        unsat_seen += 1

        subprocess.run([args.kissat, "--no-binary", "-q", cnf, proof],
                       capture_output=True, text=True)
        with open(proof) as handle:
            clean = handle.read()
        held = clean
        steps = parse_drat(clean)
        if not steps:
            continue

        if not run.compare(case, "clean", cnf, proof):
            break
        stop = False
        for kind, mutated in mutants(rng, steps):
            with open(proof, "w", newline="\n") as handle:
                handle.write(render_drat(mutated))
            if not run.compare(case, kind, cnf, proof):
                stop = True
                break
        if stop:
            break

        # The wrong formula: the same proof against a transposition of two of
        # its own clauses.
        if len(clauses) > 1:
            transposed = list(clauses)
            transposed[0], transposed[1] = transposed[1], transposed[0]
            write_cnf(other, num_vars, transposed)
            with open(proof, "w", newline="\n") as handle:
                handle.write(clean)
            if not run.compare(case, "wrong_formula", other, proof):
                break

    # Cases RUN, not cases asked for: the loop stops at the first hard
    # failure, and reporting the request would overstate the evidence.
    print("refute flags    %s"
          % (" ".join(args.refute_flags) if args.refute_flags else "(none)"))
    print("cases           %d (%d unsatisfiable, %d satisfiable)"
          % (ran, unsat_seen, sat_seen))
    print("comparisons     %d" % run.checked)
    print("harmless mutants %d (%.1f%% of mutants still verified by both)"
          % (run.harmless,
             100.0 * run.harmless / max(1, run.checked - unsat_seen)))
    print("strict wins     %d (refute rejected, drat-trim -f verified, "
          "reason on the documented list)" % run.strict)
    if args.force_compaction:
        print("compacted       %d of %d comparisons (%.1f%%) really entered "
              "the arena compaction; the rest deleted too little to trigger it "
              "at any floor"
              % (run.compacted, run.checked,
                 100.0 * run.compacted / max(1, run.checked)))
    print("false accepts   %d" % len(run.failures))
    for line in run.failures:
        print("  " + line)
    return 1 if run.failures else 0


if __name__ == "__main__":
    sys.exit(main())
