// The static server behind both `tools/serve_page.mjs` and
// `tools/browser_check.mjs`.
//
// It is a module rather than a script because the browser check used to spawn
// the script as a child process, and that cost a CI job. The child inherited
// stderr, and a step does not finish until every inherited pipe closes, so a
// check that had already decided its answer sat there holding the job open
// until the fifteen-minute timeout killed it. An in-process server has no pipe,
// no handshake, and nothing to leave behind.
//
// Three rules, and `tools/build_page.mjs` performs the same three when it
// assembles a directory to publish, so that what a developer opens and what a
// reader opens are put together the same way:
//
//     /                     page/index.html
//     /refute_wasm.wasm     target/wasm32-unknown-unknown/release-wasm/
//     /examples/<name>      tests/fixtures/<name>
//     everything else       page/<name>
//
// With a `root`, a directory already built by `build_page.mjs` is served as it
// is, because the mapping already happened.

import { createServer } from 'node:http';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';

export const MODULE_PATH =
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

/** The file a request path names, or null if it names nothing we serve. */
export function resolvePath(urlPath, root) {
  const clean = normalize(decodeURIComponent(urlPath)).replace(/\\/g, '/');
  // A path that climbs out of the tree is not a path we serve. This process can
  // read the whole disk; the page is allowed three directories of it.
  if (clean.includes('..')) {
    return null;
  }
  if (root !== null && root !== undefined) {
    const name = clean === '/' ? 'index.html' : clean.slice(1);
    return name.includes('..') ? null : join(root, name);
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

/**
 * A static server for the playground. Nothing else: no listing, no upload, no
 * state.
 *
 * Returns a `node:http` server that has not been told to listen yet.
 */
export function createPageServer({ root = null } = {}) {
  return createServer((request, response) => {
    const path = resolvePath(
      new URL(request.url, 'http://localhost').pathname,
      root,
    );
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
}
