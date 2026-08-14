#!/usr/bin/env bash
# Runs Refute and drat-trim over the same instances and compares their verdicts.
#
# Not CI. CI has neither binary and checks the committed bytes; this is the
# check that Refute agrees with an independently written implementation on
# proofs too large to commit -- pigeonhole 8x7 is 386 KB against a 500 KB corpus
# budget, and a real van der Waerden certificate is larger still.
#
# The rollback section of docs/TDD.md part 2 makes this a hard gate: the
# README's claim about what Refute checks is rewritten only after this has
# agreed with drat-trim on real proofs, never before.
#
# Locations are never written into a tracked file. Point the variables at your
# own builds:
#
#   KISSAT=/path/to/kissat DRAT_TRIM=/path/to/drat-trim tools/differential.sh
#
# or pass --kissat / --drat-trim. --extra <dir> adds every .cnf in a directory,
# so that certificates built elsewhere can be included without this repository
# depending on another one. If a .drat sits beside a .cnf in that directory it
# is used rather than re-solving, which is the shape a certificate generator
# writes with a --keep option.
#
# Milestone 2 adds the second pair of columns: drat-trim -f and refute on the
# RAW proof, with drat-trim in the chain neither as checker nor as producer.
# Forward mode, never the default -- backward checking only checks the lemmas
# it keeps, so it verifies mutants a forward checker rejects and is not a valid
# oracle for one.
set -uo pipefail

kissat="${KISSAT:-}"
drat_trim="${DRAT_TRIM:-}"
extra=""

while [ $# -gt 0 ]; do
    case "$1" in
        --kissat) kissat="$2"; shift 2 ;;
        --drat-trim) drat_trim="$2"; shift 2 ;;
        --extra) extra="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$kissat" ] || [ -z "$drat_trim" ]; then
    echo "set KISSAT and DRAT_TRIM, or pass --kissat and --drat-trim" >&2
    exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
refute="$root/target/release/refute"
[ -x "$refute" ] || refute="$refute.exe"
if [ ! -x "$refute" ]; then
    echo "build the release binary first: cargo build --release" >&2
    exit 2
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The instances, generated here rather than committed. 8x7 is the one the
# milestone is gated on and the one the corpus cannot afford.
python3 - "$work" <<'PY'
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), ""))
sys.path.insert(0, os.path.join(os.getcwd(), "tools"))
from instances import write_cnf, pigeonhole, random_3sat
out = sys.argv[1]
for pigeons, holes in [(4, 3), (5, 4), (6, 5), (7, 6), (8, 7)]:
    write_cnf(os.path.join(out, "pigeonhole_%dx%d.cnf" % (pigeons, holes)),
              *pigeonhole(pigeons, holes))
for name, (v, c, seed) in {
    "random_80_370": (80, 370, 99),
    "random_60_280": (60, 280, 7),
    "random_100_460": (100, 460, 4242),
}.items():
    write_cnf(os.path.join(out, name + ".cnf"), *random_3sat(v, c, seed))
PY

if [ -n "$extra" ]; then
    cp "$extra"/*.cnf "$work"/ || { echo "no .cnf files in $extra" >&2; exit 2; }
    cp "$extra"/*.drat "$work"/ 2>/dev/null || true
fi

verdict_of() {
    if "$@" | grep -q '^s VERIFIED'; then
        echo "s VERIFIED"
    else
        echo "s NOT VERIFIED"
    fi
}

printf '%-22s %9s %9s  %-13s %-13s  %-13s %-13s  %s\n' \
    instance drat lrat 'dt -f' refute 'dt -L' refute agree
status=0
for cnf in "$work"/*.cnf; do
    name="$(basename "$cnf" .cnf)"

    if [ -f "$work/$name.drat" ]; then
        # A certificate generator kept its raw proof. Re-solving would produce
        # a different one, and this row would then be about the solver rather
        # than about the file the author published.
        rc=20
    else
        "$kissat" --no-binary -q "$cnf" "$work/$name.drat" >/dev/null 2>&1
        rc=$?
    fi
    if [ "$rc" -ne 20 ]; then
        printf '%-22s %9s %9s  %-13s %-13s  %-13s %-13s  %s\n' \
            "$name" - - "kissat=$rc" - - - SKIPPED
        continue
    fi

    # The raw proof, both ways: the milestone-2 pair, with drat-trim in the
    # chain neither as checker nor as producer.
    raw_theirs="$(verdict_of "$drat_trim" "$cnf" "$work/$name.drat" -f)"
    raw_ours="$("$refute" "$cnf" "$work/$name.drat" 2>/dev/null | head -1)"

    # The trimmed proof, both ways: the milestone-1b pair, unchanged.
    theirs="$(verdict_of "$drat_trim" "$cnf" "$work/$name.drat" -L "$work/$name.lrat")"
    ours="$("$refute" "$cnf" "$work/$name.lrat" 2>/dev/null | head -1)"

    agree=yes
    if [ "$ours" != "$theirs" ] || [ "$raw_ours" != "$raw_theirs" ]; then
        agree=NO
        status=1
    fi
    printf '%-22s %9s %9s  %-13s %-13s  %-13s %-13s  %s\n' \
        "$name" \
        "$(wc -c < "$work/$name.drat" | tr -d ' ')" \
        "$(wc -c < "$work/$name.lrat" | tr -d ' ')" \
        "${raw_theirs#s }" "${raw_ours#s }" "${theirs#s }" "${ours#s }" "$agree"
done

echo
if [ "$status" -eq 0 ]; then
    echo "the two checkers agree on every instance"
else
    echo "DISAGREEMENT: do not rewrite the README" >&2
fi
exit "$status"
