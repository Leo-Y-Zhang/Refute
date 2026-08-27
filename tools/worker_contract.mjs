// What the worker will and will not do with a message it is sent.
//
// The worker is where the page's one claim is either true or false. It is the
// only thing on the page that fetches anything after load, it runs where a call
// to somewhere else would be least visible, and it is handed whatever the
// document posts to it. `tools/browser_check.mjs` records every request a real
// browser makes and fails on one that leaves the origin, which is that claim
// checked end to end; this asserts the two properties underneath it, and does so
// without a browser, a build or a network.
//
//   1. The module URL is the worker's own. A message naming another one is
//      ignored, so the request cannot be steered by whatever posted. The module
//      URL used to arrive in the message, and a page whose privacy rests on the
//      sender having chosen a polite URL rests on the sender.
//
//   2. A message that is not a formula and a proof gets an error back, never a
//      verdict. `new Uint8Array(undefined)` is an empty array rather than a
//      throw, and `b02_empty_cnf` in tests/boundary.rs is the checker answering
//      NOT VERIFIED about an empty formula — so without the guard the glue runs
//      to completion on a message it did not understand and reports a verdict
//      about bytes nobody supplied.
//
// The module is stubbed on purpose. What is under test is the glue's contract
// with whoever posts to it, not the checker; `tools/wasm_agreement.mjs` drives
// the real module against the real binary and is the only thing that should.
//
// Usage:
//
//     node tools/worker_contract.mjs
//
// Exits 0 and prints what the worker did, or exits 1 and says which property
// failed.

// Paths in this file are relative to the repository root, so the process moves
// there first. Running `node tools/<this>.mjs` from a home directory is the
// obvious thing to try and used to fail with a module-not-found error that
// named a path nobody had typed.
import { chdir } from 'node:process';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
chdir(resolvePath(dirname(fileURLToPath(import.meta.url)), '..'));

import { readFileSync } from 'node:fs';
import { createContext, runInContext } from 'node:vm';

/** The module the worker is allowed to ask for, and the only one. */
const OWN_MODULE = 'refute_wasm.wasm';

/** A URL that must never be requested, whoever puts it in a message. */
const ELSEWHERE = 'https://refute-must-not-fetch.invalid/evil.wasm';

/**
 * Loads `page/worker.js` into a worker-shaped global and returns a driver for
 * it.
 *
 * A dedicated worker's global is its own `self`, so the file is run in a
 * context that is its own `self` too. Everything the file reaches for is
 * stubbed here and nothing else is: a stub the worker never touches would be
 * this harness testing itself.
 */
function load() {
  const fetched = [];
  const posted = [];

  const scope = {
    performance: { now: () => 0 },
    postMessage: (message) => posted.push(message),
    fetch: async (url) => {
      fetched.push(String(url));
      return {
        ok: true,
        // Not `application/wasm`, so the plain-fetch branch runs. Either branch
        // reaches the same instantiate; this one needs no streaming source.
        headers: { get: () => 'application/octet-stream' },
        arrayBuffer: async () => new ArrayBuffer(8),
      };
    },
    WebAssembly: {
      instantiate: async () => ({
        instance: {
          exports: {
            memory: { buffer: new ArrayBuffer(1024) },
            cnf_reserve: () => 16,
            proof_reserve: () => 512,
            check: () => 0,
          },
        },
      }),
    },
  };
  scope.self = scope;
  createContext(scope);
  runInContext(readFileSync('page/worker.js', 'utf8'), scope, {
    filename: 'page/worker.js',
  });

  return {
    fetched,
    posted,
    /**
     * Posts one message and waits for the handler to finish.
     *
     * `build` is evaluated inside the context, because a structured clone
     * arrives in the worker's own realm and `instanceof ArrayBuffer` is a
     * realm-sensitive test. A buffer made out here would fail it for a reason
     * that has nothing to do with the worker.
     */
    async post(build) {
      const data = runInContext(build, scope);
      await scope.self.onmessage({ data });
    },
  };
}

const failures = [];

// 1. A message naming another origin's module.

const steered = load();
await steered.post(
  `({ moduleUrl: ${JSON.stringify(ELSEWHERE)}, ` +
    'cnf: new ArrayBuffer(8), proof: new ArrayBuffer(8) })',
);

if (steered.fetched.join(', ') !== OWN_MODULE) {
  failures.push(
    `a message naming ${ELSEWHERE} made the worker fetch ` +
      `[${steered.fetched.join(', ')}]; it may only ever fetch ${OWN_MODULE}. ` +
      'The one request this page makes is not the sender\'s to choose.',
  );
}
// Without this the assertion above passes for the wrong reason: a worker that
// refused the message outright fetches nothing at all, which is also not
// [refute_wasm.wasm], but a worker that stopped working would still want
// finding.
if (steered.posted[0]?.kind !== 'verdict') {
  failures.push(
    `the worker answered ${JSON.stringify(steered.posted[0]?.kind)} to a ` +
      'well-formed pair of files, so the check above proves nothing about ' +
      'which module it loads',
  );
}

// 2. A message that is not a formula and a proof.

/** Messages the worker cannot use, and what each one is, for the report. */
const STRAY = [
  ['an empty message', '({})'],
  ['no data at all', 'undefined'],
  ['a string', "'check this please'"],
  ['a formula and no proof', '({ cnf: new ArrayBuffer(8) })'],
  ['two strings', "({ cnf: 'formula', proof: 'proof' })"],
];

for (const [what, build] of STRAY) {
  const stray = load();
  // A throw is a failure of this property too, and one worth naming rather than
  // printing a stack trace over: a handler that dies on a message it cannot use
  // has answered nothing at all, and the page waits for a panel that never
  // fills.
  try {
    await stray.post(build);
  } catch (error) {
    failures.push(`${what} made the worker throw: ${error}`);
    continue;
  }
  if (stray.posted[0]?.kind !== 'internal') {
    failures.push(
      `${what} got ${JSON.stringify(stray.posted[0]?.kind)} back, where only ` +
        'an error will do. A verdict here is a verdict about bytes nobody ' +
        'supplied.',
    );
  }
  if (stray.fetched.length !== 0) {
    failures.push(
      `${what} made the worker fetch [${stray.fetched.join(', ')}]. A message ` +
        'it cannot use is a message it should do nothing about.',
    );
  }
}

console.log('worker     page/worker.js');
console.log(`asked for  ${ELSEWHERE}`);
console.log(`fetched    ${steered.fetched.join(', ') || 'nothing'}`);
console.log(`answered   ${steered.posted[0]?.kind}`);
console.log(
  `stray      ${STRAY.length} messages that are not a formula and a proof`,
);

if (failures.length > 0) {
  console.error('');
  for (const failure of failures) {
    console.error(`FAIL  ${failure}`);
  }
  process.exit(1);
}
