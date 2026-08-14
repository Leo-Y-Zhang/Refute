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
| `real_rat_proof` | ditto, pigeonhole 5 into 4: 80 additions, 12 with resolvent blocks, 8 with an empty hint list, 24 blocks. The smallest real proof carrying both RAT shapes |
| `rat_pigeonhole` | ditto, pigeonhole 7 into 6: 624 additions, 42 RAT, 30 empty-hint, 108 blocks, 353 deletions, 55 KB. The same construct at a scale a subtly over-strict checker fails |
| `random_unsat` | ditto, random 3-SAT just above the threshold: 980 RUP lemmas, 13,351 hints, no unsupported construct. The instance comes from an explicit linear congruential sequence, not Python's `random`, whose internals are not a stability contract across versions |
| `unit_chain` | hand-built hint lists over a real formula; the same lemma sequence in DRAT form is verified by `drat-trim` during generation |
| `resolvent_propagates` | hand-built, same discipline: the lemma sequence `1 3 0` / `0` is verified by `drat-trim` in DRAT form during generation, and only the hint lists are the author's. The one fixture whose resolvent block has hints to walk |
| `b17_binary_proof` | the first 64 bytes of `kissat`'s binary DRAT for pigeonhole 5x4, produced by the same command as the rest with `--no-binary` left off. Its first byte is `a`, 0x61, which is what recognises it |
| `r01`–`r08` | deterministic mutations of `real_rat_proof` and `resolvent_propagates` by `tools/mutate.py`, one per new rejection rule in milestone 1b. Each choice is "the first one that qualifies, in file order", so a re-run reproduces it and the meaning does not drift when the base proof is regenerated |
| `r09`–`r11` | hand-built by `tools/mutate.py`, one per rule that no mutation of a real proof reaches; each construction site says why. `kissat` is run on all three formulas during generation and the exit code checked, so the satisfiability of each is a solver's claim |
| `r09_second_block_needs_its_own_trail` | **satisfiable** (`kissat` exit 10). Two resolvent blocks on one lemma, where the second is refuted only once the first block's propagations are taken back. `s VERIFIED` here would be a false accept against a formula with a model |
| `r10_repeated_literal_is_not_a_tautology` | **satisfiable** (`kissat` exit 10). A lemma written `-2 -2`, which is the clause `-2`, not `x or not-x`. The negative half of `dup_literal`'s and B19's positive coverage |
| `r11_rat_lemma_that_is_already_rup` | **unsatisfiable** (`kissat` exit 20), and the one negative fixture whose proof is not corrupt: `drat-trim` verifies the same lemma sequence in DRAT form (`2 0` / `0`) during generation. Refute rejects it on the `RatLemmaIsRup` strictness rule alone, so it pins a decision — `docs/TDD.md` part 2, open question 2 — rather than a safety property |
| `vdw_a217058_n21` | the upper bound behind the second term of [A217058](https://oeis.org/A217058), the mixed van der Waerden numbers. The **formula** is built by `vdw/drat_certify.py --seq A217058 --rung 1 --keep <dir>` in another of the author's repositories, which is why `tools/gen_fixtures.sh` regenerates this one only when pointed at it (`--vdw <formula.cnf>`) and says so loudly otherwise. The **proof** is produced here by the same `kissat --no-binary` command as every other fixture, so what is imported is a formula and never a proof: 559 additions, 40 RAT, 633 deletions, 571 peak live clauses. The largest real RAT-carrying proof in the corpus, and the only one whose subject is a published result |
| `taut_lemma` | `deletes_originals` with one tautological lemma spliced in before the last step |
| `dup_literal` | `tiny_unsat` with the literal its first propagation depends on written twice, and `tiny_unsat`'s proof unchanged. The clause and the literal are found by `tools/mutate.py`, not chosen; `drat-trim` verifies the same lemma sequence in DRAT form against the edited formula during generation |
| `empty_clause_in_cnf` | hand-built; `drat-trim` reports "trivial UNSAT" and emits an empty LRAT file, so there is nothing to capture |
| `n01`–`n11` | deterministic mutations of `deletes_originals` by `tools/mutate.py` |
| `n10` | the satisfiable formula was found by flipping one literal at a time, in file order, until `kissat` returned SAT — so the claim "this is satisfiable" is a solver's, not the author's |
| `n12`, `b01`–`b11`, `b12b` | constructed by `tools/mutate.py` from the real fixtures |
| `hostile_escape_formula`, `hostile_escape_proof` | `tiny_unsat` with one token replaced by `ESC [ 1 A ESC [ 2 K s VERIFIED`, once in each file. The bytes are real: `od -c` them before editing either file |

## The DRAT half (milestone 2)

Raw solver output, with `drat-trim` out of the chain in both directions: it did
not produce these files and it does not check them here. A `.drat` fixture with
no `.cnf` of its own pairs with the `.cnf` of the same name, or — where the name
is a mutation's — with the formula named in the row.

| Fixture | Origin |
|---|---|
| `tiny_unsat.drat`, `deletes_originals.drat`, `real_rat_proof.drat`, `rat_pigeonhole.drat` | `kissat --no-binary` on the same four formulas, normalised to LF, each verified by `drat-trim -f` during generation. Four of the five names that now carry both a `.lrat` and a `.drat`, which is what makes the two-checker agreement test possible on committed bytes with no binary in CI |
| `empty_clause_in_cnf.drat` | hand-built, two bytes: `0`. The formula already holds the empty clause, so the whole proof is the step that says so |
| `d01`–`d08` | deterministic mutations of `real_rat_proof.drat` by `tools/mutate.py`, one per class `tools/fuzz.py` generates. Five of them are *searched* rather than chosen — `drat-trim -f` is run on every candidate and the first it rejects is kept — because a single-literal flip often leaves a valid proof: 5 of 24 measured for `docs/TDD.md` part 3 did. `d01`–`d06` pair with `real_rat_proof.cnf` |
| `d07_no_empty_clause` | the same proof with its final `0` removed, and the one fixture deliberately **not** put to `drat-trim`: forward mode reports `s VERIFIED`, because it adds the empty clause itself once the formula propagates to a conflict. Nothing was derived, so rejection is a theorem, and Refute is stricter in the only safe direction |
| `d08_satisfiable_formula` | the unchanged proof against a formula found by flipping one literal at a time until `kissat` returned SAT. `s VERIFIED` here is a false accept and not a strictness disagreement |
| `d09_trail_leak_between_candidates` | **satisfiable** (`kissat` exit 10). Two candidates on one lemma, where the second is refuted only if the first's propagations are taken back. The milestone-1b hole, reproduced on the path that has no file to disagree with. It also fails a checker that stops at the first candidate that passes |
| `d10_duplicate_clause_deleted_once` | **satisfiable** (`kissat` exit 10). `(-1 -2)` written twice and deleted once. A store that removes both copies — or one keyed by literal set, which cannot hold two — verifies it. 39 additions of the A217058 a(4) certificate duplicate a live clause, so the shape is real |
| `b29_deletion_first.drat` | `real_rat_proof.drat` with every addition before its first deletion removed, so the file leads with `d `. Under milestone 1b's binary sniff — first byte `a` or `d` — this text file is reported as a binary proof and never read. It pairs with `real_rat_proof.cnf` and is not a proof of anything; being recognised as text DRAT is its whole job |
| `b30_crlf.drat` | `kissat`'s output for `tiny_unsat` with its line endings left alone. Generated on Windows, so CRLF throughout. Pairs with `tiny_unsat.cnf`, and CI greps it for a carriage return |

## Three measured facts that shaped the corpus

**Every real proof reports its empty hint list before it reaches a RAT block.**
Measured on pigeonhole 4x3, 5x4, 6x5, 7x6 and 8x7: in each of 5x4 through 8x7
the first RAT-shaped line is an empty hint list, on line 2, every time. The RAT
blocks resolve against exactly those lemmas, so they cannot come first. This is
why milestone 1 always stopped on line 2, and why `b12b_rat_hints` — a single
RAT line lifted out of `real_rat_proof.lrat` — was the only fixture that ever
reached the RAT path. It is kept for a better reason now: its blocks name
lemmas that do not exist when the line stands alone, so it is the control that
a RAT step is checked against the database it is in.

**No real proof exercises a resolvent block's hint walk.** All 703 blocks
measured across the eleven proofs behind `docs/TDD.md` part 2 are refuted by the
negation of their own resolvent and carry no hints at all.
`resolvent_propagates` is built so that one does: three hints, conflict on the
last. Without it that path ships with no coverage.

The same measurement is why `r09_second_block_needs_its_own_trail` is built
rather than mutated. A block that propagates nothing leaves nothing behind for
the next block to inherit, so no real file — and therefore no mutation of one —
can show whether the trail is taken back between blocks.

**The 8x7 instance reproduces the design's measurement exactly:** 2,747 RUP
additions, 70 RAT, 56 empty-hint, 1,459 deletions, ids 205 to 3571, and 20,069
clauses examined by the candidate scan. It is not committed — 386 KB against a
500 KB corpus budget — but `tools/instances.py` regenerates it and
`tools/differential.sh` runs it against `drat-trim`, which is the gate the
milestone was held to rather than a CI fixture.
