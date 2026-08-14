// The playground, in a real browser, driven rather than described.
//
// Node is not a browser, and this milestone's whole surface is the browser.
// `tools/wasm_agreement.mjs` proves the module agrees with the checker; this
// proves the page reaches the module at all, that the verdict a reader sees is
// the one the module gave, and that nothing leaves the origin.
//
// It answers two of the three things `docs/TDD.md` part 5 makes the page's
// release conditional on:
//
//   rollback step 2, desktop half. Each example is loaded and its verdict read
//   out of the DOM, with the peak memory the page reports. The phone half is
//   not automated and is still the owner's to do.
//   rollback step 3. Every network request the page makes is recorded from the
//   protocol, not from a screenshot of a tab, and any request to another origin
//   is a failure. That is the privacy claim, checked.
//
// Usage:
//
//     node tools/browser_check.mjs [--browser <path>] [--port 8081] [--root dist]
//                                    [--keep-open]
//
// With --root it drives the built artefact rather than the working tree, which
// is the version a reader would actually get.
//
// It starts its own static server and its own browser, and stops both.

import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { createPageServer } from './page_server.mjs';

const BROWSERS = [
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
];

/**
 * What each example must report, and it is the CLI that decides these.
 *
 * The third column is W7: an `UNSUPPORTED` verdict has to tell the reader what
 * to do about it, or it is a dead end wearing a verdict's clothes.
 */
const EXPECTED = [
  ['tiny', 'VERIFIED'],
  ['vdw-n21', 'VERIFIED'],
  ['pigeonhole', 'VERIFIED'],
  ['corrupted', 'NOT VERIFIED'],
  ['binary', 'UNSUPPORTED', '--no-binary'],
];

let browserPath = BROWSERS.find((p) => existsSync(p));
let port = 8081;
let root = null;
let keepOpen = false;
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (arg === '--browser') {
    i += 1;
    browserPath = process.argv[i];
  } else if (arg === '--port') {
    i += 1;
    port = Number(process.argv[i]);
  } else if (arg === '--root') {
    i += 1;
    root = process.argv[i];
  } else if (arg === '--keep-open') {
    keepOpen = true;
  }
}

if (browserPath === undefined) {
  console.error('no Chromium-based browser found; pass --browser <path>');
  process.exit(1);
}

const origin = `http://127.0.0.1:${port}`;
const failures = [];

// Nothing in this script may wait forever.
//
// The first version could, in three places, and it burned ten minutes of a
// local run and most of a CI job before anyone found out. A harness that hangs
// tells you less than one that fails: a failure names the step it died on.
const WATCHDOG_MS = Number(process.env.REFUTE_BROWSER_TIMEOUT_MS ?? 300000);
const watchdog = setTimeout(() => {
  console.error(
    `
FAIL  the browser check did not finish within ${WATCHDOG_MS} ms. ` +
      `Last step reached: ${stage}.`,
  );
  process.exit(1);
}, WATCHDOG_MS);

/** What the script is waiting on, for the watchdog to name. */
let stage = 'startup';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// ---------------------------------------------------------------------------
// A very small DevTools protocol client

class Devtools {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = [];
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (message.id !== undefined) {
        const resolve = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (resolve !== undefined) {
          resolve(message);
        }
      } else {
        for (const listener of this.listeners) {
          listener(message);
        }
      }
    });
  }

  static async open(url) {
    const socket = new WebSocket(url);
    const opened = await Promise.race([
      new Promise((resolve) => {
        socket.addEventListener('open', () => resolve(true), { once: true });
        socket.addEventListener('error', () => resolve(false), { once: true });
      }),
      sleep(20000).then(() => false),
    ]);
    if (!opened) {
      throw new Error(`could not open a DevTools connection to ${url}`);
    }
    return new Devtools(socket);
  }

  /**
   * Sends one command and waits for its reply, but not forever.
   *
   * A reply that never comes is exactly what a closed target, a detached
   * session or a dead socket produces, and an unbounded await on one is how
   * this check spent two CI jobs sitting still. The timeout resolves rather
   * than rejects, so a caller gets `null` and the run carries on to report
   * something.
   */
  send(method, params = {}, sessionId = undefined, timeoutMs = 30000) {
    const id = this.nextId;
    this.nextId += 1;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve(null);
      }, timeoutMs);
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        resolve(message);
      });
      const message = { id, method, params };
      if (sessionId !== undefined) {
        message.sessionId = sessionId;
      }
      this.socket.send(JSON.stringify(message));
    });
  }

  on(listener) {
    this.listeners.push(listener);
  }

  /** Evaluates an expression in the page and returns its value. */
  async evaluate(expression) {
    const reply = await this.send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (reply === null) {
      throw new Error(`the page never answered ${expression}`);
    }
    if (reply.result?.exceptionDetails !== undefined) {
      throw new Error(
        `evaluating in the page threw: ${reply.result.exceptionDetails.text}`,
      );
    }
    return reply.result?.result?.value;
  }

  close() {
    this.socket.close();
  }
}

/**
 * Polls until the predicate gives something, or the deadline passes.
 *
 * The deadline is raced against the predicate rather than checked after it.
 * Checked after, a predicate that hangs makes the whole loop hang and the
 * timeout becomes decorative - which is what it was, and it is why a job that
 * had a thirty-second budget per example ran for fifteen minutes.
 */
async function until(predicate, { timeoutMs = 60000, everyMs = 100 } = {}) {
  const deadline = Date.now() + timeoutMs;
  const expired = Symbol('expired');
  for (;;) {
    const remaining = deadline - Date.now();
    if (remaining <= 0) {
      return null;
    }
    const value = await Promise.race([
      Promise.resolve().then(predicate).catch(() => null),
      sleep(remaining).then(() => expired),
    ]);
    if (value === expired) {
      return null;
    }
    if (value !== null && value !== undefined && value !== false) {
      return value;
    }
    await sleep(everyMs);
  }
}

// ---------------------------------------------------------------------------

const profile = mkdtempSync(join(tmpdir(), 'refute-browser-'));

// In this process, deliberately.
//
// This used to spawn `tools/serve_page.mjs` as a child with stderr inherited. A
// GitHub Actions step does not finish until every inherited pipe closes, so a
// check that had already decided its answer held the job open until the
// fifteen-minute timeout killed it — twice. There is no child, no pipe and no
// handshake now.
stage = 'starting the static server';
const server = createPageServer({ root });
await new Promise((resolve, reject) => {
  server.once('error', reject);
  server.listen(port, '127.0.0.1', resolve);
});

const browser = spawn(
  browserPath,
  [
    '--headless=new',
    '--remote-debugging-port=9333',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    '--disable-background-networking',
    '--disable-component-update',
    // No first-party requests to anywhere but our own server, so that a
    // request to another origin is the page's doing and nothing else's.
    '--disable-sync',
    // Chrome's own sandbox cannot start in most CI containers, and a browser
    // that will not start is a check that never runs. Only where CI says so,
    // and never on a developer's machine.
    ...(process.env.CI ? ['--no-sandbox', '--disable-dev-shm-usage'] : []),
    `--user-data-dir=${profile}`,
    'about:blank',
  ],
  { stdio: ['ignore', 'ignore', 'ignore'] },
);

function shutdown() {
  if (!keepOpen) {
    browser.kill();
  }
  server.close();
  server.closeAllConnections?.();
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {
    // A profile directory the browser still holds open is not a test failure.
  }
}

// A backstop, not the plan. `exit` only fires once the event loop is empty,
// and a listening server is exactly what keeps it from being — so relying on
// this alone deadlocked: the check printed its whole report and then sat there
// holding the CI step open, having already decided the answer. `shutdown()` is
// called explicitly on both paths below.
process.on('exit', shutdown);

// The debugging port takes a moment to answer.
stage = 'waiting for the browser debugging port';
const targets = await until(async () => {
  try {
    const reply = await fetch('http://127.0.0.1:9333/json/list');
    return await reply.json();
  } catch {
    return null;
  }
}, { timeoutMs: 30000 });

if (targets === null) {
  console.error('the browser never opened its debugging port');
  process.exit(1);
}

// Not simply the first page target. Edge opens a `edge://sync-confirmation-dialog`
// on a fresh profile and reports it as a page, ahead of `about:blank`; driving
// that one navigates nothing, reaches no verdict, and looks exactly like a
// hanging check. Take a target the page could actually be.
const target = targets.find(
  (t) =>
    t.type === 'page' &&
    !t.url.startsWith('edge://') &&
    !t.url.startsWith('chrome://') &&
    !t.url.startsWith('devtools://'),
);
if (target === undefined) {
  console.error(
    'the browser opened no usable page target; it offered ' +
      targets.map((t) => `${t.type} ${t.url}`).join(', '),
  );
  process.exit(1);
}
const devtools = await Devtools.open(target.webSocketDebuggerUrl);

const requests = [];
const consoleErrors = [];
devtools.on((message) => {
  if (message.method === 'Network.requestWillBeSent') {
    requests.push(message.params.request.url);
  }
  if (
    message.method === 'Runtime.consoleAPICalled' &&
    message.params.type === 'error'
  ) {
    consoleErrors.push(
      message.params.args.map((a) => a.value ?? a.description).join(' '),
    );
  }
  if (message.method === 'Runtime.exceptionThrown') {
    consoleErrors.push(
      message.params.exceptionDetails.text ??
        JSON.stringify(message.params.exceptionDetails),
    );
  }
});

await devtools.send('Network.enable');
await devtools.send('Runtime.enable');
await devtools.send('Page.enable');

// The workers, too, and this is not a refinement.
//
// The first version of this check enabled Network on the page target alone and
// reported that every request stayed on the origin. It was not seeing the
// worker's requests at all — `refute_wasm.wasm` itself, the largest thing the
// page fetches, was missing from its own list, and nobody would have noticed
// because the list looked complete. A worker is precisely where a call to
// somewhere else would be least visible, so the privacy claim has to be checked
// where it would be hidden.
//
// `flatten: true` makes worker events arrive on this same socket with a
// sessionId, so one listener sees all of them.
await devtools.send('Target.setAutoAttach', {
  autoAttach: true,
  waitForDebuggerOnStart: false,
  flatten: true,
});
const attachedSessions = new Set();
devtools.on((message) => {
  if (message.method === 'Target.attachedToTarget') {
    const { sessionId } = message.params;
    attachedSessions.add(`${message.params.targetInfo.type} ${message.params.targetInfo.url}`);
    void devtools.send('Network.enable', {}, sessionId);
    void devtools.send('Runtime.enable', {}, sessionId);
  }
});

console.log(`browser  ${browserPath}`);
console.log(`origin   ${origin}`);
console.log(`serving  ${root ?? 'the working tree'}`);
console.log('');
console.log(
  `${'example'.padEnd(12)} ${'expected'.padEnd(13)} ${'reported'.padEnd(13)} ` +
    `${'peak'.padEnd(9)} time`,
);

for (const [id, expected, mustMention] of EXPECTED) {
  stage = `checking the ${id} example`;
  process.stdout.write(`${id.padEnd(12)} ${expected.padEnd(13)} `);
  await devtools.send('Page.navigate', { url: `${origin}/?example=${id}` });
  await sleep(200);

  const state = await until(
    async () =>
      (await devtools.evaluate(
        "document.getElementById('verdict').className",
      ))?.includes('done')
        ? await devtools.evaluate(
            "document.getElementById('verdict').innerText",
          )
        : null,
    { timeoutMs: 30000 },
  );

  if (state === null) {
    failures.push(`${id}: the page never reached a verdict`);
    console.log('(timed out)');
    continue;
  }

  const reported = await devtools.evaluate(
    "document.querySelector('#verdict .word')?.innerText.replace(/^\\S+\\s*/, '').trim() ?? ''",
  );
  const facts = await devtools.evaluate(
    "JSON.stringify(Array.from(document.querySelectorAll('#verdict .facts dd')).map(d => d.innerText))",
  );
  const [, , took = '-', peak = '-'] = JSON.parse(facts ?? '[]');

  if (mustMention !== undefined && !String(state).includes(mustMention)) {
    failures.push(
      `${id}: the panel never mentions ${JSON.stringify(mustMention)}. An ` +
        'UNSUPPORTED verdict has to say what to do about it.',
    );
  }

  const agrees = reported === expected;
  if (!agrees) {
    failures.push(
      `${id}: the page reported ${JSON.stringify(reported)}, expected ` +
        `${JSON.stringify(expected)}`,
    );
  }
  console.log(
    `${reported.padEnd(13)} ${peak.padEnd(9)} ${took}` +
      `${agrees ? '' : '   <-- DISAGREE'}`,
  );
}

// ---------------------------------------------------------------------------
// Rollback step 3, from the protocol rather than from a screenshot.

const foreign = [...new Set(requests)].filter((url) => !url.startsWith(origin));
console.log('');
console.log(`workers seen    ${attachedSessions.size ? [...attachedSessions].join(', ') : 'none'}`);
console.log(`requests        ${requests.length}, ${new Set(requests).size} distinct`);

// If the module itself never appears, this check is not watching the worker,
// and a clean report would mean nothing. It is the one request that must be
// there.
if (!requests.some((url) => url.endsWith('refute_wasm.wasm'))) {
  failures.push(
    'no request for refute_wasm.wasm was recorded, so the worker was not ' +
      'being watched. A privacy claim checked only where nothing happens is ' +
      'not a checked privacy claim.',
  );
}
for (const url of [...new Set(requests)].sort()) {
  console.log(`  ${url.startsWith(origin) ? url.slice(origin.length) : url}`);
}
if (foreign.length > 0) {
  failures.push(
    `the page made ${foreign.length} request(s) off its own origin: ` +
      `${foreign.join(', ')}. "Your files are never uploaded" has to be true ` +
      'of every request, not only of the ones carrying a file.',
  );
}

if (consoleErrors.length > 0) {
  failures.push(`the page logged ${consoleErrors.length} error(s):`);
  for (const error of consoleErrors) {
    failures.push(`  ${error}`);
  }
}

if (failures.length > 0) {
  console.error('');
  for (const failure of failures) {
    console.error(`FAIL  ${failure}`);
  }
  devtools.close();
  shutdown();
  process.exit(1);
}
clearTimeout(watchdog);
console.log('');
console.log('every example reported the verdict the CLI reports');
console.log('every request stayed on this page\'s own origin');
devtools.close();
shutdown();
