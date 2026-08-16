// The page. No framework, no build step for this file, no dependency.
//
// Its whole job is to get two byte arrays to a worker and put one word back on
// the screen. Everything that could report a verdict lives in the module; this
// file must never decide one, and the only place a verdict word is written is
// the render below, from a message the worker sent.

import { MAX_INPUT_BYTES, MAX_INPUT_LABEL } from './limits.js';

const MODULE_URL = 'refute_wasm.wasm';

/**
 * The preloaded examples.
 *
 * Every one of these files is in `tests/fixtures`, is checked by `cargo test`
 * on every push, and is served here by the build rather than copied into the
 * repository twice. `expect` is what the CLI says about it, shown before the
 * check runs so that a page reporting something else is obviously wrong rather
 * than quietly wrong.
 */
const EXAMPLES = [
  {
    id: 'tiny',
    label: 'A tiny refutation',
    detail: '8 clauses, LRAT',
    cnf: 'tiny_unsat.cnf',
    proof: 'tiny_unsat.lrat',
    expect: 'VERIFIED',
  },
  {
    id: 'vdw-n21',
    label: 'A published result',
    detail: 'van der Waerden A217058, n=21, raw DRAT',
    cnf: 'vdw_a217058_n21.cnf',
    proof: 'vdw_a217058_n21.drat',
    expect: 'VERIFIED',
  },
  {
    id: 'pigeonhole',
    label: 'Pigeonhole 7x6',
    detail: '55 KB of LRAT, with RAT steps',
    cnf: 'rat_pigeonhole.cnf',
    proof: 'rat_pigeonhole.lrat',
    expect: 'VERIFIED',
  },
  {
    id: 'corrupted',
    label: 'A corrupted proof',
    detail: 'one hint redirected to another clause',
    cnf: 'n01_hint_redirected.cnf',
    proof: 'n01_hint_redirected.lrat',
    expect: 'NOT VERIFIED',
  },
  {
    id: 'binary',
    label: 'A binary proof',
    detail: 'a construct this checker does not read',
    cnf: 'b17_binary_proof.cnf',
    proof: 'b17_binary_proof.lrat',
    expect: 'UNSUPPORTED',
  },
];

const SHAPES = {
  VERIFIED: '✓',
  'NOT VERIFIED': '✕',
  UNSUPPORTED: '—',
};

const dom = {
  examples: document.getElementById('examples'),
  cnfInput: document.getElementById('cnf-input'),
  proofInput: document.getElementById('proof-input'),
  cnfChosen: document.getElementById('cnf-chosen'),
  proofChosen: document.getElementById('proof-chosen'),
  cnfDrop: document.getElementById('cnf-drop'),
  proofDrop: document.getElementById('proof-drop'),
  run: document.getElementById('run'),
  cancel: document.getElementById('cancel'),
  verdict: document.getElementById('verdict'),
};

/** The two files, as `{ name, bytes }` or null. */
const chosen = { cnf: null, proof: null };

/**
 * What each slot's label says when nothing is in it.
 *
 * Read out of the markup rather than written here, so that emptying a slot
 * restores what index.html says an empty slot reads and not what this file
 * remembers it said.
 */
const EMPTY_LABEL = {
  cnf: dom.cnfChosen.textContent,
  proof: dom.proofChosen.textContent,
};

let worker = null;
let ticker = null;

// ---------------------------------------------------------------------------
// Rendering. One function per state of the verdict panel, and nothing else
// writes to it.
//
// Every node here is built and given `textContent`; no string of HTML is ever
// assigned anywhere in this file. Two of the values interpolated below are
// chosen by whoever supplies the files - the formula's name and the proof's -
// and milestone 1 has already had this bug once, in the other direction: the
// CLI echoed terminal escape bytes out of a hostile formula and let it repaint
// the verdict line above it. `innerHTML` with an escaper is the same bet on the
// same escaper being right. `textContent` is not a bet.

/** A node, with a class and a run of text. */
function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className !== undefined && className !== null) {
    node.className = className;
  }
  if (text !== undefined && text !== null) {
    node.textContent = text;
  }
  return node;
}

/** A `<dt>`/`<dd>` pair. */
function fact(term, value) {
  return [el('dt', null, term), el('dd', null, value)];
}

function panel(nodes, className) {
  dom.verdict.className = `verdict ${className}`;
  dom.verdict.replaceChildren(...nodes);
}

/** The verdict word, with a shape beside it so colour is never the carrier. */
function wordLine(word, shape) {
  const line = el('p', 'word');
  const mark = el('span', null, shape);
  mark.setAttribute('aria-hidden', 'true');
  line.append(mark, document.createTextNode(` ${word}`));
  return line;
}

function renderChecking(elapsed) {
  // No step count: the module reports a verdict and nothing else in this
  // commit, so a progress bar would be an animation rather than information.
  // Elapsed time is true.
  panel(
    [
      el('p', 'state', 'Checking...'),
      el(
        'p',
        'detail',
        `${elapsed.toFixed(1)} s elapsed. The tab is responsive because the ` +
          'check is on a worker; cancel stops it immediately.',
      ),
    ],
    'checking',
  );
}

function renderVerdict(word, seconds, peakBytes, files) {
  const facts = el('dl', 'facts');
  facts.append(
    ...fact('Formula', files.cnf),
    ...fact('Proof', files.proof),
    ...fact('Checked in', `${seconds.toFixed(2)} s`),
    ...fact('Peak memory', `${(peakBytes / (1024 * 1024)).toFixed(1)} MB`),
  );
  panel(
    [
      wordLine(word, SHAPES[word] ?? ''),
      facts,
      el('p', 'detail', explain(word)),
    ],
    `done ${word === 'VERIFIED' ? 'ok' : 'not-ok'}`,
  );
  dom.verdict.focus();
}

function explain(word) {
  switch (word) {
    case 'VERIFIED':
      return 'Every step was checked and the sequence derives the empty clause. The formula has no satisfying assignment.';
    case 'NOT VERIFIED':
      return 'The proof was read and found wanting. Which step, which line and why is what the command-line tool prints; this page does not, yet.';
    case 'UNSUPPORTED':
      return 'The proof uses a construct this checker does not check, so it is not a pass. A binary proof is the usual cause: re-run the solver with --no-binary.';
    default:
      return '';
  }
}

/** A `<pre><code>` block. */
function code(text) {
  const block = el('pre');
  block.append(el('code', null, text));
  return block;
}

function renderTooLarge(which, name, bytes) {
  // The one failure that must not look transient. No retry button, and the
  // exact command instead.
  //
  // `which` is which of the two arguments the refused file was, and it is a
  // parameter rather than an assumption because both drop zones reach here.
  // A refused formula printed into the proof position is a command that checks
  // the user's formula against whatever was loaded before it, which is worse
  // than printing nothing.
  const megabytes = (bytes / (1024 * 1024)).toFixed(1);
  const command =
    which === 'cnf'
      ? `refute ${name} ${chosen.proof?.name ?? 'proof.drat'}`
      : `refute ${chosen.cnf?.name ?? 'formula.cnf'} ${name}`;
  panel(
    [
      wordLine('Too large for a browser tab', '—'),
      el(
        'p',
        'detail',
        `${name} is ${megabytes} MB, and this page refuses above ` +
          `${MAX_INPUT_LABEL} because a tab has to hold the whole file before ` +
          'the checker can read a byte of it. The command-line tool streams ' +
          'the proof and has no such limit.',
      ),
      code(command),
    ],
    'done too-large',
  );
  dom.verdict.focus();
}

/** This device could not hold the files. Not a verdict, and not the ceiling. */
function renderExhausted(needBytes, detail) {
  const megabytes = (needBytes / (1024 * 1024)).toFixed(1);
  panel(
    [
      wordLine('Not enough memory on this device', '!'),
      el(
        'p',
        'detail',
        `This tab could not hold ${megabytes} MB of formula and proof. The ` +
          'page allows up to ' +
          `${MAX_INPUT_LABEL} per file, but that ceiling was measured on a ` +
          'desktop and your device has less to give. Nothing was checked, and ' +
          'this says nothing about the proof.',
      ),
      code(
        `refute ${chosen.cnf?.name ?? 'formula.cnf'} ${chosen.proof?.name ?? 'proof.drat'}`,
      ),
    ],
    'done too-large',
  );
  dom.verdict.focus();
}

function renderInternal(detail) {
  panel(
    [
      wordLine('Stopped without a verdict', '!'),
      el(
        'p',
        'detail',
        'The checker stopped part-way through. Either it ran out of memory on ' +
          'this device or it has a defect; the two look identical from here, ' +
          'and guessing between them would be worse than saying so. Either ' +
          'way it is not a statement about your proof — the command-line tool ' +
          'streams the proof and has no such limit.',
      ),
      code(
        `refute ${chosen.cnf?.name ?? 'formula.cnf'} ${chosen.proof?.name ?? 'proof.drat'}`,
      ),
      el('p', 'detail', detail),
    ],
    'done internal',
  );
  dom.verdict.focus();
}

/** An example that could not be fetched. Not a verdict, and never shown as one. */
function renderMissing(detail) {
  panel(
    [
      wordLine('Example not available', '!'),
      el(
        'p',
        'detail',
        'That example could not be loaded, so nothing was checked. This is a ' +
          'broken link on this page, not a statement about any proof.',
      ),
      code(detail),
    ],
    'done internal',
  );
  dom.verdict.focus();
}

function renderUnavailable(detail) {
  const link = el('a', null, 'Refute on GitHub');
  link.href = 'https://github.com/Leo-Y-Zhang/Refute';
  const linkLine = el('p', 'detail');
  linkLine.append(link);
  panel(
    [
      wordLine('The checker could not start', '!'),
      el(
        'p',
        'detail',
        'This page needs WebAssembly. The command-line tool needs nothing but ' +
          'a terminal.',
      ),
      linkLine,
      code(detail),
    ],
    'done internal',
  );
}

// ---------------------------------------------------------------------------
// Choosing files

function setChosen(which, name, bytes) {
  chosen[which] = { name, bytes };
  const label = which === 'cnf' ? dom.cnfChosen : dom.proofChosen;
  label.textContent = `${name} (${(bytes.byteLength / 1024).toFixed(1)} KB)`;
  dom.run.disabled = chosen.cnf === null || chosen.proof === null;
}

/** Empties one slot, so that nothing stale can be checked out of it. */
function clearChosen(which) {
  chosen[which] = null;
  const label = which === 'cnf' ? dom.cnfChosen : dom.proofChosen;
  label.textContent = EMPTY_LABEL[which];
  dom.run.disabled = true;
}

async function takeFile(which, file) {
  if (file.size > MAX_INPUT_BYTES) {
    // Cleared before the panel is drawn, and this is the same rule the module
    // keeps one layer down: a refusal must not leave the file it replaced
    // checkable. Left alone, the slot still holds the previous file under the
    // previous name, Check is still enabled, and the next click reports a
    // verdict about a file the user believes they have replaced.
    clearChosen(which);
    renderTooLarge(which, file.name, file.size);
    return;
  }
  const bytes = await file.arrayBuffer();
  setChosen(which, file.name, bytes);
}

function wireInput(which, input) {
  input.addEventListener('change', () => {
    const file = input.files?.[0];
    if (file !== undefined) {
      void takeFile(which, file);
    }
  });
}

function wireDropZone(which, zone) {
  // Drag and drop is an addition to the file input, never a replacement for
  // it: dropping is not operable by keyboard, and the input is.
  zone.addEventListener('dragover', (event) => {
    event.preventDefault();
    zone.classList.add('over');
  });
  zone.addEventListener('dragleave', () => zone.classList.remove('over'));
  zone.addEventListener('drop', (event) => {
    event.preventDefault();
    zone.classList.remove('over');
    const file = event.dataTransfer?.files?.[0];
    if (file !== undefined) {
      void takeFile(which, file);
    }
  });
}

// ---------------------------------------------------------------------------
// Running

function stopWorker() {
  if (worker !== null) {
    worker.terminate();
    worker = null;
  }
  if (ticker !== null) {
    clearInterval(ticker);
    ticker = null;
  }
  dom.cancel.hidden = true;
  dom.run.disabled = chosen.cnf === null || chosen.proof === null;
}

function run() {
  if (chosen.cnf === null || chosen.proof === null) {
    return;
  }
  stopWorker();

  const files = { cnf: chosen.cnf.name, proof: chosen.proof.name };
  const started = performance.now();
  renderChecking(0);
  ticker = setInterval(
    () => renderChecking((performance.now() - started) / 1000),
    100,
  );
  dom.run.disabled = true;
  dom.cancel.hidden = false;

  worker = new Worker('worker.js');
  worker.onmessage = (event) => {
    const message = event.data;
    stopWorker();
    switch (message.kind) {
      case 'verdict':
        renderVerdict(message.word, message.seconds, message.peakBytes, files);
        break;
      case 'refused':
        renderTooLarge('proof', files.proof, chosen.proof.bytes.byteLength);
        break;
      case 'exhausted':
        renderExhausted(message.needBytes, message.detail);
        break;
      case 'internal':
        renderInternal(message.detail);
        break;
      case 'unavailable':
        renderUnavailable(message.detail);
        break;
      default:
        renderInternal(`the worker sent ${JSON.stringify(message.kind)}`);
    }
  };
  worker.onerror = (event) => {
    stopWorker();
    renderInternal(event.message ?? 'the worker failed to start');
  };

  // The buffers are copied rather than transferred, so that checking the same
  // pair twice does not find them detached the second time.
  worker.postMessage({
    moduleUrl: MODULE_URL,
    cnf: chosen.cnf.bytes.slice(0),
    proof: chosen.proof.bytes.slice(0),
  });
}

function cancel() {
  stopWorker();
  panel(
    [
      el('p', 'state', 'Cancelled.'),
      el(
        'p',
        'detail',
        'The worker was terminated, which is also how its memory came back.',
      ),
    ],
    'idle',
  );
}

// ---------------------------------------------------------------------------
// Examples

async function loadExample(example) {
  panel([el('p', 'state', `Loading ${example.label}...`)], 'checking');
  try {
    // `response.ok` first, and this is not defensive habit. Without it a 404
    // hands `arrayBuffer()` the error page's body, the checker reads that as a
    // formula it cannot parse, and the panel says NOT VERIFIED — a verdict,
    // about a file that was never fetched. A stale build directory produced
    // exactly that, and it looked entirely convincing.
    const [cnf, proof] = await Promise.all(
      [example.cnf, example.proof].map(async (name) => {
        const response = await fetch(`examples/${name}`);
        if (!response.ok) {
          throw new Error(`examples/${name} gave HTTP ${response.status}`);
        }
        return response.arrayBuffer();
      }),
    );
    setChosen('cnf', example.cnf, cnf);
    setChosen('proof', example.proof, proof);
    run();
  } catch (error) {
    renderMissing(String(error));
  }
}

function renderExamples() {
  dom.examples.replaceChildren();
  for (const example of EXAMPLES) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'example';
    button.dataset.example = example.id;
    const label = document.createElement('span');
    label.className = 'example-label';
    label.textContent = example.label;
    const detail = document.createElement('span');
    detail.className = 'example-detail';
    detail.textContent = `${example.detail} — expect ${example.expect}`;
    button.append(label, detail);
    button.addEventListener('click', () => void loadExample(example));
    dom.examples.append(button);
  }
}

// ---------------------------------------------------------------------------
// Start

function start() {
  if (typeof WebAssembly !== 'object' || typeof Worker !== 'function') {
    renderUnavailable(
      typeof WebAssembly !== 'object'
        ? 'WebAssembly is not available in this browser.'
        : 'Web Workers are not available in this browser.',
    );
    dom.examples.replaceChildren();
    dom.run.disabled = true;
    return;
  }

  renderExamples();
  wireInput('cnf', dom.cnfInput);
  wireInput('proof', dom.proofInput);
  wireDropZone('cnf', dom.cnfDrop);
  wireDropZone('proof', dom.proofDrop);
  dom.run.addEventListener('click', run);
  dom.cancel.addEventListener('click', cancel);

  // ?example=vdw-n21 runs one on arrival, which is what a link in a paper or a
  // certificate note wants to do.
  const wanted = new URLSearchParams(location.search).get('example');
  if (wanted !== null) {
    const example = EXAMPLES.find((e) => e.id === wanted);
    if (example !== undefined) {
      void loadExample(example);
    }
  }
}

start();
