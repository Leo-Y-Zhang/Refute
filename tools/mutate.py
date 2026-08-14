#!/usr/bin/env python3
"""Derives the negative and boundary fixtures from the real ones.

Every mutation here is deterministic: run it twice and you get the same bytes.
That matters because the committed fixtures are what CI checks, and a fixture
that drifts is a test that quietly changes what it asserts.

Eight fixtures cannot come from a solver and are built here instead. Each says
why in a comment at its construction site:

  resolvent_propagates every resolvent block in every proof measured is refuted
                      by the negation of its own resolvent, so no real file
                      exercises the hint walk inside a block. The same two
                      lemmas in DRAT form are verified by drat-trim during
                      generation; only the hint lists are hand-supplied.
  unit_chain          drat-trim's backward checking trims a chain of unit
                      lemmas down to a single step, so a proof whose steps are
                      all unit lemmas cannot survive it. The same lemma
                      sequence in DRAT form is verified by drat-trim during
                      generation, so only the hint lists are hand-supplied.
  taut_lemma          no solver emits a tautological lemma, and backward
                      checking would trim it if one did.
  empty_clause_in_cnf drat-trim reports 'trivial UNSAT' and emits an empty
                      LRAT file, so there is nothing to capture.
  dup_literal         no solver writes the same literal twice in one clause,
                      and drat-trim's output would not preserve it if one did.
                      The formula is edited by one literal; the proof is the
                      real one, unchanged, and drat-trim verifies it against
                      the edited formula during generation.
  r09, r10, r11       three rejection rules whose shape no real proof carries,
                      so no mutation of one reaches them. kissat is run on each
                      formula during generation, so whether it is satisfiable
                      is a solver's claim; r11's lemma sequence is checked by
                      drat-trim in DRAT form as well, because it is a valid
                      proof that Refute rejects on strictness alone.

Milestone 2 adds a DRAT section at the end. Its mutants are chosen by search
rather than by hand: `drat-trim -f` is run on every candidate and the first --
or last, where the fixture wants a later step -- that it rejects is the one
kept. That is not fussiness. Of 24 single-literal mutations of two real proofs
measured for docs/TDD.md part 3, five remain valid proofs, and a fixture named
"rejected" that is not rejected is a test asserting the opposite of its name.

Usage: mutate.py --fixtures DIR [--kissat PATH] [--drat-trim PATH]
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


def split_hints(hints):
    """Splits a flat hint list into (prefix, [(clause, hints), ...]).

    A negative identifier opens a resolvent block and every positive one after
    it belongs to that block; the positives before the first negative are the
    prefix. That is the grammar in docs/TDD.md part 2, and it is the only place
    in this file that knows it.
    """
    prefix = []
    blocks = []
    for hint in hints:
        if hint < 0:
            blocks.append((-hint, []))
        elif blocks:
            blocks[-1][1].append(hint)
        else:
            prefix.append(hint)
    return prefix, blocks


def join_hints(prefix, blocks):
    flat = list(prefix)
    for clause, hints in blocks:
        flat.append(-clause)
        flat.extend(hints)
    return flat


def first_with_blocks(steps):
    for index in additions(steps):
        if any(h < 0 for h in steps[index][3]):
            return index
    raise AssertionError("no addition in this proof carries a resolvent block")


def first_without_hints(steps):
    for index in additions(steps):
        if not steps[index][3]:
            return index
    raise AssertionError("no addition in this proof has an empty hint list")


def clause_count(cnf_text):
    return sum(
        1
        for line in cnf_text.splitlines()
        if line.strip() and not line.startswith(("c", "p", "%"))
    )


def parse_cnf_clauses(cnf_text):
    """Returns (line index, literals) for every clause, one clause per line.

    Only the generated instances are read here, and instances.py writes one
    clause per line. A file that does not is rejected rather than guessed at.
    """
    clauses = []
    for index, line in enumerate(cnf_text.splitlines()):
        if not line.strip() or line.startswith(("c", "p", "%")):
            continue
        tokens = line.split()
        assert tokens[-1] == "0", "clause on line %d does not end its line" % (index + 1)
        clauses.append((index, [int(t) for t in tokens[:-1]]))
    return clauses


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


def build_resolvent_propagates(out):
    """P11. The one proof whose resolvent block has to propagate.

    All 703 resolvent blocks measured across the eleven proofs behind
    docs/TDD.md part 2 are refuted by the negation of their own resolvent, with
    an empty hint list. So no real file exercises the hint walk inside a block,
    and that path would ship uncovered.

    Formula (see tools/instances.py):
        1: (-1 2)   2: (2 4)   3: (-4 5)   4: (-5 3 1)   5: (-2)   6: (-3)

    Lemma 7 is (1 3), pivot 1. Clause 1 is the only clause holding -1, so it is
    the only resolution candidate; the prefix is empty, so the block starts
    from the negated lemma alone. Resolving -1 away leaves 2, whose negation
    propagates 4, then 5, and clause 4 is then falsified -- three hints, with
    the conflict on the last. Step 8 derives the empty clause by RUP.

    Only the hint lists are hand-supplied. gen_fixtures.sh verifies the same
    two lemmas in DRAT form against the same formula, so the claim that they
    refute it is drat-trim's.
    """
    steps = [
        (ADD, 7, [1, 3], [-1, 2, 3, 4]),
        (ADD, 8, [], [5, 6, 7, 1]),
    ]
    write(os.path.join(out, "resolvent_propagates.lrat"), render(steps))


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
    """P4 and P18. A formula that already contains the empty clause.

    Refuted in one step, and the DRAT form of that step is the shortest proof
    there is: two bytes. It is also the only route to the store's empty-clause
    counter, since no solver is ever handed a formula this trivial.
    """
    write(os.path.join(out, "empty_clause_in_cnf.cnf"), "p cnf 2 3\n1 0\n-1 2 0\n0\n")
    write(os.path.join(out, "empty_clause_in_cnf.lrat"), "4 0 3 0\n")
    write(os.path.join(out, "empty_clause_in_cnf.drat"), "0\n")


def build_duplicate_literal(out, base_cnf, base_lrat):
    """P7. A real formula with one literal written twice, and its real proof.

    A repeated literal is the same literal: `1 2 -3 -3` is the clause `1 2 -3`,
    so the proof is untouched by the edit -- hints are positions, and the
    clause's meaning has not changed. Solvers do not emit a repeat, but
    hand-written and machine-generated formulas do, and a checker that counts
    free literals rather than distinct ones calls that clause non-unit and
    rejects a valid proof. gen_fixtures.sh verifies the same lemma sequence in
    DRAT form against this formula, so the claim that the proof is valid is
    drat-trim's rather than mine.

    The clause and the literal are found rather than chosen. Under the negated
    literals of the first addition's lemma, a clause literal is falsified
    exactly when it appears in that lemma, so the first hint's remaining
    literal is the one the step propagates -- and that is the one written
    twice. Duplicating a literal the proof only ever falsifies would prove
    nothing: a falsified duplicate is still falsified.
    """
    cnf_text = read(base_cnf)
    lrat_text = read(base_lrat)
    clauses = parse_cnf_clauses(cnf_text)
    step = next(
        s for s in parse_lrat(lrat_text) if s[0] == ADD and s[2] and len(s[3]) > 1
    )
    _, _, lits, hints = step
    line_index, clause = clauses[hints[0] - 1]
    free = [lit for lit in clause if lit not in lits]
    assert len(free) == 1, "the first hint is not a unit propagation: %r" % (free,)

    doubled = []
    for lit in clause:
        doubled.append(lit)
        if lit == free[0]:
            doubled.append(lit)
    lines = cnf_text.splitlines()
    lines[line_index] = " ".join(str(lit) for lit in doubled) + " 0"
    write(os.path.join(out, "dup_literal.cnf"), "\n".join(lines) + "\n")
    write(os.path.join(out, "dup_literal.lrat"), lrat_text)


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


def build_rat_negatives(out, base_name):
    """R1-R3 and R5-R8: one mutation per new rejection rule in part 2.

    Every choice below is "the first one that qualifies, in file order", so a
    re-run produces the same bytes and the fixture's meaning does not drift
    when the base proof is regenerated.
    """
    cnf_text = read(os.path.join(out, base_name + ".cnf"))
    steps = parse_lrat(read(os.path.join(out, base_name + ".lrat")))
    num_clauses = clause_count(cnf_text)

    def emit(tag, steps_out):
        write(os.path.join(out, tag + ".cnf"), cnf_text)
        write(os.path.join(out, tag + ".lrat"),
              steps_out if isinstance(steps_out, str) else render(steps_out))

    rat = first_with_blocks(steps)
    _, rat_id, rat_lits, rat_hints = steps[rat]
    rat_prefix, rat_blocks = split_hints(rat_hints)
    assert len(rat_lits) > 1, "the first RAT lemma is a unit; nothing to swap"

    # R1: the first two literals of a RAT lemma swapped. The pivot is the first
    # literal as written, so this is the wrong-pivot mutation -- and it is the
    # one a checker reading the pivot after normalisation fails on, because
    # normalise sorts.
    swapped = list(steps)
    swapped[rat] = (ADD, rat_id, [rat_lits[1], rat_lits[0]] + rat_lits[2:], rat_hints)
    emit("r01_wrong_pivot", swapped)

    # R2: the last resolvent block, and its hints, deleted. The candidate it
    # covered is then uncovered, which is the mutation this milestone exists to
    # catch: every real block is refuted by its own negation, so a checker that
    # skipped trivially refuted candidates could not tell this from the truth.
    dropped = list(steps)
    dropped[rat] = (ADD, rat_id, rat_lits, join_hints(rat_prefix, rat_blocks[:-1]))
    emit("r02_block_dropped", dropped)

    # R3: a block redirected to a clause deleted earlier in the proof. The
    # first RAT addition with anything deleted before it, redirected to the
    # smallest such identifier.
    target = None
    for index in additions(steps):
        if not any(h < 0 for h in steps[index][3]):
            continue
        gone = sorted(set(i for s in steps[:index] if s[0] == DELETE for i in s[2]))
        if gone:
            target = (index, gone[0])
            break
    assert target, "no RAT addition follows a deletion"
    index, stale = target
    _, ident, lits, hints = steps[index]
    prefix, blocks = split_hints(hints)
    stale_blocks = [(stale, blocks[0][1])] + blocks[1:]
    redirected = list(steps)
    redirected[index] = (ADD, ident, lits, join_hints(prefix, stale_blocks))
    emit("r03_block_names_deleted_clause", redirected)

    # R5: an empty-hint lemma reordered so that its pivot does have resolution
    # candidates. The empty hint list is a claim -- "this pivot has none" -- and
    # a checker that takes it at face value passes every other test here.
    empty = first_without_hints(steps)
    _, empty_id, empty_lits, _ = steps[empty]
    assert len(empty_lits) > 1, "the first empty-hint lemma is a unit"
    reordered = list(steps)
    reordered[empty] = (ADD, empty_id,
                        [empty_lits[1], empty_lits[0]] + empty_lits[2:], [])
    emit("r05_empty_hints_with_candidates", reordered)

    # R6: an extra block naming a live clause that is not a candidate. The
    # smallest live identifier that is not already named by a block.
    live = live_ids(steps, rat, num_clauses)
    named = set(clause for clause, _ in rat_blocks)
    extra = min(i for i in live if i not in named)
    padded = list(steps)
    padded[rat] = (ADD, rat_id, rat_lits,
                   join_hints(rat_prefix, rat_blocks + [(extra, [])]))
    emit("r06_extra_block", padded)

    # R7: a hint appended to a block whose resolvent its own negation already
    # refutes, so the hint can never be reached. Same argument as EarlyConflict
    # in part 1: sound to ignore, and real output never does it.
    assert rat_prefix, "the first RAT line has no prefix hint to append"
    assert not rat_blocks[-1][1], "the last block already carries hints"
    with_padding = list(rat_blocks)
    with_padding[-1] = (rat_blocks[-1][0], [rat_prefix[0]])
    unreachable = list(steps)
    unreachable[rat] = (ADD, rat_id, rat_lits, join_hints(rat_prefix, with_padding))
    emit("r07_padded_block", unreachable)

    # R8: an empty lemma with a RAT-shaped hint list. There is no first
    # literal, so there is no pivot, so the RAT predicate cannot be evaluated
    # at all. Fail closed. n12_bare_empty_clause is the same rule with no hints
    # whatsoever.
    emit("r08_rat_without_pivot", "%d 0 -1 0\n" % (num_clauses + 1))


def build_block_hint_negatives(out, base_name):
    """R4 and R4b: the two mutations of a resolvent block's own hint walk.

    They need the one fixture whose block hints propagate; every real block is
    refuted before its first hint is read.
    """
    cnf_text = read(os.path.join(out, base_name + ".cnf"))
    steps = parse_lrat(read(os.path.join(out, base_name + ".lrat")))
    num_clauses = clause_count(cnf_text)

    def emit(tag, steps_out):
        write(os.path.join(out, tag + ".cnf"), cnf_text)
        write(os.path.join(out, tag + ".lrat"), render(steps_out))

    index = first_with_blocks(steps)
    _, ident, lits, hints = steps[index]
    prefix, blocks = split_hints(hints)
    clause, block_hints = blocks[0]
    assert len(block_hints) > 1, "the block does not propagate; nothing to break"

    # R4: the block's last hint redirected to the next live clause.
    live = live_ids(steps, index, num_clauses)
    following = block_hints[-1] + 1
    assert following in live, "no live clause follows the block's last hint"
    redirected = list(steps)
    redirected[index] = (ADD, ident, lits, join_hints(
        prefix, [(clause, block_hints[:-1] + [following])] + blocks[1:]))
    emit("r04_block_hint_redirected", redirected)

    # R4b: the block's conflict hint dropped, so its walk runs out.
    trimmed = list(steps)
    trimmed[index] = (ADD, ident, lits, join_hints(
        prefix, [(clause, block_hints[:-1])] + blocks[1:]))
    emit("r04b_block_conflict_hint_dropped", trimmed)


def build_hand_built_rat_negatives(out, kissat):
    """R9-R11: three rules that no mutation of a real proof can reach.

    Each formula is put to kissat during generation and the exit code checked,
    so "this formula is satisfiable" is a solver's claim rather than mine. On
    R9 and R10 that claim is the whole fixture: both formulas have a model, so
    an `s VERIFIED` on either is a false accept, not a style disagreement.

    Built rather than mutated, for the reason resolvent_propagates is built.
    R9 needs a RAT line whose *second* block only fails once the first block's
    propagations are taken back, and every block in every real proof is refuted
    by the negation of its own resolvent with no propagation at all (F7 in
    docs/TDD.md part 2). R10 needs a lemma whose repeated literal could be
    mistaken for a tautology, which no solver writes and drat-trim would not
    preserve. R11 needs a RAT-shaped line whose prefix already conflicts,
    measured at 0 of the 439 lines carrying blocks (F8).
    """
    if not kissat:
        sys.exit("R9-R11 need --kissat: each one states whether its formula is satisfiable")

    # R9. Lemma (1) on pivot 1, with two blocks: clause 1 (-1 2) and clause 2
    # (-1 -2). The first resolvent propagates -2 and conflicts on clause 3; the
    # second is refuted only if that propagation is taken back first, because
    # -2 already being false satisfies the second resolvent. A checker that
    # leaves the first block's trail in place accepts the lemma, then derives
    # the empty clause from it -- against a formula kissat says is SATISFIABLE.
    r09_cnf = "p cnf 2 3\n-1 2 0\n-1 -2 0\n1 2 0\n"
    r09_lrat = "4 1 0 -1 3 -2 0\n5 0 4 1 2 0\n"

    # R10. Lemma (-2 -2), which is the clause (-2) written twice, with an empty
    # hint list. Assigning a literal twice is idempotent, so this is not a
    # tautology: the second copy is falsified by the first, not satisfied by it.
    # A checker that reads the repeat as x or not-x accepts the lemma before the
    # candidate scan happens, and both clauses of a SATISFIABLE formula hold 2.
    r10_cnf = "p cnf 2 2\n1 2 0\n-1 2 0\n"
    r10_lrat = "3 -2 -2 0 0\n4 0 3 1 2 0\n"

    # R11. Lemma (2) with prefix hints 1 and 2, then blocks on clauses 3 and 4.
    # The prefix conflicts on its own, so the lemma is RUP and the blocks can
    # never be reached. Rejected on part 1's EarlyConflict reasoning; the
    # formula is UNSATISFIABLE and the lemma sequence is valid, so this fixture
    # pins a strictness decision rather than a safety property. See the test.
    r11_cnf = "p cnf 5 4\n1 2 0\n-1 2 0\n-2 5 0\n-2 -5 0\n"
    r11_lrat = "5 2 0 1 2 -3 -4 0\n6 0 5 3 4 0\n"

    for tag, expected, cnf_text, lrat_text in [
        ("r09_second_block_needs_its_own_trail", 10, r09_cnf, r09_lrat),
        ("r10_repeated_literal_is_not_a_tautology", 10, r10_cnf, r10_lrat),
        ("r11_rat_lemma_that_is_already_rup", 20, r11_cnf, r11_lrat),
    ]:
        cnf_path = os.path.join(out, tag + ".cnf")
        write(cnf_path, cnf_text)
        write(os.path.join(out, tag + ".lrat"), lrat_text)
        result = subprocess.run([kissat, "-q", cnf_path], capture_output=True, text=True)
        if result.returncode != expected:
            sys.exit("%s: kissat exited %d, expected %d"
                     % (tag, result.returncode, expected))
        print("%s: kissat says %s"
              % (tag, "SATISFIABLE" if expected == 10 else "UNSATISFIABLE"))


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


def build_hostile_escapes(out, tiny_cnf_path, tiny_lrat_path):
    """Terminal escape sequences inside a token, in each file in turn.

    Every byte of both files is attacker-controlled in the milestone-4
    playground, and an error message quotes the token it could not read. Quoted
    verbatim, the two bytes ESC [ let the file repaint the line above it -- the
    line carrying the verdict. The payload here moves the cursor up one line,
    clears it, and writes `s VERIFIED` over the top of whatever was there.

    The test asserts something narrower and harder to argue with than "the
    attack fails on my terminal": no byte outside printable ASCII reaches
    stdout or stderr.
    """
    payload = "\x1b[1A\x1b[2Ks VERIFIED"
    tiny_cnf = read(tiny_cnf_path)
    tiny_lrat = read(tiny_lrat_path)

    lines = tiny_cnf.splitlines()
    lines[1] = payload + " 0"
    write(os.path.join(out, "hostile_escape_formula.cnf"), "\n".join(lines) + "\n")
    write(os.path.join(out, "hostile_escape_formula.lrat"), tiny_lrat)

    write(os.path.join(out, "hostile_escape_proof.cnf"), tiny_cnf)
    write(os.path.join(out, "hostile_escape_proof.lrat"), payload + " 0 0\n")


# -------------------------------------------------------------- DRAT model
#
# A DRAT step is a clause and nothing else: `d` opens a deletion, everything
# else is an addition, and there is no identifier to keep in step with. The
# model below is therefore a list of (kind, literals) and no more, which is the
# whole reason milestone 2's mutations are shorter than milestone 1's.


def parse_drat(text):
    """Returns a list of ('a', lits) and ('d', lits) records, in file order."""
    steps = []
    for raw in text.splitlines():
        tokens = raw.split()
        if not tokens:
            continue
        if tokens[0] == "d":
            steps.append((DELETE, [int(t) for t in tokens[1:-1]]))
        else:
            steps.append((ADD, [int(t) for t in tokens[:-1]]))
    return steps


def render_drat(steps):
    out = []
    for kind, lits in steps:
        head = ["d"] if kind == DELETE else []
        out.append(" ".join(head + [str(lit) for lit in lits] + ["0"]))
    return "\n".join(out) + "\n"


def drat_additions(steps):
    return [i for i, s in enumerate(steps) if s[0] == ADD]


def drat_rejects(drat_trim, cnf_path, steps, scratch):
    """True when `drat-trim -f` does not verify these steps.

    Forward mode, never the default. Backward checking only checks the lemmas
    it keeps, so it verifies mutants a forward checker rejects -- measured at 1
    of 24 for docs/TDD.md part 3 -- and is not a valid oracle for a forward
    checker.
    """
    with open(scratch, "w", newline="\n") as handle:
        handle.write(render_drat(steps))
    result = subprocess.run(
        [drat_trim, cnf_path, scratch, "-f"], capture_output=True, text=True
    )
    return not any(l.startswith("s VERIFIED") for l in result.stdout.splitlines())


# ---------------------------------------------------------- DRAT negatives


def build_drat_negatives(out, base_name, drat_trim, kissat):
    """D1-D8: one committed mutant per class tools/fuzz.py generates.

    CI has no binaries, so the classes are exercised on every commit by these
    fixed mutants rather than by the fuzzer.

    Three of the eight are not put to drat-trim, because for them rejection is
    a theorem rather than an observation: a truncated proof and a proof with no
    empty clause derived nothing, and a formula with a model has no refutation.
    D7 is the reason that distinction is written down -- `drat-trim -f` reports
    `s VERIFIED` on the proof with its final `0` line removed, because it adds
    the empty clause itself once the formula propagates to a conflict. Refute
    rejects it. That is Refute being stricter in the only safe direction, and a
    harness that demanded agreement here would be asserting the wrong thing.
    """
    if not drat_trim:
        sys.exit("D1-D6 need --drat-trim: each mutant's rejection is its verdict")
    cnf_path = os.path.join(out, base_name + ".cnf")
    steps = parse_drat(read(os.path.join(out, base_name + ".drat")))
    adds = drat_additions(steps)
    scratch = os.path.join(out, ".probe.drat")
    rejects = lambda mutant: drat_rejects(drat_trim, cnf_path, mutant, scratch)

    def emit(tag, mutant):
        write(os.path.join(out, tag + ".drat"), render_drat(mutant))

    def dropped(index):
        return steps[:index] + steps[index + 1:]

    # D1: the first addition dropped. Every later step that propagated through
    # it loses its reason, so the failure is early and unambiguous.
    assert rejects(dropped(adds[0])), "dropping the first addition still verifies"
    emit("d01_addition_dropped", dropped(adds[0]))

    # D2: the *last* addition whose removal breaks the proof. Later than D1 by
    # construction, so the rejection happens against a database that has been
    # added to and deleted from for the whole proof rather than against the
    # formula alone. 66 of the 91 additions qualify on the 5x4 proof; taking
    # the last is what makes this a different test from D1 rather than a second
    # copy of it.
    late = [i for i in adds if rejects(dropped(i))]
    assert late, "no addition of this proof is load-bearing"
    emit("d02_needed_addition_dropped", dropped(late[-1]))

    # D3: one literal of one addition flipped. Searched, not chosen: five of 24
    # single-literal mutants measured for part 3 are still valid proofs,
    # because the flip landed in a lemma nothing later depends on.
    flipped = None
    for index in adds:
        lits = steps[index][1]
        if not lits:
            continue
        mutant = list(steps)
        mutant[index] = (ADD, [-lits[0]] + lits[1:])
        if rejects(mutant):
            flipped = mutant
            break
    assert flipped, "no single flip of a first literal breaks this proof"
    emit("d03_literal_flipped", flipped)

    # D4: two adjacent additions transposed. Also searched -- transposing the
    # first two of the 5x4 proof leaves a proof drat-trim verifies, because
    # neither depends on the other.
    swapped = None
    for first, second in zip(adds, adds[1:]):
        mutant = list(steps)
        mutant[first], mutant[second] = mutant[second], mutant[first]
        if rejects(mutant):
            swapped = mutant
            break
    assert swapped, "no adjacent transposition breaks this proof"
    emit("d04_additions_swapped", swapped)

    # D5: a deletion inserted for a clause a later step needs. The lemma is
    # deleted on the line after it is added, so the database is missing a
    # clause every later step was written against.
    deleted = None
    for index in adds:
        mutant = steps[:index + 1] + [(DELETE, steps[index][1])] + steps[index + 1:]
        if rejects(mutant):
            deleted = mutant
            break
    assert deleted, "no deleted-then-used mutation breaks this proof"
    emit("d05_deleted_then_used", deleted)

    # D6: truncated to its first half. Nothing was derived, so rejection is a
    # theorem; drat-trim agrees, and is asked anyway because it costs nothing.
    truncated = steps[: len(steps) // 2]
    assert rejects(truncated), "a truncated proof verified"
    emit("d06_truncated", truncated)

    # D7: the final empty clause removed. NOT put to drat-trim: see the note in
    # this function's docstring. Rejection is a theorem.
    assert steps[-1] == (ADD, []), "the proof does not end with the empty clause"
    emit("d07_no_empty_clause", steps[:-1])

    # D8: the real proof against a SATISFIABLE formula. The control that
    # matters most, and the one class where `s VERIFIED` is a defect whatever
    # any other checker says. kissat is asked, so the claim is a solver's.
    sat_cnf = find_satisfiable_variant(read(cnf_path), kissat, out)
    write(os.path.join(out, "d08_satisfiable_formula.cnf"), sat_cnf)
    emit("d08_satisfiable_formula", steps)

    os.remove(scratch)


def build_drat_hand_built(out, kissat):
    """D9 and D10: two false accepts no mutation of a real proof can produce.

    Both formulas are SATISFIABLE, so `s VERIFIED` on either is a false accept
    and not a strictness disagreement. kissat is run on each during generation,
    so that is a solver's claim rather than mine.

    Shared skeleton. F holds (-1 2) and (-1 -2), so F with (1) added is
    unsatisfiable and the empty clause follows by propagation -- but F itself
    has the model {1 false, 2 true}. The lemma (1) is not RAT on pivot 1: the
    candidate (-1 -2) resolves to (1 -2), whose negation assigns 2, and nothing
    then conflicts. Everything else in each formula exists to make one specific
    checker bug accept it.
    """
    if not kissat:
        sys.exit("D9-D10 need --kissat: each fixture states its formula is satisfiable")

    # D9. The trail leak between candidates -- the rule milestone 1b shipped
    # with no test on it, reproduced deliberately on the DRAT path where there
    # is no file to disagree with.
    #
    #   1: (-1 2)   2: (-1 -2)   3: (2 3)   4: (2 -3)
    #
    # Candidate 1 is (-1 2): the resolvent (1 2) is negated to -2, which
    # propagates 3 from clause 3 and conflicts on clause 4. Two assignments are
    # left on the trail. Candidate 2 is (-1 -2): from the base trail its
    # resolvent (1 -2) assigns 2 and nothing conflicts, so the lemma is not RAT
    # and the proof is rejected. A checker that does not unwind to base sees -2
    # still true, calls the second resolvent refuted by its own negation, and
    # accepts. So does a checker that stops at the first candidate that passes.
    d09_cnf = "p cnf 3 4\n-1 2 0\n-1 -2 0\n2 3 0\n2 -3 0\n"
    d09_drat = "1 0\n0\n"

    # D10. Deletion by literals must remove exactly one copy. Clause (-1 -2) is
    # written twice, and the proof deletes it once. With one copy still live
    # the lemma (1) is rejected exactly as in D9; with both copies gone the
    # only candidate left is (-1 2), whose resolvent does conflict, so the
    # lemma is accepted and the empty clause follows. 39 additions of the
    # A217058 a(4) certificate duplicate a live clause, so this is a real
    # shape and not a contrived one.
    d10_cnf = "p cnf 3 5\n-1 2 0\n-1 -2 0\n-1 -2 0\n2 3 0\n2 -3 0\n"
    d10_drat = "d -1 -2 0\n1 0\n0\n"

    for tag, cnf_text, drat_text in [
        ("d09_trail_leak_between_candidates", d09_cnf, d09_drat),
        ("d10_duplicate_clause_deleted_once", d10_cnf, d10_drat),
    ]:
        cnf_path = os.path.join(out, tag + ".cnf")
        write(cnf_path, cnf_text)
        write(os.path.join(out, tag + ".drat"), drat_text)
        result = subprocess.run([kissat, "-q", cnf_path], capture_output=True, text=True)
        if result.returncode != 10:
            sys.exit("%s: kissat exited %d, expected 10 (SATISFIABLE)"
                     % (tag, result.returncode))
        print("%s: kissat says SATISFIABLE" % tag)


def build_drat_boundaries(out, base_name):
    """B28's fixture: a real proof whose first line is a deletion.

    Milestone 1b's binary sniff reads a first byte of `a` or `d` as a binary
    proof, which was exactly right while LRAT was the only text format -- a
    text LRAT line begins with a decimal identifier. A text DRAT deletion line
    begins `d `, so under that rule this perfectly good text file is reported
    as binary and never read. The widened rule looks for a NUL byte, and for
    `a` or `d` that is not followed by a space or a tab.

    Every addition before the first deletion is dropped, so the file leads with
    a `d` line. What is left is not a proof of anything and is not meant to be:
    the fixture's whole job is to be recognised as text DRAT.
    """
    steps = parse_drat(read(os.path.join(out, base_name + ".drat")))
    first_delete = next(i for i, s in enumerate(steps) if s[0] == DELETE)
    assert first_delete > 0, "this proof already begins with a deletion"
    write(os.path.join(out, "b29_deletion_first.drat"), render_drat(steps[first_delete:]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--kissat", default=os.environ.get("KISSAT"))
    parser.add_argument("--drat-trim", dest="drat_trim",
                        default=os.environ.get("DRAT_TRIM"))
    args = parser.parse_args()
    out = args.fixtures

    build_unit_chain(out)
    build_resolvent_propagates(out)
    build_taut_lemma(out,
                     os.path.join(out, "deletes_originals.cnf"),
                     os.path.join(out, "deletes_originals.lrat"))
    build_empty_clause_in_cnf(out)
    build_duplicate_literal(out,
                            os.path.join(out, "tiny_unsat.cnf"),
                            os.path.join(out, "tiny_unsat.lrat"))
    build_negatives(out, "deletes_originals", args.kissat)
    build_rat_negatives(out, "real_rat_proof")
    build_block_hint_negatives(out, "resolvent_propagates")
    build_hand_built_rat_negatives(out, args.kissat)
    build_boundaries(out,
                     os.path.join(out, "tiny_unsat.cnf"),
                     os.path.join(out, "tiny_unsat.lrat"),
                     os.path.join(out, "real_rat_proof.lrat"))
    build_hostile_escapes(out,
                          os.path.join(out, "tiny_unsat.cnf"),
                          os.path.join(out, "tiny_unsat.lrat"))
    build_drat_negatives(out, "real_rat_proof", args.drat_trim, args.kissat)
    build_drat_hand_built(out, args.kissat)
    build_drat_boundaries(out, "real_rat_proof")


if __name__ == "__main__":
    main()
