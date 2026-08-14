// What the module is, before anything asks it what it thinks.
//
// Three properties, all about doors rather than about furniture.
//
//   1. The three exported functions are exactly the ones the glue contract
//      names. A fourth is a second way into a checker whose whole point is
//      having one way in.
//
//   2. `memory` is exported, because the glue writes the two files into it.
//
//   3. The module imports NOTHING. That is the mechanism behind the
//      playground's privacy claim — "your files are never uploaded" — and it is
//      the difference between a promise and a property. A module with no
//      imports cannot call out, because there is nothing to call: no fetch, no
//      clock, no random source, no host function of any kind.
//
// What it deliberately does not assert: exported globals. `rustc` 1.74.0 emits
// `__data_end` and `__heap_base` where 1.97.1 emits neither, so an
// exact-set assertion here was a gate on the linker's mood — it failed on the
// MSRV leg the first time it ran, on a module that was in every way correct. A
// global is an address, not a capability: nothing can be called through one.
// They are printed instead, so a change shows up in a log without turning a
// toolchain difference into a red build.
//
// Usage:
//
//     node tools/wasm_shape.mjs [path to .wasm]
//
// Exits 0 and prints the shape, or exits 1 and says which property failed.

import { readFileSync, statSync } from 'node:fs';

const DEFAULT_MODULE =
  'target/wasm32-unknown-unknown/release-wasm/refute_wasm.wasm';

/** Exactly these functions are exported, and no others. */
const EXPECTED_FUNCTIONS = ['check', 'cnf_reserve', 'proof_reserve'];

const path = process.argv[2] ?? DEFAULT_MODULE;

let bytes;
try {
  bytes = readFileSync(path);
} catch (err) {
  console.error(`cannot read ${path}: ${err.message}`);
  console.error(
    'build it first: cargo build --profile release-wasm ' +
      '--target wasm32-unknown-unknown -p refute-wasm',
  );
  process.exit(1);
}

// A file that is not a module at all is a failure of this check, not a crash
// in it. Found by pointing this script at a hand-encoded module with a
// miscounted section length, which is exactly the shape a truncated build
// artefact has.
let module;
try {
  module = new WebAssembly.Module(bytes);
} catch (err) {
  console.error(`${path} is not a WebAssembly module: ${err.message}`);
  process.exit(1);
}
const exports = WebAssembly.Module.exports(module);
const imports = WebAssembly.Module.imports(module);

const failures = [];

const functions = exports
  .filter((e) => e.kind === 'function')
  .map((e) => e.name)
  .sort();
const wanted = [...EXPECTED_FUNCTIONS].sort();
if (functions.join(', ') !== wanted.join(', ')) {
  failures.push(
    `exported functions are [${functions.join(', ')}]; ` +
      `expected exactly [${wanted.join(', ')}]`,
  );
}

if (!exports.some((e) => e.kind === 'memory' && e.name === 'memory')) {
  failures.push(
    'the module does not export `memory`; the glue has nowhere to write the ' +
      'formula and the proof',
  );
}

if (imports.length !== 0) {
  const named = imports.map((i) => `${i.module}.${i.name}`).join(', ');
  failures.push(
    `the module imports ${imports.length} thing(s): ${named}. ` +
      'It must import nothing at all — that is what makes "your files are ' +
      'never uploaded" a property of the artefact rather than a promise ' +
      'about it.',
  );
}

const others = exports
  .filter((e) => e.kind !== 'function' && e.name !== 'memory')
  .map((e) => `${e.kind} ${e.name}`)
  .sort();

console.log(`module     ${path}`);
console.log(`bytes      ${statSync(path).size}`);
console.log(`functions  ${functions.join(', ')}`);
console.log(`imports    ${imports.length === 0 ? 'none' : imports.length}`);
console.log(`also       ${others.length === 0 ? 'nothing' : others.join(', ')}`);

if (failures.length > 0) {
  console.error('');
  for (const failure of failures) {
    console.error(`FAIL  ${failure}`);
  }
  process.exit(1);
}
