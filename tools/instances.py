#!/usr/bin/env python3
"""Writes the DIMACS instances the fixture corpus is built from.

Kept separate from mutate.py so that the only thing reading a solver's output
is the generator, and the only thing editing it is the mutator.

Usage: instances.py <output-directory>
       instances.py --names

--names lists the instance names and writes nothing. gen_fixtures.sh uses it so
that the solver is only ever run on these instances, never on a fixture that a
previous run derived.
"""
import os
import sys


def write_cnf(path, num_vars, clauses):
    with open(path, "w", newline="\n") as handle:
        handle.write("p cnf %d %d\n" % (num_vars, len(clauses)))
        for clause in clauses:
            handle.write(" ".join(str(lit) for lit in clause) + " 0\n")


def all_assignments(num_vars):
    """Every clause over num_vars variables: unsatisfiable, and tiny."""
    clauses = []
    for mask in range(1 << num_vars):
        clauses.append([
            (var + 1) if (mask >> var) & 1 else -(var + 1)
            for var in range(num_vars)
        ])
    return num_vars, clauses


def pigeonhole(pigeons, holes):
    """Pigeons into holes. Unsatisfiable whenever pigeons > holes."""
    def var(pigeon, hole):
        return pigeon * holes + hole + 1

    clauses = [[var(p, h) for h in range(holes)] for p in range(pigeons)]
    for hole in range(holes):
        for first in range(pigeons):
            for second in range(first + 1, pigeons):
                clauses.append([-var(first, hole), -var(second, hole)])
    return pigeons * holes, clauses


def random_3sat(num_vars, num_clauses, seed):
    """Random 3-SAT just above the satisfiability threshold.

    The generator is an explicit linear congruential sequence rather than the
    `random` module, because `random.sample`'s internals are not a stability
    contract across Python versions and this corpus has to re-derive
    byte-identically years from now.
    """
    state = seed
    def step():
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return state >> 33

    seen = set()
    clauses = []
    while len(clauses) < num_clauses:
        picked = []
        while len(picked) < 3:
            var = step() % num_vars + 1
            if var not in picked:
                picked.append(var)
        clause = tuple(sorted(
            (-v if step() & 1 else v for v in picked), key=abs
        ))
        if clause in seen:
            continue
        seen.add(clause)
        clauses.append(list(clause))
    return num_vars, clauses


def implication_chain(length):
    """(1), (-1 2), ..., (-(n-1) n), (-n). Unsatisfiable by propagation alone."""
    clauses = [[1]]
    clauses.extend([-i, i + 1] for i in range(1, length))
    clauses.append([-length])
    return length, clauses


def resolvent_chain():
    """The one formula whose proof has a resolvent block that propagates.

    Every resolvent block in every proof measured for milestone 1b -- 703 of
    them -- is refuted by its own negation alone, so no real file exercises the
    hint walk inside a block. This formula is built so that one does.

    Lemma (1 3) is RAT on pivot 1. Clause 1 is the only clause holding -1, so
    it is the only resolution candidate, and the resolvent (1 3 2) needs three
    unit propagations before it conflicts: 2 -> 4 -> 5 -> the clause the lemma
    resolves back onto. The lemma is also RUP by full propagation, which is
    what lets drat-trim verify the same lemma sequence in DRAT form during
    generation -- the hint lists are hand-supplied, the verdict is not.
    """
    return 5, [[-1, 2], [2, 4], [-4, 5], [-5, 3, 1], [-2], [-3]]


INSTANCES = {
    # P1: the end-to-end happy path, small enough to read by eye.
    "tiny_unsat": all_assignments(3),
    # P5: 44 real addition steps, 43 deletions, and no unsupported construct.
    "deletes_originals": pigeonhole(4, 3),
    # B12: the smallest measured instance whose proof contains both an empty
    # hint list and a RAT resolvent block.
    "real_rat_proof": pigeonhole(5, 4),
    # P9: the same construct at scale. 624 additions, 42 RAT lines, 30 empty
    # hint lists, 108 resolvent blocks, 353 deletions. A checker that is subtly
    # over-strict about RAT still passes 5x4.
    "rat_pigeonhole": pigeonhole(7, 6),
    # P11: the formula behind the hand-built resolvent chain.
    "resolvent_propagates": resolvent_chain(),
    # P2: the formula behind the hand-built unit chain.
    "unit_chain": implication_chain(12),
    # P6: 980 real RUP lemmas and no unsupported construct anywhere. The other
    # positive fixtures are tens of steps; this one is the evidence that the
    # strict rules survive a proof of some size.
    "random_unsat": random_3sat(80, 370, 99),
}

if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    if sys.argv[1] == "--names":
        # Bypass text mode: print() emits CRLF on Windows, and a name carrying
        # a stray carriage return becomes a path that does not exist.
        sys.stdout.buffer.write(("\n".join(INSTANCES) + "\n").encode())
        sys.exit(0)
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    for name, (num_vars, clauses) in INSTANCES.items():
        write_cnf(os.path.join(out, name + ".cnf"), num_vars, clauses)
        print("wrote %s.cnf (%d vars, %d clauses)" % (name, num_vars, len(clauses)))
