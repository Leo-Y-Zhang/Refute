// The playground, served from a working tree, with nothing copied.
//
// The page needs three things that do not live in `page/`: the compiled module,
// the example formulas and the example proofs. Committing copies of them under
// `page/` would put the same bytes in the repository twice and let one copy go
// stale, so this server maps them instead, and `tools/build_page.mjs` performs
// exactly the same mapping when it assembles a directory for publishing. The
// two are deliberately the same three rules:
//
//     /                     page/index.html
//     /refute_wasm.wasm     target/wasm32-unknown-unknown/release-wasm/
//     /examples/<name>      tests/fixtures/<name>
//     everything else       page/<name>
//
// A static file server and nothing else: no directory listing, no upload, no
// state. It exists so that the page can be opened at all — `file://` cannot
// start a worker or fetch a sibling — and so that a browser check is something
// anyone can run.
//
//     node tools/serve_page.mjs [--port 8080]

import { createServer } from 'node:http';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';

const MODULE_PATH =
  'target/wasm32-unknown-unknown/release-wasm/refute_wasm.wasm';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.cnf': 'text/plain; charset=utf-8',
  '.drat': 'text/plain; charset=utf-8',
  '.lrat': 'text/plain; charset=utf-8',
};

let port = 8080;
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === '--port') {
    i += 1;
    port = Number(process.argv[i]);
  }
}

/** The file a request path names, or null if it names nothing we serve. */
function resolve(urlPath) {
  const clean = normalize(decodeURIComponent(urlPath)).replace(/\\/g, '/');
  // A path that climbs out of the tree is not a path we serve. This process can
  // read the whole disk; the page is allowed three directories of it.
  if (clean.includes('..')) {
    return null;
  }
  if (clean === '/' || clean === '/index.html') {
    return join('page', 'index.html');
  }
  if (clean === '/refute_wasm.wasm') {
    return MODULE_PATH;
  }
  if (clean.startsWith('/examples/')) {
    const name = clean.slice('/examples/'.length);
    return name.includes('/') ? null : join('tests', 'fixtures', name);
  }
  const name = clean.slice(1);
  return name.includes('/') ? null : join('page', name);
}

const server = createServer((request, response) => {
  const path = resolve(new URL(request.url, 'http://localhost').pathname);
  if (path === null || !existsSync(path) || !statSync(path).isFile()) {
    response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    response.end(`not served: ${request.url}\n`);
    return;
  }
  response.writeHead(200, {
    'content-type': TYPES[extname(path)] ?? 'application/octet-stream',
    'cache-control': 'no-store',
  });
  createReadStream(path).pipe(response);
});

server.listen(port, '127.0.0.1', () => {
  if (!existsSync(MODULE_PATH)) {
    console.log(
      'the module is not built yet; the page will report that the checker ' +
        'could not start until you run\n' +
        '  cargo build --profile release-wasm --target wasm32-unknown-unknown -p refute-wasm',
    );
  }
  console.log(`http://127.0.0.1:${port}/`);
});
