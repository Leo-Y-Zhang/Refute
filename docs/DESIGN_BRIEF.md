# Design Brief — Refute

**Date:** 2026-08-13 · **PRD:** [PRD.md](PRD.md) · **App Flow:** [APP_FLOW.md](APP_FLOW.md)

Covers two surfaces: the terminal output of the CLI (milestone 1) and the
playground page (milestone 4).

## Intent

**Flat, factual, unpersuaded.** A checker that looks pleased with itself is a
checker you stop reading. The verdict should land with the affect of an instrument
reading, not an announcement.

It must never feel like a product demo, a dashboard, or a build tool celebrating
a green pipeline. No confetti, no "Success!", no large green tick as the primary
element. The interesting outcome is `NOT VERIFIED`, and the design should treat it
as equally normal — because a checker whose failure state feels like an error page
teaches people that failure means "something went wrong with the tool" rather than
"the proof is wrong".

## Who is looking at it

Someone deciding whether to believe a mathematical claim — often the author,
re-checking their own work and specifically hoping to catch themselves out. They
are unhurried, sceptical, and reading closely. Occasionally a stranger who has
followed a citation and has thirty seconds.

Both need the same thing first: **what was checked, and what the answer was.**
Neither needs encouragement.

## Precedents

- **`drat-trim`'s own output.** One line, `s VERIFIED`, prefixed in DIMACS
  convention, greppable, unchanged for a decade. What it gets right: the verdict
  is a *token*, not a sentence, so tooling can depend on it. Refute copies the
  wording deliberately.
- **`git bisect` / `rustc` error messages.** What they get right: the failure
  names the exact location and the exact expectation, then stops. `rustc`'s
  "expected X, found Y" shape is the model for every rejection message.
- **`caniuse`-style static reference pages** (for the playground): dense, one
  screen, no chrome, loads instantly, works without JavaScript for the parts that
  can. What it gets right: the page is the content.

## Anti-patterns for this project

- A green/red banner as the dominant element. The verdict is text.
- Percentage progress bars during checking that imply precision the checker does
  not have. Steps checked is a real number; "63%" of an unknown total is not.
- Emoji in output. It breaks `grep`, it breaks screen readers, and it is exactly
  the register this tool must avoid.
- Colour as the verdict. A verdict must survive `refute a.cnf b.lrat > log.txt`.
- A landing page that explains SAT solving before saying what the button does.
- Marketing comparisons to `drat-trim`. See TDD "Benchmark honesty".
- Any animation on the verdict's appearance. It arrives; it does not swoop.

## Type

**CLI:** whatever the terminal has. Never emit escape sequences to change it.

**Playground:** one family — a system monospace stack for everything
(`ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`), self-hosted nothing,
third-party nothing. Monospace throughout is the right call here rather than a
stylistic tic: every piece of content on the page is a clause, a literal, a step
id, a line number or a verdict token, and column alignment carries meaning.

Scale: 14px base, 13px for file listings, 20px for the verdict token, 600 weight
for the verdict only. Line length capped at 80ch for prose, unconstrained for
proof lines, which get horizontal scroll rather than wrapping — a wrapped clause
is unreadable.

## Colour

Roles first. Light and dark both derived from the same roles via
`prefers-color-scheme`; neither is the "real" one.

| Role | Light | Dark | Use |
|---|---|---|---|
| surface | `#fbfbf9` | `#16181c` | Page |
| raised | `#ffffff` | `#1e2126` | Panels, drop zones |
| text | `#16181c` | `#e8e6e3` | Body, verdict token |
| muted | `#5a5f66` | `#9aa0a8` | Line numbers, counts, help text |
| border | `#d8d6d1` | `#31353c` | Hairlines, drop-zone outline |
| accent | `#2f5d8a` | `#7fa9d4` | Links, focus ring |
| verified | `#1f6b3a` | `#5fbf85` | A rule beside the verdict, never the verdict's own text colour |
| refuted | `#a3341f` | `#e0806a` | Same treatment |
| caution | `#8a6a1f` | `#d4b45f` | `UNSUPPORTED` |

Contrast, measured against its own surface: text 15.8:1 light / 13.1:1 dark;
muted 5.9:1 / 6.4:1; accent 6.4:1 / 7.1:1; verified 5.3:1 / 7.9:1; refuted
6.1:1 / 6.5:1; caution 4.9:1 / 8.8:1. All clear 4.5:1. **These are the values to
verify in the build, not to trust from this table** — the check goes in the M4
done-list, run against the shipped CSS.

The verdict word itself is always `text`. The status colour appears only as a 3px
rule to its left, which also carries a shape (`+` verified, `x` refuted, `-`
unsupported) so colour is never the only signal.

## Spacing and layout

4px base scale: 4, 8, 12, 16, 24, 32, 48. Single column, `max-width: 72rem`,
centred, 24px page padding (16px under 480px). Vertical stack, no grid: heading,
one-sentence explanation, examples row, two file inputs, check button, verdict
panel, failing-line detail. Nothing is ever side-by-side above a breakpoint —
the reading order is the DOM order at every width.

## Components touched

All new; there is no existing component library and one must not be introduced.
Five components total: `FileDrop`, `ExampleRow`, `VerdictPanel`, `StepDetail`,
`StatRow`. `StatRow` and `StepDetail` are near-neighbours — if the build finds
them within one prop of each other, they merge rather than both existing.

No framework. The page is HTML, CSS and one module that instantiates the WASM
checker. A dependency-free page is also the strongest possible evidence for the
"nothing is uploaded" claim, which is a design requirement, not a technical one.

## States

| Component | Hover | Focus | Active | Disabled | Loading | Error |
|---|---|---|---|---|---|---|
| Check button | border darkens | 2px accent ring, 2px offset | 1px inset | 45% opacity + `aria-disabled`, label says why | label becomes "Checking… 12,400 steps" | n/a |
| FileDrop | border → accent | 2px accent ring | dashed → solid on dragover | n/a | n/a | filename struck through, reason beneath |
| ExampleRow item | underline | 2px accent ring | n/a | n/a | spinner replaced by "loading example" text | inline message |
| VerdictPanel | n/a | receives focus on completion, `tabindex="-1"` | n/a | n/a | skeleton, not a spinner | is itself the error surface |

`outline: none` appears nowhere. The focus ring is a single token reused
everywhere so it cannot drift.

## Accessibility floor — non-negotiable

- Contrast 4.5:1 body, 3:1 UI boundaries — table above, re-measured at build.
- Full keyboard operation; file inputs are real `<input type=file>`, drop is an
  enhancement.
- Touch targets ≥ 44px, including example-row items.
- Colour never the only signal — shape + word beside every verdict.
- `prefers-reduced-motion`: the only motion is the checking indicator, which
  becomes static text.
- 200% zoom: single column, nothing clipped, proof lines scroll horizontally
  within their own container rather than the page.
- Verdict panel is `aria-live="polite"` and announces the verdict word first.

## Responsive

Below 480px: page padding 16px, examples row wraps to a vertical list, the file
inputs stack (they already do). Above 1152px: nothing changes; the measure is
capped and the page simply centres. **What never changes at any width:** the
reading order, the verdict's position directly below the check button, and the
fact that the failing-line detail is visible without a click.

## Done means

- [ ] Verdict reads as an instrument, not an announcement — in both outcomes
- [ ] Every state designed, including WASM-failed-to-load and memory-exceeded
- [ ] Contrast re-measured against the shipped CSS, not this table
- [ ] Keyboard path walked end to end: skip link → examples → inputs → check →
      verdict
- [ ] Checked at 320px and at 200% zoom
- [ ] Network tab shows zero third-party requests and zero uploads
- [ ] CLI output contains no colour, no Unicode and no emoji, verified by piping
      to a file and reading the bytes
