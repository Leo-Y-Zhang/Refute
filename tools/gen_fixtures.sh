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
    # unit_chain's proof is hand-built: see the note in tools/mutate.py.
    [ "$name" = "unit_chain" ] && continue

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

echo
echo "corpus size: $(du -sk "$fixtures" | cut -f1) KB in $(ls "$fixtures" | wc -l) files"
