// The playground, served from a working tree, with nothing copied.
//
// A thin command-line front for `tools/page_server.mjs`, which is where the
// three mapping rules live and which `tools/browser_check.mjs` now uses
// directly rather than by spawning this script.
//
//     node tools/serve_page.mjs [--port 8080] [--root dist]
//
// With --root it serves a directory built by tools/build_page.mjs exactly as
// it is, which is what a reader would actually be given.
//
// It exists because `file://` cannot start a worker or fetch a sibling, so
// there is no way to open this page without a server, and because a browser
// check ought to be something anyone can run.

import { existsSync } from 'node:fs';

import { createPageServer, MODULE_PATH } from './page_server.mjs';

let port = 8080;
let root = null;
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === '--port') {
    i += 1;
    port = Number(process.argv[i]);
  } else if (process.argv[i] === '--root') {
    i += 1;
    root = process.argv[i];
  }
}

const server = createPageServer({ root });
server.listen(port, '127.0.0.1', () => {
  if (root === null && !existsSync(MODULE_PATH)) {
    console.log(
      'the module is not built yet; the page will report that the checker ' +
        'could not start until you run\n' +
        '  cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm',
    );
  }
  console.log(`http://127.0.0.1:${port}/${root === null ? '' : `   (${root})`}`);
});
