// Does the module say what the checker says?
//
// This is the only test in milestone 4 that really matters. Everything else
// the playground does is interface; a page that reports a verdict the CLI does
// not report is worse than no page at all, because it discredits every other
// verdict this project has ever produced.
//
// The expectations are not written here. They come from the native binary, run
// on the same two files, at run time. Writing them down a second time would
// only prove that two lists agree with each other.
//
// The pair list is not written here either. It is read out of the test sources,
// so the corpus this harness runs is by construction the corpus tests/*.rs
// pins: add a fixture pair to a test and it is checked here on the next run,
// with nothing to remember. Every proof file in the corpus must appear in some
// pair, and the harness fails if one does not, so a fixture cannot go quietly
// unchecked.
//
// Usage:
//
//     node tools/wasm_agreement.mjs [options]
//
//       --module <path>   the .wasm to test
//                         (default target/wasm32-unknown-unknown/release-wasm/refute_wasm.wasm)
//       --binary <path>   the native refute to compare against
//                         (default target/debug/refute[.exe])
//       --extra <dir>     also run every .cnf/.drat and .cnf/.lrat pair in a
//                         directory, for certificates too large to commit
//       --quiet           only print disagreements and the summary
//
// Exits 0 if every pair agrees and all three verdicts were seen, 1 otherwise.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, join } from 'node:path';

const VERDICTS = ['VERIFIED', 'NOT VERIFIED', 'UNSUPPORTED'];

/**
 * The module's fourth return code, which is not a verdict.
 *
 * Named here so that a pair which ever produced it disagrees with the native
 * checker loudly, rather than being printed as though the checker had said it.
 */
const REFUSED_CODE = 3;

/**
 * The number this file does not get to choose.
 *
 * The ceiling exists in two places and has to: the page refuses a file before
 * it instantiates anything, so it cannot ask the module how large a file the
 * module would hold. So this harness reads both, fails if they disagree, and
 * then checks that the compiled module really does accept exactly that and
 * refuse exactly one byte more. Three links, and nothing here is a fourth copy
 * of the number.
 *
 * Comment lines are stripped before matching, and that is not tidiness either.
 * The same trick caught a guard in `tests/trust_boundary.rs` reading a
 * manifest's own explanatory comment instead of its setting, and passing while
 * the setting was wrong.
 */
function constantFrom(path, pattern, what) {
  const code = readFileSync(path, 'utf8')
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n');
  const match = pattern.exec(code);
  if (match === null) {
    throw new Error(`could not find ${what} in ${path}`);
  }
  return Number(match[1].replaceAll('_', ''));
}

const PAGE_CEILING = constantFrom(
  join('page', 'limits.js'),
  /MAX_INPUT_BYTES\s*=\s*([0-9_]+)/,
  'MAX_INPUT_BYTES',
);
const MODULE_CEILING = constantFrom(
  join('wasm', 'src', 'lib.rs'),
  /MAX_INPUT_BYTES:\s*usize\s*=\s*([0-9_]+)/,
  'MAX_INPUT_BYTES',
);
const MAX_INPUT_BYTES = MODULE_CEILING;

function parseArgs(argv) {
  const options = {
    module: 'target/wasm32-unknown-unknown/release-wasm/refute_wasm.wasm',
    binary: existsSync('target/debug/refute.exe')
      ? 'target/debug/refute.exe'
      : 'target/debug/refute',
    extra: null,
    quiet: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--quiet') {
      options.quiet = true;
    } else if (arg === '--module' || arg === '--binary' || arg === '--extra') {
      i += 1;
      if (i >= argv.length) {
        throw new Error(`${arg} needs a value`);
      }
      options[arg.slice(2)] = argv[i];
    } else {
      throw new Error(`unknown argument ${arg}`);
    }
  }
  return options;
}

// ---------------------------------------------------------------------------
// The corpus, read out of the tests that pin it

const FIXTURES = join('tests', 'fixtures');

/**
 * Every (formula, proof) pair named anywhere in the Rust test sources.
 *
 * Matched on the shape rather than on the function name — `common::outcome`,
 * `common::verdict`, `common::cli` and every local helper that takes the pair
 * as its first two arguments all look identical here, and a helper added later
 * will too.
 */
function pairsFromTests() {
  const pattern =
    /"([A-Za-z0-9_]+\.cnf)"\s*,\s*"([A-Za-z0-9_]+\.(?:lrat|drat))"/g;
  const seen = new Map();
  for (const entry of readdirSync('tests')) {
    if (!entry.endsWith('.rs')) {
      continue;
    }
    const source = readFileSync(join('tests', entry), 'utf8');
    for (const match of source.matchAll(pattern)) {
      const [, cnf, proof] = match;
      seen.set(`${cnf} ${proof}`, { cnf, proof, from: entry });
    }
  }
  return [...seen.values()].sort((a, b) =>
    `${a.proof} ${a.cnf}`.localeCompare(`${b.proof} ${b.cnf}`),
  );
}

/** Proof files in the corpus that no pair names. */
function uncoveredProofs(pairs) {
  const named = new Set(pairs.map((p) => p.proof));
  return readdirSync(FIXTURES)
    .filter((f) => f.endsWith('.lrat') || f.endsWith('.drat'))
    .filter((f) => !named.has(f))
    .sort();
}

/**
 * Pairs in a directory outside the repository, for certificates too large to
 * commit.
 *
 * Two conventions, because two exist in the wild. `tools/differential.sh` knows
 * only the first, and the author's own generator writes the second: a `--keep`
 * directory from `MathRecords/vdw/drat_certify.py` holds `f_n33_j4.cnf` beside
 * `p_n33_j4.drat`, which share no stem at all. A harness that silently skipped
 * that directory would report zero extra pairs and look like it had passed.
 *
 *   1. same stem      `x.cnf`   with `x.drat`   or `x.lrat`
 *   2. f_ / p_ prefix `f_x.cnf` with `p_x.drat` or `p_x.lrat`
 */
function pairsFromDirectory(dir) {
  const files = readdirSync(dir);
  const pairs = [];
  for (const proof of files.filter(
    (f) => f.endsWith('.drat') || f.endsWith('.lrat'),
  )) {
    const stem = proof.slice(0, proof.lastIndexOf('.'));
    const sameStem = `${stem}.cnf`;
    const prefixed = stem.startsWith('p_')
      ? `f_${stem.slice(2)}.cnf`
      : undefined;
    const cnf = files.includes(sameStem)
      ? sameStem
      : prefixed !== undefined && files.includes(prefixed)
        ? prefixed
        : undefined;
    if (cnf !== undefined) {
      pairs.push({ cnf, proof, from: dir, dir });
    }
  }
  if (pairs.length === 0) {
    throw new Error(
      `--extra ${dir} holds no formula/proof pair this harness recognises. ` +
        'Expected x.cnf beside x.drat, or f_x.cnf beside p_x.drat.',
    );
  }
  return pairs.sort((a, b) => a.proof.localeCompare(b.proof));
}

// ---------------------------------------------------------------------------
// The two checkers

function nativeVerdict(binary, cnfPath, proofPath) {
  const run = spawnSync(binary, [cnfPath, proofPath], { encoding: 'utf8' });
  if (run.error) {
    throw new Error(`cannot run ${binary}: ${run.error.message}`);
  }
  const line = (run.stdout ?? '')
    .split('\n')
    .map((l) => l.trim())
    .find((l) => l.startsWith('s '));
  if (line === undefined) {
    throw new Error(
      `${binary} printed no verdict line for ${basename(proofPath)}: ` +
        `${JSON.stringify(run.stdout)} ${JSON.stringify(run.stderr)}`,
    );
  }
  return line.slice(2);
}

/**
 * One check, on its own instance.
 *
 * Fresh every time, and that is a memory rule rather than hygiene: `memory.grow`
 * has no inverse, so linear memory is a high-water mark and reusing an instance
 * charges the second check for the first. It is also what makes the peak figure
 * below mean anything.
 *
 * The glue order is the documented one, and it is documented because the
 * probe's harness got it wrong: take the pointer from the reserve call first,
 * read `memory.buffer` second. Reading the buffer first hands `Uint8Array` a
 * view that the growing memory has already detached.
 */
function wasmVerdict(moduleBytes, cnf, proof) {
  const instance = new WebAssembly.Instance(
    new WebAssembly.Module(moduleBytes),
    {},
  );
  const exports = instance.exports;

  const cnfOffset = exports.cnf_reserve(cnf.length);
  const proofOffset = exports.proof_reserve(proof.length);
  // Both views built after the last call that can grow memory, never before.
  new Uint8Array(exports.memory.buffer, cnfOffset, cnf.length).set(cnf);
  new Uint8Array(exports.memory.buffer, proofOffset, proof.length).set(proof);

  let code;
  // One call inside the clock, and only this one. The probe's first harness
  // timed `check()` together with a second export that re-runs the whole check,
  // and reported the sandbox as 2.3x native when it is 1.21x.
  const started = process.hrtime.bigint();
  try {
    code = exports.check();
  } catch (err) {
    // A panic in the checker is a trap here. It is reported as what it is and
    // never as a verdict.
    return { verdict: `TRAPPED (${err.message})`, peak: NaN, seconds: NaN };
  }
  const seconds = Number(process.hrtime.bigint() - started) / 1e9;
  // REFUSED is named rather than mapped to a verdict, so that a pair which
  // somehow produced it disagrees with the native checker instead of being
  // printed as though the checker had said it.
  const verdict = code === REFUSED_CODE ? 'REFUSED' : VERDICTS[code];
  return {
    verdict: verdict ?? `UNKNOWN CODE ${code}`,
    // `memory.grow` has no inverse, so this is the high-water mark of the whole
    // run rather than what is live at the end of it.
    peak: exports.memory.buffer.byteLength,
    seconds,
  };
}

// ---------------------------------------------------------------------------

const options = parseArgs(process.argv.slice(2));

if (!existsSync(options.module)) {
  console.error(`cannot read ${options.module}`);
  console.error(
    'build it first: cargo build --profile release-wasm ' +
      '--target wasm32-unknown-unknown -p refute-wasm',
  );
  process.exit(1);
}
if (!existsSync(options.binary)) {
  console.error(`cannot read ${options.binary}`);
  console.error('build it first: cargo build');
  process.exit(1);
}

const moduleBytes = readFileSync(options.module);

const pairs = pairsFromTests();
if (options.extra !== null) {
  pairs.push(...pairsFromDirectory(options.extra));
}

const failures = [];
const seenVerdicts = new Set();
let checked = 0;

if (!options.quiet) {
  console.log(
    `${'proof'.padEnd(42)} ${'formula'.padEnd(28)} ` +
      `${'native'.padEnd(13)} ${'wasm'.padEnd(13)} ${'peak'.padEnd(9)} wasm`,
  );
}

for (const pair of pairs) {
  const dir = pair.dir ?? FIXTURES;
  const cnfPath = join(dir, pair.cnf);
  const proofPath = join(dir, pair.proof);
  if (!existsSync(cnfPath) || !existsSync(proofPath)) {
    failures.push(`${pair.proof}: named by ${pair.from} but not on disk`);
    continue;
  }

  const native = nativeVerdict(options.binary, cnfPath, proofPath);
  const { verdict, peak, seconds } = wasmVerdict(
    moduleBytes,
    readFileSync(cnfPath),
    readFileSync(proofPath),
  );
  checked += 1;
  seenVerdicts.add(native);

  const agree = native === verdict;
  if (!agree) {
    failures.push(
      `${pair.proof} against ${pair.cnf}: native says ${native}, ` +
        `the module says ${verdict}`,
    );
  }
  if (!options.quiet || !agree) {
    const megabytes = Number.isNaN(peak)
      ? '-'
      : `${(peak / (1024 * 1024)).toFixed(1)} MB`;
    const took = Number.isNaN(seconds) ? '-' : `${seconds.toFixed(2)} s`;
    console.log(
      `${pair.proof.padEnd(42)} ${pair.cnf.padEnd(28)} ` +
        `${native.padEnd(13)} ${verdict.padEnd(13)} ${megabytes.padEnd(9)} ${took}` +
        `${agree ? '' : '   <-- DISAGREE'}`,
    );
  }
}

// W1's coverage, stated rather than assumed. A proof fixture no test names is
// a proof fixture this harness never runs, and silence about it would read as
// having covered everything.
const uncovered = uncoveredProofs(pairsFromTests());
if (uncovered.length > 0) {
  failures.push(
    `${uncovered.length} proof fixture(s) are named by no test and so were ` +
      `never checked: ${uncovered.join(', ')}`,
  );
}

// W2. A module that collapsed UNSUPPORTED into NOT VERIFIED would agree with
// the native checker on every fixture that is not binary, and this is the line
// that notices.
for (const verdict of VERDICTS) {
  if (!seenVerdicts.has(verdict)) {
    failures.push(
      `no pair produced ${verdict}; the corpus must exercise all three ` +
        'verdicts or agreement means less than it looks like it does',
    );
  }
}

// ---------------------------------------------------------------------------
// W5, the size refusal, in the environment it exists for.
//
// The native checker has no such ceiling, so there is nothing here to compare
// against and this is an assertion rather than an agreement. It runs in wasm
// rather than only in the Rust unit tests because the failure it prevents is a
// browser one: a tab that dies with no explanation instead of a page that says
// what happened.

function boundary() {
  const results = [];

  if (PAGE_CEILING !== MODULE_CEILING) {
    failures.push(
      `page/limits.js says ${PAGE_CEILING} and wasm/src/lib.rs says ` +
        `${MODULE_CEILING}. The page refuses before it instantiates anything, ` +
        'so the two numbers have to be the same one written twice, and this ' +
        'is the only thing keeping them that way.',
    );
  }

  const overInstance = new WebAssembly.Instance(
    new WebAssembly.Module(moduleBytes),
    {},
  );
  const over = overInstance.exports;
  const overOffset = over.proof_reserve(MAX_INPUT_BYTES + 1);
  let overCode;
  try {
    overCode = over.check();
  } catch (err) {
    failures.push(`one byte over the ceiling trapped instead of refusing: ${err.message}`);
    overCode = null;
  }
  if (overOffset !== 0) {
    failures.push(
      `proof_reserve(MAX_INPUT_BYTES + 1) returned offset ${overOffset}; ` +
        'a refusal must be offset 0, or the page cannot tell one from a place ' +
        'to write',
    );
  }
  if (overCode !== null && overCode !== REFUSED_CODE) {
    failures.push(
      `one byte over the ceiling returned ${overCode}, not ${REFUSED_CODE}; ` +
        'the module reported a verdict on a file it never held',
    );
  }
  results.push(
    `one byte over  offset ${overOffset}, check ${overCode} ` +
      `(peak ${(over.memory.buffer.byteLength / (1024 * 1024)).toFixed(1)} MB, ` +
      'nothing allocated for the refused input)',
  );

  // And the ceiling itself, on its own instance: a boundary written `>=`
  // instead of `>` refuses a file the module can hold, and no agreement test
  // in this harness would ever notice.
  const atInstance = new WebAssembly.Instance(
    new WebAssembly.Module(moduleBytes),
    {},
  );
  const at = atInstance.exports;
  const atOffset = at.proof_reserve(MAX_INPUT_BYTES);
  if (atOffset === 0) {
    failures.push(
      `proof_reserve(MAX_INPUT_BYTES) was refused; the ceiling itself must be ` +
        'accepted, or wasm/src/lib.rs and this harness disagree about where it is',
    );
  }
  results.push(
    `at the ceiling  offset ${atOffset}, ` +
      `peak ${(at.memory.buffer.byteLength / (1024 * 1024)).toFixed(1)} MB`,
  );

  return results;
}

console.log('');
console.log(`ceiling         ${MAX_INPUT_BYTES} bytes per input (page and module agree)`);
for (const line of boundary()) {
  console.log(`  ${line}`);
}

console.log('');
console.log(`pairs checked   ${checked}`);
console.log(`verdicts seen   ${[...seenVerdicts].sort().join(', ')}`);
console.log(`module          ${options.module} (${statSync(options.module).size} bytes)`);
console.log(`native          ${options.binary}`);

if (failures.length > 0) {
  console.error('');
  for (const failure of failures) {
    console.error(`FAIL  ${failure}`);
  }
  process.exit(1);
}
console.log('agreement       every pair');
