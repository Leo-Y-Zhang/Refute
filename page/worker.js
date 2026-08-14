// One check, on one instance, off the main thread.
//
// The worker exists so that the tab never freezes, and so that cancelling is
// possible at all: the checker has no yield points, so there is nothing to poll
// and nothing to interrupt. `worker.terminate()` from the page is the cancel,
// and it is also how the memory comes back — `memory.grow` has no inverse, so
// a dropped instance is the only thing that reclaims linear memory, and a
// dropped worker is the surest way to drop an instance.
//
// A classic worker rather than a module worker: module workers are still not
// universal, and this file has nothing to import.

const VERIFIED = 0;
const NOT_VERIFIED = 1;
const UNSUPPORTED = 2;
const REFUSED = 3;

const WORDS = {
  [VERIFIED]: 'VERIFIED',
  [NOT_VERIFIED]: 'NOT VERIFIED',
  [UNSUPPORTED]: 'UNSUPPORTED',
};

self.onmessage = async (event) => {
  const { moduleUrl, cnf, proof } = event.data;

  let instance;
  try {
    // `instantiateStreaming` where the server sets the right type, and a plain
    // fetch where it does not. GitHub Pages serves .wasm as application/wasm;
    // a local static server may not, and a page that only worked on one of
    // them would be a page nobody could develop.
    const response = await fetch(moduleUrl);
    if (!response.ok) {
      throw new Error(`fetching the checker gave HTTP ${response.status}`);
    }
    if (response.headers.get('content-type') === 'application/wasm') {
      ({ instance } = await WebAssembly.instantiateStreaming(response, {}));
    } else {
      const bytes = await response.arrayBuffer();
      ({ instance } = await WebAssembly.instantiate(bytes, {}));
    }
  } catch (error) {
    self.postMessage({ kind: 'unavailable', detail: String(error) });
    return;
  }

  const exports = instance.exports;
  const cnfBytes = new Uint8Array(cnf);
  const proofBytes = new Uint8Array(proof);

  // The documented glue order, and not the obvious one. Growing linear memory
  // detaches every ArrayBuffer view of it, and JavaScript evaluates arguments
  // left to right, so
  //
  //     new Uint8Array(exports.memory.buffer, exports.cnf_reserve(n), n)
  //
  // reads the buffer, grows the memory, and hands Uint8Array a detached corpse.
  // Take every offset first; read memory.buffer last; never cache a view across
  // an export call.
  // Reserving is where the memory is actually taken, and it is the one step
  // that can fail on a device smaller than the one the ceiling was measured on.
  // The module's allocator has nothing to return but a trap when `memory.grow`
  // refuses, and an uncaught trap in a worker is a blank panel. This is the
  // difference between a page that says "this device has not got the memory,
  // here is the command" and a page that appears to have died.
  //
  // 32 MB was measured on a desktop. No phone has been measured. That is
  // exactly why this path exists rather than being left to the ceiling.
  let cnfOffset;
  let proofOffset;
  try {
    cnfOffset = exports.cnf_reserve(cnfBytes.length);
    proofOffset = exports.proof_reserve(proofBytes.length);
    if (cnfOffset === 0 || proofOffset === 0) {
      // The page refuses on size before it gets here, so this is the module's
      // own second opinion rather than the path a user travels.
      self.postMessage({ kind: 'refused' });
      return;
    }
    new Uint8Array(exports.memory.buffer, cnfOffset, cnfBytes.length).set(
      cnfBytes,
    );
    new Uint8Array(exports.memory.buffer, proofOffset, proofBytes.length).set(
      proofBytes,
    );
  } catch (error) {
    self.postMessage({
      kind: 'exhausted',
      needBytes: cnfBytes.length + proofBytes.length,
      detail: String(error),
    });
    return;
  }

  const started = performance.now();
  let code;
  try {
    code = exports.check();
  } catch (error) {
    // A panic in the checker is a trap here, because the module is built with
    // panic = "abort". So is running out of memory part-way through, and the
    // two are the same RuntimeError with the same text — nothing here can tell
    // them apart, and the panel says so rather than picking one. What it must
    // never do is report either as a verdict: a page that turned a crash into
    // "NOT VERIFIED" would accuse a proof of something the checker never
    // established.
    self.postMessage({ kind: 'internal', detail: String(error) });
    return;
  }
  const seconds = (performance.now() - started) / 1000;

  if (code === REFUSED) {
    self.postMessage({ kind: 'refused' });
    return;
  }
  const word = WORDS[code];
  if (word === undefined) {
    self.postMessage({
      kind: 'internal',
      detail: `the checker returned ${code}, which is not a verdict`,
    });
    return;
  }
  self.postMessage({
    kind: 'verdict',
    word,
    seconds,
    peakBytes: exports.memory.buffer.byteLength,
  });
};
