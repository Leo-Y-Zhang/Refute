export const MAX_LAUNCH_ATTEMPTS = 3;

/** The first line of DevToolsActivePort is a TCP port, or not yet usable. */
export function parseDevToolsActivePort(contents) {
  const firstLine = contents.split(/\r?\n/, 1)[0];
  if (!/^\d+$/.test(firstLine)) {
    return null;
  }
  const port = Number(firstLine);
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

/** Attempts are numbered from one; only launch failures get another try. */
export function shouldRetryLaunch(attempt, outcome) {
  return attempt < MAX_LAUNCH_ATTEMPTS &&
    (outcome === 'exit' || outcome === 'timeout' || outcome === 'error');
}
