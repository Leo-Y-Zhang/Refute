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


def implication_chain(length):
    """(1), (-1 2), ..., (-(n-1) n), (-n). Unsatisfiable by propagation alone."""
    clauses = [[1]]
    clauses.extend([-i, i + 1] for i in range(1, length))
    clauses.append([-length])
    return length, clauses


INSTANCES = {
    # P1: the end-to-end happy path, small enough to read by eye.
    "tiny_unsat": all_assignments(3),
    # P5: 44 real addition steps, 43 deletions, and no unsupported construct.
    "deletes_originals": pigeonhole(4, 3),
    # B12: the smallest measured instance whose proof contains both an empty
    # hint list and a RAT resolvent block.
    "real_rat_proof": pigeonhole(5, 4),
    # P2: the formula behind the hand-built unit chain.
    "unit_chain": implication_chain(12),
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
