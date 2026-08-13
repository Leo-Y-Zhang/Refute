#!/usr/bin/env bash
# Re-derives the fixture corpus in tests/fixtures/.
#
# The committed bytes are what CI checks; this script exists to prove they were
# not hand-written and to regenerate them when the corpus changes. CI has
# neither binary, and a test suite that skips itself when a binary is missing is
# how a checker ends up never having been run.
#
# Locations are never written into a tracked file. Point the two variables at
# your own builds:
#
#   KISSAT=/path/to/kissat DRAT_TRIM=/path/to/drat-trim tools/gen_fixtures.sh
#
# or pass --kissat / --drat-trim.
set -euo pipefail

kissat="${KISSAT:-}"
drat_trim="${DRAT_TRIM:-}"

while [ $# -gt 0 ]; do
    case "$1" in
        --kissat) kissat="$2"; shift 2 ;;
        --drat-trim) drat_trim="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$kissat" ] || [ -z "$drat_trim" ]; then
    echo "set KISSAT and DRAT_TRIM, or pass --kissat and --drat-trim" >&2
    exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
fixtures="$root/tests/fixtures"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$fixtures"
python3 "$root/tools/instances.py" "$fixtures"

# Only the instances, never a fixture a previous run derived: the derived
# corpus deliberately contains satisfiable and malformed formulas.
for name in $(python3 "$root/tools/instances.py" --names); do
    cnf="$fixtures/$name.cnf"
    # Two proofs are hand-built: see the notes in tools/mutate.py. Both have
    # their lemma sequence validated against drat-trim in DRAT form below.
    [ "$name" = "unit_chain" ] && continue
    [ "$name" = "resolvent_propagates" ] && continue

    "$kissat" --no-binary -q "$cnf" "$work/$name.drat" && rc=$? || rc=$?
    if [ "$rc" -ne 20 ]; then
        echo "$name: kissat did not report UNSATISFIABLE (exit $rc)" >&2
        exit 1
    fi

    if ! "$drat_trim" "$cnf" "$work/$name.drat" -L "$fixtures/$name.lrat" \
        | grep -q '^s VERIFIED'; then
        echo "$name: drat-trim did not verify its own proof" >&2
        exit 1
    fi
    echo "generated $name.lrat"
done

# Independent check of the one hand-built lemma sequence: the same lemmas in
# DRAT form, verified by drat-trim. Only the hint lists are hand-supplied.
python3 - "$work/unit_chain.drat" <<'PY'
import sys
length = 12
with open(sys.argv[1], "w", newline="\n") as handle:
    for k in range(2, length + 1):
        handle.write("%d 0\n" % k)
    handle.write("0\n")
PY
if ! "$drat_trim" "$fixtures/unit_chain.cnf" "$work/unit_chain.drat" \
    | grep -q '^s VERIFIED'; then
    echo "unit_chain: drat-trim rejected the hand-built lemma sequence" >&2
    exit 1
fi
echo "validated unit_chain lemma sequence against drat-trim"

# The same independent check for the other hand-built proof. Only the hint
# lists -- prefix and resolvent block -- are mine; that the two lemmas refute
# the formula is drat-trim's verdict on the same sequence in DRAT form.
printf '1 3 0\n0\n' > "$work/resolvent_propagates.drat"
if ! "$drat_trim" "$fixtures/resolvent_propagates.cnf" \
    "$work/resolvent_propagates.drat" | grep -q '^s VERIFIED'; then
    echo "resolvent_propagates: drat-trim rejected the hand-built lemma sequence" >&2
    exit 1
fi
echo "validated resolvent_propagates lemma sequence against drat-trim"

# B17: a real binary proof, which is the mistake the PRD says drat-trim reports
# as a bad proof. kissat writes binary DRAT unless it is told not to, so this is
# the same command as above with --no-binary forgotten. Only the first 64 bytes
# are kept: the fixture has to be recognisable, not checkable.
"$kissat" -q "$fixtures/real_rat_proof.cnf" "$work/binary.drat" && rc=$? || rc=$?
if [ "$rc" -ne 20 ]; then
    echo "b17_binary_proof: kissat did not report UNSATISFIABLE (exit $rc)" >&2
    exit 1
fi
head -c 64 "$work/binary.drat" > "$fixtures/b17_binary_proof.lrat"
cp "$fixtures/real_rat_proof.cnf" "$fixtures/b17_binary_proof.cnf"
if [ "$(head -c 1 "$fixtures/b17_binary_proof.lrat")" != "a" ]; then
    echo "b17_binary_proof: first byte is not the binary DRAT addition marker" >&2
    exit 1
fi
echo "captured b17_binary_proof.lrat (64 bytes of binary DRAT)"

python3 "$root/tools/mutate.py" --fixtures "$fixtures" --kissat "$kissat"

# Independent check of the one formula edited by hand: the same lemma sequence
# in DRAT form, verified by drat-trim against the edited formula. The lemmas are
# the solver's; only one literal of the formula was written twice. Without this
# the fixture asserts a proof is valid on my say-so.
python3 - "$fixtures/dup_literal.lrat" "$work/dup_literal.drat" <<'PY'
import sys
with open(sys.argv[1]) as source, open(sys.argv[2], "w", newline="\n") as out:
    for raw in source:
        tokens = raw.split()
        if len(tokens) > 1 and tokens[1] == "d":
            continue
        end = tokens.index("0", 1)
        out.write(" ".join(tokens[1:end] + ["0"]) + "\n")
PY
if ! "$drat_trim" "$fixtures/dup_literal.cnf" "$work/dup_literal.drat" \
    | grep -q '^s VERIFIED'; then
    echo "dup_literal: drat-trim rejected the proof against the edited formula" >&2
    exit 1
fi
echo "validated dup_literal against drat-trim"

# R11 is the one negative fixture whose proof is not corrupt. Its lemma
# sequence is valid and Refute rejects it on a strictness rule alone, so the
# fixture is worth nothing unless the sequence really does refute the formula.
# Same check the two hand-built positives get: the same lemmas in DRAT form.
printf '2 0\n0\n' > "$work/r11.drat"
if ! "$drat_trim" "$fixtures/r11_rat_lemma_that_is_already_rup.cnf" \
    "$work/r11.drat" | grep -q '^s VERIFIED'; then
    echo "r11: drat-trim rejected the lemma sequence Refute is strict about" >&2
    exit 1
fi
echo "validated r11 lemma sequence against drat-trim"

echo
echo "corpus size: $(du -sk "$fixtures" | cut -f1) KB in $(ls "$fixtures" | wc -l) files"
