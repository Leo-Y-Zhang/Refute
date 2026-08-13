# Fixtures

Committed bytes, not generated at test time. CI has neither `kissat` nor
`drat-trim`, and a suite that skips itself when a binary is missing is how a
checker ends up never having been run.

Regenerate with `tools/gen_fixtures.sh` (see `KISSAT` / `DRAT_TRIM` there). The
script is deterministic: a re-run produces byte-identical files.

`.gitattributes` marks this directory `-text`. Line endings here are evidence,
not formatting — `b08_crlf` exists specifically to check CRLF handling.

## Provenance

| Fixture | Origin |
|---|---|
| `tiny_unsat` | `kissat --no-binary` then `drat-trim -L`, 3 variables, all 8 clauses |
| `deletes_originals` | ditto, pigeonhole 4 into 3: 44 additions, 43 deletions, no unsupported construct |
| `real_rat_proof` | ditto, pigeonhole 5 into 4: contains both an empty hint list and RAT resolvent blocks |
| `random_unsat` | ditto, random 3-SAT just above the threshold: 980 RUP lemmas, 13,351 hints, no unsupported construct. The instance comes from an explicit linear congruential sequence, not Python's `random`, whose internals are not a stability contract across versions |
| `unit_chain` | hand-built hint lists over a real formula; the same lemma sequence in DRAT form is verified by `drat-trim` during generation |
| `taut_lemma` | `deletes_originals` with one tautological lemma spliced in before the last step |
| `dup_literal` | `tiny_unsat` with the literal its first propagation depends on written twice, and `tiny_unsat`'s proof unchanged. The clause and the literal are found by `tools/mutate.py`, not chosen; `drat-trim` verifies the same lemma sequence in DRAT form against the edited formula during generation |
| `empty_clause_in_cnf` | hand-built; `drat-trim` reports "trivial UNSAT" and emits an empty LRAT file, so there is nothing to capture |
| `n01`–`n11` | deterministic mutations of `deletes_originals` by `tools/mutate.py` |
| `n10` | the satisfiable formula was found by flipping one literal at a time, in file order, until `kissat` returned SAT — so the claim "this is satisfiable" is a solver's, not the author's |
| `n12`, `b01`–`b11`, `b12b` | constructed by `tools/mutate.py` from the real fixtures |

## Two measured facts that shaped the corpus

**Every real proof reports its empty hint list before it reaches a RAT block.**
Measured on pigeonhole 4x3, 5x4, 6x5, 7x6 and 8x7: in each of 5x4 through 8x7
the first unsupported construct is an empty hint list, on line 2, every time.
The RAT blocks resolve against exactly those lemmas, so they cannot come first.
`b12b_rat_hints` therefore carries a single RAT line copied verbatim out of
`real_rat_proof.lrat`; without it the `RatHints` path is never exercised.

**The 8x7 instance reproduces the design's measurement exactly:** 2,747 RUP
additions, 70 RAT, 56 empty-hint, 1,459 deletions, ids 205 to 3571. It is not
committed — 386 KB against a 500 KB corpus budget — but `tools/instances.py`
will regenerate it if the numbers are ever in doubt.
