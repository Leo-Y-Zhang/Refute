# Refute

An independent, zero-dependency Rust forward checker for DRAT and LRAT
unsatisfiability proofs — a second opinion on SAT-solver UNSAT certificates,
written separately from `drat-trim` so a soundness bug in one checker isn't a
soundness bug in both. Ships as a CLI (`src/bin/refute.rs`) and as a
no-imports WebAssembly module (`wasm/`) for an in-browser checker page
(`page/`). `unsafe_code = "forbid"` at the workspace level (the wasm crate
must locally `deny` it instead, for the `#[no_mangle]` export boundary only).

## Directory layout

- `src/` — the checker library: `checker.rs`, `drat.rs` (+ `drat/checker.rs`,
  `drat/store.rs`), `lrat.rs`, `cnf.rs`, `parse.rs`, `format.rs`, `lit.rs`,
  `limits.rs`, `verdict.rs`; `src/bin/refute.rs` is the CLI entry point.
- `wasm/` — the `refute-wasm` crate (`cdylib`), the WebAssembly export
  boundary only, depending on `refute` by path.
- `tests/` — integration test binaries: `positive.rs` (13, proofs that must
  verify), `negative.rs` (24, corruption controls that must be rejected),
  `drat.rs` (35), `boundary.rs` (26), `memory.rs` (16), `cli.rs` (13),
  `trust_boundary.rs` (8); `tests/fixtures/` holds the `.cnf`/`.lrat`/`.drat`
  corpus (including deliberately-CRLF fixtures, see CI caveat below).
- `tools/` — Node scripts (`wasm_shape.mjs`, `wasm_agreement.mjs`,
  `worker_contract.mjs`, `build_page.mjs`, `browser_check.mjs`) plus Python
  fuzzing/mutation tooling (`fuzz.py`, `mutate.py`) — run directly with
  `node`/`python3`, no `package.json`/npm install involved anywhere.
- `page/` — the static checker page (`index.html`, `main.js`, `worker.js`).

## Install

```
cargo build --workspace
```
No external dependencies anywhere in the workspace (deliberate design
constraint, see `docs/PRD.md`), so this only compiles the two in-tree crates.
For the WebAssembly claim specifically:
```
rustup target add wasm32-unknown-unknown
cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm
```

## Lint / format / typecheck

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
CI pins toolchain 1.97.1 for this job specifically ("a gate whose verdict
depends on the day you run it is not a gate"); this container's stable
toolchain (1.94.1, with clippy/rustfmt already installed) is used instead —
see caveat below.

## Test

```
cargo test --workspace         # 165 passed across both crates, ~8s
```
Fastest useful subset — the security-relevant corruption-detection suite,
one integration binary:
```
cargo test --workspace --test negative
```

## Verification gate (source of truth)

The full `cargo test --workspace` run (165 tests, cheap at ~8s) is the
practical gate. Within it, **`negative.rs` (24 corruption controls) and
`boundary.rs` (26 edge cases) are what the README treats as the load-bearing
half of the suite** — `positive.rs` only proves the checker accepts valid
proofs, but a checker that accepts everything would pass that trivially; it's
the tests that assert a *tampered* proof (flipped literal, dropped addition,
swapped hints, non-monotonic ids, etc.) is correctly rejected that back the
soundness claim. The README is explicit that some later boundary cases
(R9-R11, P12, from milestone 1b onward) were written *after* a real bug that
`check()` unconditionally verified was fixed, and says which earlier cases
(N1-N12, R1-R8) predate that discipline and can't claim the same rigor —
worth preserving that distinction rather than treating all "negative" tests
as equally strong evidence.

## Environment caveats (from audit)

- CI's `lint` and `page`/`wasm` jobs install a pinned toolchain (1.97.1) via
  `rustup`; this sandbox already has a working stable toolchain (rustc
  1.94.1, cargo 1.94.1, clippy 0.1.94, rustfmt 1.8.0) with the wasm32 target
  addable via `rustup target add`. The hook uses the pre-installed stable
  toolchain rather than fetching 1.97.1, to avoid a network dependency.
- Whole workspace built and tested cleanly with zero warnings; no external
  crates, so there's no registry/network dependency for `cargo build`/`test`
  themselves (only for adding the wasm32 target the first time).
- `tools/fuzz.py` needs `KISSAT`/`DRAT_TRIM` external solver binaries and
  10,000-case runs — well outside a normal session's time box; not part of
  the hook or the everyday gate.
- `tools/*.mjs` are plain Node scripts with no `package.json` — nothing to
  `npm install` for the `wasm`/`page` CI jobs' checks.

## CI / conventions

- `ci.yml` has 5 jobs: `lint` (pinned 1.97.1, fmt + clippy), `test`
  (`stable` and `1.74.0` matrix x ubuntu/windows, `cargo test --workspace
  --no-fail-fast`, plus a check that CRLF fixtures still contain literal
  CRLF), `wasm` (build the wasm32 target on `stable` and `1.74.0`, then
  `wasm_shape.mjs`/`wasm_agreement.mjs`), `page` (headless-Chrome checks that
  the page makes zero cross-origin requests), and `gitleaks`.
- All `actions/*` steps are pinned to a commit SHA, not a tag, deliberately
  (see comments in `ci.yml`).
- `rust-version = "1.74"` in `Cargo.toml` is verified, not just asserted: the
  `test`/`wasm` jobs actually build/test on 1.74.0 as well as `stable`.
- Clippy lints `unwrap_used`/`expect_used`/`indexing_slicing`/`panic`/
  `arithmetic_side_effects` are all `deny` workspace-wide — a panic in the
  CLI is documented as "the same denial of service as a panic in the
  library", and in the wasm target "a blank page".
