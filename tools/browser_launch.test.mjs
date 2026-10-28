import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseDevToolsActivePort, shouldRetryLaunch } from './browser_launch.mjs';

test('reads the port from the first line, with LF or CRLF', () => {
  assert.equal(parseDevToolsActivePort('41235\n/devtools/browser/example\n'), 41235);
  assert.equal(parseDevToolsActivePort('52346\r\n/devtools/browser/example\r\n'), 52346);
  assert.equal(parseDevToolsActivePort('1'), 1);
  assert.equal(parseDevToolsActivePort('65535\n'), 65535);
});

test('rejects missing, malformed and out-of-range ports', () => {
  for (const contents of [
    '', '\n41235', '0\n', '-1\n', '65536\n', '999999999999999999999\n',
    '41235junk\n', '4.5\n', '4e3\n', ' 41235\n', '41235 \n',
  ]) {
    assert.equal(parseDevToolsActivePort(contents), null, JSON.stringify(contents));
  }
});

test('retries early exits, timeouts and spawn errors only before attempt three', () => {
  for (const outcome of ['exit', 'timeout', 'error']) {
    assert.equal(shouldRetryLaunch(1, outcome), true, outcome);
    assert.equal(shouldRetryLaunch(2, outcome), true, outcome);
    assert.equal(shouldRetryLaunch(3, outcome), false, outcome);
    assert.equal(shouldRetryLaunch(4, outcome), false, outcome);
  }
});

test('does not retry a successful launch', () => {
  for (const attempt of [1, 2, 3]) {
    assert.equal(shouldRetryLaunch(attempt, 'ready'), false);
  }
});
