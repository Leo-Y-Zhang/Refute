#!/usr/bin/env python3
"""Derives the negative and boundary fixtures from the real ones.

Every mutation here is deterministic: run it twice and you get the same bytes.
That matters because the committed fixtures are what CI checks, and a fixture
that drifts is a test that quietly changes what it asserts.

Three fixtures cannot come from a solver and are built here instead. Each says
why in a comment at its construction site:

  unit_chain          drat-trim's backward checking trims a chain of unit
                      lemmas down to a single step, so a proof whose steps are
                      all unit lemmas cannot survive it. The same lemma
                      sequence in DRAT form is verified by drat-trim during
                      generation, so only the hint lists are hand-supplied.
  taut_lemma          no solver emits a tautological lemma, and backward
                      checking would trim it if one did.
  empty_clause_in_cnf drat-trim reports 'trivial UNSAT' and emits an empty
                      LRAT file, so there is nothing to capture.

Usage: mutate.py --fixtures DIR [--kissat PATH]
"""
import argparse
import os
import subprocess
import sys

ADD = "a"
DELETE = "d"


# ---------------------------------------------------------------- LRAT model


def parse_lrat(text):
    """Returns a list of ('d', step, ids) and ('a', id, lits, hints) records."""
    steps = []
    for raw in text.splitlines():
        tokens = raw.split()
        if not tokens:
            continue
        if len(tokens) > 1 and tokens[1] == "d":
            steps.append((DELETE, int(tokens[0]), [int(t) for t in tokens[2:-1]]))
            continue
        end_of_lits = tokens.index("0", 1)
        lits = [int(t) for t in tokens[1:end_of_lits]]
        hints = [int(t) for t in tokens[end_of_lits + 1:-1]]
        steps.append((ADD, int(tokens[0]), lits, hints))
    return steps


def render(steps):
    out = []
    for step in steps:
        if step[0] == DELETE:
            out.append(" ".join([str(step[1]), "d"] + [str(i) for i in step[2]] + ["0"]))
        else:
            _, ident, lits, hints = step
            body = [str(ident)] + [str(l) for l in lits] + ["0"]
            body += [str(h) for h in hints] + ["0"]
            out.append(" ".join(body))
    return "\n".join(out) + "\n"


def live_ids(steps, upto, num_clauses):
    """Clause ids present in the database just before steps[upto]."""
    live = set(range(1, num_clauses + 1))
    for step in steps[:upto]:
        if step[0] == DELETE:
            live.difference_update(step[2])
        else:
            live.add(step[1])
    return live


def additions(steps):
    return [i for i, s in enumerate(steps) if s[0] == ADD]


def clause_count(cnf_text):
    return sum(
        1
        for line in cnf_text.splitlines()
        if line.strip() and not line.startswith(("c", "p", "%"))
    )


# ------------------------------------------------------------------- helpers


def write(path, text, newline="\n"):
    with open(path, "w", newline=newline) as handle:
        handle.write(text)
    print("wrote %s (%d bytes)" % (os.path.basename(path), os.path.getsize(path)))


def read(path):
    with open(path) as handle:
        return handle.read()


# ---------------------------------------------------- hand-built positives


def build_unit_chain(out, length=12):
    """P2. Formula is (1), (-1 2), ..., (-(n-1) n), (-n): n + 1 clauses.

    Lemma (k) is derived from lemma (k-1) and clause k, so every step is a unit
    lemma and the trail is pushed and unwound n times rather than once.
    """
    num_clauses = length + 1
    steps = []
    previous = 1  # clause (1) seeds the chain
    for k in range(2, length + 1):
        ident = num_clauses + k - 1
        steps.append((ADD, ident, [k], [previous, k]))
        previous = ident
    steps.append((ADD, num_clauses + length, [], [previous, num_clauses]))
    write(os.path.join(out, "unit_chain.lrat"), render(steps))


def build_taut_lemma(out, base_cnf, base_lrat):
    """P3. A tautological lemma spliced into a real proof, before its last step.

    Adding x or not-x preserves satisfiability, so accepting it is sound, and it
    can never be the empty clause. Nothing later refers to it, so the rest of
    the real proof is untouched.
    """
    steps = parse_lrat(read(base_lrat))
    last = additions(steps)[-1]
    previous_add = additions(steps)[-2]
    ident = steps[last][1] - 1
    assert ident > steps[previous_add][1], "no free id for the tautology"
    taut = (ADD, ident, [1, -1], [steps[previous_add][1]])
    steps.insert(last, taut)
    write(os.path.join(out, "taut_lemma.cnf"), read(base_cnf))
    write(os.path.join(out, "taut_lemma.lrat"), render(steps))


def build_empty_clause_in_cnf(out):
    """P4. A formula that already contains the empty clause, refuted in one step."""
    write(os.path.join(out, "empty_clause_in_cnf.cnf"), "p cnf 2 3\n1 0\n-1 2 0\n0\n")
    write(os.path.join(out, "empty_clause_in_cnf.lrat"), "4 0 3 0\n")


# ------------------------------------------------------------- negatives


def build_negatives(out, base_name, kissat):
    cnf_text = read(os.path.join(out, base_name + ".cnf"))
    lrat_text = read(os.path.join(out, base_name + ".lrat"))
    steps = parse_lrat(lrat_text)
    num_clauses = clause_count(cnf_text)
    adds = additions(steps)

    def emit(tag, steps_out=None, cnf_out=None):
        write(os.path.join(out, tag + ".cnf"), cnf_out if cnf_out else cnf_text)
        write(os.path.join(out, tag + ".lrat"),
              steps_out if isinstance(steps_out, str) else render(steps_out))

    # N1: one hint redirected to a different clause that is live at that point.
    for index in adds:
        _, ident, lits, hints = steps[index]
        if not hints:
            continue
        live = live_ids(steps, index, num_clauses)
        target = next((h + 1 for h in hints if h + 1 in live and h + 1 not in hints), None)
        if target is None:
            continue
        moved = list(steps)
        new_hints = [target if h == target - 1 else h for h in hints]
        moved[index] = (ADD, ident, lits, new_hints)
        emit("n01_hint_redirected", moved)
        break

    # N2: the last hint of the final step removed.
    last = adds[-1]
    _, ident, lits, hints = steps[last]
    trimmed = list(steps)
    trimmed[last] = (ADD, ident, lits, hints[:-1])
    emit("n02_last_hint_dropped", trimmed)

    # N3: a hint redirected at a clause deleted earlier in the proof.
    deleted_before = sorted(
        set(i for step in steps[:last] if step[0] == DELETE for i in step[2])
    )
    stale = deleted_before[0]
    redirected = list(steps)
    redirected[last] = (ADD, ident, lits, [stale] + hints[1:])
    emit("n03_hint_deleted_clause", redirected)

    # N4: one literal of a lemma flipped.
    first_with_lits = next(i for i in adds if steps[i][2])
    _, fid, flits, fhints = steps[first_with_lits]
    flipped = list(steps)
    flipped[first_with_lits] = (ADD, fid, [-flits[0]] + flits[1:], fhints)
    emit("n04_lemma_literal_flipped", flipped)

    # N5: the final empty clause removed.
    emit("n05_no_empty_clause", steps[:last] + steps[last + 1:])

    # N6: the proof truncated to its first half.
    emit("n06_truncated", steps[: len(steps) // 2])

    # N7: the hint list of the final step reversed.
    reversed_hints = list(steps)
    reversed_hints[last] = (ADD, ident, lits, list(reversed(hints)))
    emit("n07_hints_reversed", reversed_hints)

    # N8: a deletion moved to before the step that uses the clause.
    move = None
    for add_index in adds:
        if steps[add_index][0] != ADD:
            continue
        for hint in steps[add_index][3]:
            for del_index in range(add_index + 1, len(steps)):
                if steps[del_index][0] == DELETE and hint in steps[del_index][2]:
                    move = (add_index, del_index)
                    break
            if move:
                break
        if move:
            break
    assert move, "no deletion in this proof follows a use of the clause"
    add_index, del_index = move
    moved = list(steps)
    line = moved.pop(del_index)
    moved.insert(add_index, line)
    emit("n08_deletion_moved_early", moved)

    # N9: the same proof against a different formula, two clauses transposed.
    lines = cnf_text.splitlines()
    body = [i for i, l in enumerate(lines) if l.strip() and not l.startswith(("c", "p"))]
    swapped = list(lines)
    swapped[body[0]], swapped[body[1]] = swapped[body[1]], swapped[body[0]]
    emit("n09_different_formula", lrat_text, "\n".join(swapped) + "\n")

    # N10: the same proof against a SATISFIABLE formula. The control that
    # matters most: a pipeline that passes here certifies a false upper bound.
    sat_cnf = find_satisfiable_variant(cnf_text, kissat, out)
    emit("n10_satisfiable_formula", lrat_text, sat_cnf)

    # N11: the ids of the last two additions transposed.
    previous_add = adds[-2]
    non_monotonic = list(steps)
    first_id = steps[previous_add][1]
    second_id = steps[last][1]
    non_monotonic[previous_add] = (ADD, second_id) + steps[previous_add][2:]
    non_monotonic[last] = (ADD, first_id) + steps[last][2:]
    emit("n11_non_monotonic_ids", non_monotonic)

    # N12: a bare empty clause with no hints. Unsupported, exit 2, never 0.
    emit("n12_bare_empty_clause", "%d 0 0\n" % (num_clauses + 1))


def find_satisfiable_variant(cnf_text, kissat, out):
    """Flips the sign of one literal, in file order, until the formula is SAT.

    A deterministic search rather than a hand-picked edit, so the fixture can be
    re-derived and the claim 'this formula is satisfiable' is a solver's, not
    mine.
    """
    if not kissat:
        sys.exit("N10 needs --kissat: the fixture asserts a formula is satisfiable")
    lines = cnf_text.splitlines()
    probe = os.path.join(out, ".probe.cnf")
    for index, line in enumerate(lines):
        if not line.strip() or line.startswith(("c", "p", "%")):
            continue
        tokens = line.split()
        for position in range(len(tokens) - 1):
            candidate = list(tokens)
            candidate[position] = str(-int(tokens[position]))
            trial = list(lines)
            trial[index] = " ".join(candidate)
            text = "\n".join(trial) + "\n"
            with open(probe, "w", newline="\n") as handle:
                handle.write(text)
            result = subprocess.run([kissat, "-q", probe], capture_output=True, text=True)
            if result.returncode == 10:
                os.remove(probe)
                print("N10: satisfiable after flipping token %d of line %d"
                      % (position, index + 1))
                return text
    sys.exit("N10: no single-literal flip made the formula satisfiable")


# -------------------------------------------------------------- boundaries


def build_boundaries(out, tiny_cnf_path, tiny_lrat_path, rat_lrat_path):
    tiny_cnf = read(tiny_cnf_path)
    tiny_lrat = read(tiny_lrat_path)

    # B1: a proof file of zero bytes.
    write(os.path.join(out, "b01_empty_proof.cnf"), tiny_cnf)
    write(os.path.join(out, "b01_empty_proof.lrat"), "")

    # B2: an empty formula and an empty proof.
    write(os.path.join(out, "b02_empty_cnf.cnf"), "p cnf 0 0\n")
    write(os.path.join(out, "b02_empty_cnf.lrat"), "")

    # B3: the header understates the variable count.
    write(os.path.join(out, "b03_header_undercount.cnf"),
          tiny_cnf.replace("p cnf 3 8", "p cnf 2 8", 1))
    write(os.path.join(out, "b03_header_undercount.lrat"), tiny_lrat)

    # B4: the header overstates the clause count.
    write(os.path.join(out, "b04_header_overcount.cnf"),
          tiny_cnf.replace("p cnf 3 8", "p cnf 3 99", 1))
    write(os.path.join(out, "b04_header_overcount.lrat"), tiny_lrat)

    # B5: an integer no machine word holds.
    write(os.path.join(out, "b05_huge_literal.cnf"),
          "p cnf 3 8\n99999999999999999999 0\n")
    write(os.path.join(out, "b05_huge_literal.lrat"), tiny_lrat)

    # B6: a literal past the default variable ceiling of 2^26.
    write(os.path.join(out, "b06_var_over_limit.cnf"), "p cnf 3 8\n100000000 0\n")
    write(os.path.join(out, "b06_var_over_limit.lrat"), tiny_lrat)

    # B7: one clause across five lines, comments interleaved.
    body = tiny_cnf.splitlines()
    spread = [body[0], "c a comment before the first clause",
              "-1", "c a comment inside a clause", "-2", "", "-3", "0"]
    spread += body[2:]
    write(os.path.join(out, "b07_split_clause.cnf"), "\n".join(spread) + "\n")
    write(os.path.join(out, "b07_split_clause.lrat"), tiny_lrat)

    # B8: CRLF throughout. The fixtures are generated on Windows; without this
    # the first Linux-only reader of a CRLF proof finds out the hard way.
    write(os.path.join(out, "b08_crlf.cnf"), tiny_cnf, newline="\r\n")
    write(os.path.join(out, "b08_crlf.lrat"), tiny_lrat, newline="\r\n")

    # B9: the terminator missing from the last step.
    lines = tiny_lrat.rstrip("\n").splitlines()
    lines[-1] = lines[-1].rsplit(" 0", 1)[0]
    write(os.path.join(out, "b09_missing_terminator.cnf"), tiny_cnf)
    write(os.path.join(out, "b09_missing_terminator.lrat"), "\n".join(lines) + "\n")

    # B10: a deletion of an id that was never added. Permissive by design:
    # deletion only removes tools from the checker, so it cannot cause a false
    # VERIFIED, and being strict would reject other producers' output.
    steps = parse_lrat(tiny_lrat)
    with_unknown = [steps[0], (DELETE, steps[0][1], [9999])] + steps[1:]
    write(os.path.join(out, "b10_unknown_deletion.cnf"), tiny_cnf)
    write(os.path.join(out, "b10_unknown_deletion.lrat"), render(with_unknown))

    # B11: the same id deleted twice.
    first_delete = next(s for s in steps if s[0] == DELETE and s[2])
    twice = list(steps)
    twice.insert(len(steps) - 1, first_delete)
    write(os.path.join(out, "b11_double_deletion.cnf"), tiny_cnf)
    write(os.path.join(out, "b11_double_deletion.lrat"), render(twice))

    # B12b: a single RAT resolvent block, copied verbatim from the real proof.
    #
    # B12 itself is the whole real proof, which reports the empty hint list on
    # its second line: in every instance measured (4x3 through 8x7) the empty
    # hint list precedes the first RAT block, because the RAT blocks resolve
    # against those very lemmas. So the RatHints path needs its own fixture, or
    # it is never exercised.
    rat_line = next(
        line for line in read(rat_lrat_path).splitlines()
        if line.split()[1:2] != ["d"]
        and any(t.startswith("-") for t in line.split()[line.split().index("0", 1) + 1:-1])
    )
    write(os.path.join(out, "b12b_rat_hints.cnf"),
          read(os.path.join(out, "real_rat_proof.cnf")))
    write(os.path.join(out, "b12b_rat_hints.lrat"), rat_line + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--kissat", default=os.environ.get("KISSAT"))
    args = parser.parse_args()
    out = args.fixtures

    build_unit_chain(out)
    build_taut_lemma(out,
                     os.path.join(out, "deletes_originals.cnf"),
                     os.path.join(out, "deletes_originals.lrat"))
    build_empty_clause_in_cnf(out)
    build_negatives(out, "deletes_originals", args.kissat)
    build_boundaries(out,
                     os.path.join(out, "tiny_unsat.cnf"),
                     os.path.join(out, "tiny_unsat.lrat"),
                     os.path.join(out, "real_rat_proof.lrat"))


if __name__ == "__main__":
    main()
