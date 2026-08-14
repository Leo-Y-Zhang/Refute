// Assembles the playground into one directory, ready to publish.
//
// Nothing in `page/` is a copy of anything. The module is a build output and
// the examples are test fixtures, and both would go stale the moment they were
// duplicated into a directory nobody rebuilds. So they are brought together
// here instead, and `tools/serve_page.mjs` maps exactly the same three rules so
// that what a developer opens and what a reader opens are assembled the same
// way.
//
//     page/*                 as they are
//     refute_wasm.wasm       from target/wasm32-unknown-unknown/release-wasm/
//     examples/<name>        from tests/fixtures/<name>
//
// The example list is not written here either. It is read out of `page/main.js`,
// so a sixth example added to the page is published without anyone remembering
// to add it, and an example naming a file that does not exist is a build
// failure rather than a broken button.
//
//     node tools/build_page.mjs [--out dist]

// Paths in this file are relative to the repository root, so the process moves
// there first. Running `node tools/<this>.mjs` from a home directory is the
// obvious thing to try and used to fail with a module-not-found error that
// named a path nobody had typed.
import { chdir } from 'node:process';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
chdir(resolvePath(dirname(fileURLToPath(import.meta.url)), '..'));

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from 'node:fs';
import { join } from 'node:path';

const MODULE_PATH =
  'target/wasm32-unknown-unknown/release-wasm/refute_wasm.wasm';
const FIXTURES = join('tests', 'fixtures');

let out = 'dist';
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === '--out') {
    i += 1;
    out = process.argv[i];
  }
}

/** The fixture names `page/main.js` asks for, in the order it lists them. */
function examplesFromPage() {
  const code = readFileSync(join('page', 'main.js'), 'utf8')
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n');
  const names = new Set();
  for (const match of code.matchAll(
    /\b(?:cnf|proof):\s*'([A-Za-z0-9_]+\.(?:cnf|lrat|drat))'/g,
  )) {
    names.add(match[1]);
  }
  if (names.size === 0) {
    throw new Error(
      'found no example files named in page/main.js; either the page stopped ' +
        'having examples or this pattern stopped matching them, and both are ' +
        'worth failing over',
    );
  }
  return [...names].sort();
}

if (!existsSync(MODULE_PATH)) {
  console.error(`the module is not built: ${MODULE_PATH}`);
  console.error(
    'cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm',
  );
  process.exit(1);
}

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'examples'), { recursive: true });

let total = 0;
function take(from, to) {
  copyFileSync(from, to);
  const size = statSync(to).size;
  total += size;
  console.log(`  ${to.padEnd(40)} ${String(size).padStart(9)} bytes`);
}

console.log(`building ${out}`);
for (const name of readdirSync('page')) {
  take(join('page', name), join(out, name));
}
take(MODULE_PATH, join(out, 'refute_wasm.wasm'));
for (const name of examplesFromPage()) {
  const source = join(FIXTURES, name);
  if (!existsSync(source)) {
    console.error(
      `page/main.js names ${name}, which is not in ${FIXTURES}. An example ` +
        'button that fetches a 404 is worse than one that is not there.',
    );
    process.exit(1);
  }
  take(source, join(out, 'examples', name));
}

console.log('');
console.log(`total    ${total} bytes`);
