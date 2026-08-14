// The playground, served from a working tree, with nothing copied.
//
// A thin command-line front for `tools/page_server.mjs`, which is where the
// three mapping rules live and which `tools/browser_check.mjs` now uses
// directly rather than by spawning this script.
//
//     node tools/serve_page.mjs [--port 8080] [--root dist] [--lan]
//
// With --root it serves a directory built by tools/build_page.mjs exactly as
// it is, which is what a reader would actually be given.
//
// With --lan it binds every interface instead of loopback, and prints the
// addresses a phone on the same network can reach. That exists for one
// measurement: rollback step 2 of docs/TDD.md part 5 requires the page to be
// checked on a phone as well as a desktop, and a phone cannot reach
// 127.0.0.1. It means the page can be measured on a phone *before* it is
// published rather than after, which is the order that gate is written in.
// WebAssembly and workers both run over plain HTTP, so no certificate is
// needed. Loopback is still the default: --lan opens a port to the network and
// should be a thing someone typed on purpose.
//
// It exists because `file://` cannot start a worker or fetch a sibling, so
// there is no way to open this page without a server, and because a browser
// check ought to be something anyone can run.

// Paths in this file are relative to the repository root, so the process moves
// there first. Running `node tools/<this>.mjs` from a home directory is the
// obvious thing to try and used to fail with a module-not-found error that
// named a path nobody had typed.
import { chdir } from 'node:process';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
chdir(resolvePath(dirname(fileURLToPath(import.meta.url)), '..'));

import { existsSync } from 'node:fs';
import { networkInterfaces } from 'node:os';

import { createPageServer, MODULE_PATH } from './page_server.mjs';

let port = 8080;
let root = null;
let lan = false;
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === '--port') {
    i += 1;
    port = Number(process.argv[i]);
  } else if (process.argv[i] === '--root') {
    i += 1;
    root = process.argv[i];
  } else if (process.argv[i] === '--lan') {
    lan = true;
  }
}

/** Every IPv4 address a device on the same network could use. */
function lanAddresses() {
  return Object.values(networkInterfaces())
    .flat()
    .filter((i) => i && i.family === 'IPv4' && !i.internal)
    .map((i) => i.address);
}

const server = createPageServer({ root });
server.listen(port, lan ? '0.0.0.0' : '127.0.0.1', () => {
  if (root === null && !existsSync(MODULE_PATH)) {
    console.log(
      'the module is not built yet; the page will report that the checker ' +
        'could not start until you run\n' +
        '  cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm',
    );
  }
  const where = root === null ? '' : `   (${root})`;
  console.log(`http://127.0.0.1:${port}/${where}`);
  if (lan) {
    for (const address of lanAddresses()) {
      console.log(`http://${address}:${port}/${where}`);
    }
    console.log(
      '\nOpen one of those on a phone on the same network. Windows Firewall ' +
        'will ask once; allow it on private networks only, and stop this ' +
        'server when the measurement is done.',
    );
  }
});
