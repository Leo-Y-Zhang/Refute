#!/bin/bash
# SessionStart hook for Claude Code on the web.
# Synchronous, idempotent: warms the cargo build for both workspace crates
# (and the wasm target) so lint/test work immediately. Only runs in a remote
# (web) session.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$PROJECT_DIR"

# No external crates anywhere in the workspace (deliberate, see docs/PRD.md),
# so this only ever compiles the two in-tree crates (refute, refute-wasm) and
# warms target/ for a fast `cargo test --workspace` afterwards.
cargo build --workspace

# Needed for the wasm crate's own build target (`-p refute-wasm --target
# wasm32-unknown-unknown`); a no-op once already installed.
rustup target add wasm32-unknown-unknown

exit 0
