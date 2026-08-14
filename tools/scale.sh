#!/usr/bin/env bash
# Measures what a check costs on artefacts too large to commit.
#
# Not CI, and nothing here is a test. CI checks the committed corpus, whose
# largest proof is 386 KB; the budget in docs/TDD.md part 4 is stated against
# an 87 MB one, and a budget nobody re-measures is a sentence rather than a
# gate. This is the instrument that re-measures it.
#
# Given a directory of .cnf and .drat pairs it prints one row per artefact:
# bytes, additions, peak live clauses, wall clock and peak working set, and the
# same wall clock and peak for drat-trim -f beside it. Forward mode, never the
# default: backward checking only checks the lemmas it keeps, so its cost is
# not the cost of the same job.
#
# No path from any machine is written here. Point the variables at your own
# builds, as tools/differential.sh does:
#
#   DRAT_TRIM=/path/to/drat-trim tools/scale.sh <dir>
#
# The pairs come from a certificate generator run with a --keep option: a .drat
# is used where it sits beside its .cnf, and an artefact with no proof beside
# it is skipped rather than re-solved, because re-solving would make the row a
# measurement of the solver.
#
# Peak working set is read from the OS, per platform, by polling every 5 ms:
# PeakWorkingSet64 on Windows, /usr/bin/time -v on Linux. Where neither is
# available the column says so rather than guessing, which is the whole reason
# it is a column and not a claim.
set -uo pipefail

drat_trim="${DRAT_TRIM:-}"
refute_bin="${REFUTE:-}"
extra_args=""
dir=""

while [ $# -gt 0 ]; do
    case "$1" in
        --drat-trim) drat_trim="$2"; shift 2 ;;
        --refute) refute_bin="$2"; shift 2 ;;
        --refute-args) extra_args="$2"; shift 2 ;;
        -*) echo "unknown argument: $1" >&2; exit 2 ;;
        *) dir="$1"; shift ;;
    esac
done

if [ -z "$dir" ] || [ ! -d "$dir" ]; then
    echo "usage: tools/scale.sh <dir of .cnf and .drat pairs>" >&2
    exit 2
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "$refute_bin" ]; then
    refute_bin="$root/target/release/refute"
    [ -x "$refute_bin" ] || refute_bin="$refute_bin.exe"
fi
if [ ! -x "$refute_bin" ]; then
    echo "build the release binary first: cargo build --release" >&2
    exit 2
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# How the peak is read. Decided once, here, so that every row of a table comes
# from the same instrument and a table can say which one it was.
method="none"
if command -v powershell >/dev/null 2>&1 && [ -n "${WINDIR:-}${SYSTEMROOT:-}" ]; then
    method="windows"
elif [ -x /usr/bin/time ] && /usr/bin/time -v true >/dev/null 2>&1; then
    method="linux"
fi

if [ "$method" = "windows" ]; then
    cat > "$work/peak.ps1" <<'PS1'
# Runs a command and reports "<peak bytes> <seconds> <exit code>".
#
# Polled rather than read at exit: PeakWorkingSet64 is a property of a live
# process, and a process that has exited reports nothing at all.
$out = $args[0]
$exe = $args[1]
$rest = @()
if ($args.Count -gt 2) { $rest = $args[2..($args.Count - 1)] }
$watch = [Diagnostics.Stopwatch]::StartNew()
if ($rest.Count -gt 0) {
    $p = Start-Process -FilePath $exe -ArgumentList $rest -PassThru -NoNewWindow `
        -RedirectStandardOutput $out -RedirectStandardError "$out.err"
} else {
    $p = Start-Process -FilePath $exe -PassThru -NoNewWindow `
        -RedirectStandardOutput $out -RedirectStandardError "$out.err"
}
# Cache the handle before the process can exit. Without this the object
# reports an empty exit code however long it is waited on -- measured, both
# ways, on cmd /c exit 7: [7] with the handle cached and [] without it. An
# empty exit code read as success is how a harness reports that a checker
# verified something it never ran.
$null = $p.Handle
$peak = 0
while (-not $p.HasExited) {
    try {
        $p.Refresh()
        if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 }
    } catch { }
    Start-Sleep -Milliseconds 5
}
try {
    $p.Refresh()
    if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 }
} catch { }
$p.WaitForExit()
$watch.Stop()
"$peak $($watch.Elapsed.TotalSeconds) $($p.ExitCode)"
PS1
fi

# Runs one command and prints "<peak bytes or -> <seconds> <exit code>", with
# the command's own stdout left in $work/out.
measure() {
    case "$method" in
        windows)
            powershell -NoProfile -ExecutionPolicy Bypass -File "$work/peak.ps1" \
                "$work/out" "$@" 2>/dev/null | tr -d '\r'
            ;;
        linux)
            local start end
            start="$(date +%s.%N)"
            /usr/bin/time -v -o "$work/time" "$@" > "$work/out" 2> "$work/out.err"
            local code=$?
            end="$(date +%s.%N)"
            local kb
            kb="$(awk '/Maximum resident set size/ {print $NF}' "$work/time")"
            echo "$((kb * 1024)) $(echo "$end - $start" | bc) $code"
            ;;
        *)
            local start end
            start="$(date +%s)"
            "$@" > "$work/out" 2> "$work/out.err"
            local code=$?
            end="$(date +%s)"
            echo "- $((end - start)) $code"
            ;;
    esac
}

mb() {
    case "$1" in
        -|"") echo "-" ;;
        *) awk -v b="$1" 'BEGIN { printf "%.1f", b / 1048576 }' ;;
    esac
}

secs() {
    case "$1" in
        -|"") echo "-" ;;
        *) awk -v s="$1" 'BEGIN { printf "%.2f", s }' ;;
    esac
}

echo "peak working set read by: $method"
echo
printf '%-18s %12s %10s %10s %8s %9s %8s %9s\n' \
    artefact bytes additions 'peak live' refute 'peak MB' 'dt -f' 'peak MB'

status=0
for cnf in "$dir"/*.cnf; do
    [ -e "$cnf" ] || continue
    name="$(basename "$cnf" .cnf)"
    proof="$dir/${name%.cnf}.drat"
    # The generator writes f_<stem>.cnf beside p_<stem>.drat, so a plain stem
    # match is tried first and the f/p convention second. Anything else is
    # skipped and said to be skipped.
    if [ ! -f "$proof" ]; then
        proof="$dir/$(echo "$name" | sed 's/^f_/p_/').drat"
    fi
    if [ ! -f "$proof" ]; then
        printf '%-18s %12s %10s %10s %8s %9s %8s %9s\n' \
            "$name" "$(wc -c < "$cnf" | tr -d ' ')" - - - - - SKIPPED
        continue
    fi

    read -r peak secs code <<EOF
$(measure "$refute_bin" "$cnf" "$proof" --stats $extra_args)
EOF
    verdict="$(head -1 "$work/out" 2>/dev/null)"
    additions="$(sed -n 's/^refute: \([0-9]*\) additions.*/\1/p' "$work/out.err" | head -1)"
    live="$(sed -n 's/.*, \([0-9]*\) peak live clauses.*/\1/p' "$work/out.err" | head -1)"
    held="$(sed -n 's/^refute: \([0-9]*\) KB held.*/\1/p' "$work/out.err" | head -1)"
    if [ "$verdict" != "s VERIFIED" ] || [ "$code" != "0" ]; then
        echo "$name: refute said '${verdict:-nothing}' (exit $code)" >&2
        status=1
    fi

    read -r their_peak their_secs their_code <<EOF
$(measure "$drat_trim" "$cnf" "$proof" -f)
EOF
    if [ -z "${drat_trim:-}" ]; then
        their_secs="-"; their_peak="-"
    elif [ "$their_code" != "0" ]; then
        echo "$name: drat-trim -f exited $their_code" >&2
    fi

    printf '%-18s %12s %10s %10s %8s %9s %8s %9s\n' \
        "$name" "$(wc -c < "$proof" | tr -d ' ')" "${additions:--}" "${live:--}" \
        "$(secs "$secs")" "$(mb "$peak")" "$(secs "${their_secs:--}")" \
        "$(mb "${their_peak:--}")"
    echo "    store: ${held:--} KB held, by refute's own accounting"
done

echo
if [ "$status" -ne 0 ]; then
    echo "an artefact did not verify: the table above measures nothing" >&2
fi
exit "$status"
