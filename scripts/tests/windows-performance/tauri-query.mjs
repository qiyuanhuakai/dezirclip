import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';

const MAX_BYTES = 256 * 1024;
export const DOM_QUERY_EXPRESSION = `(() => {
  const visible = element => {
    if (!(element instanceof HTMLElement)) return false;
    const box = element.getBoundingClientRect();
    let left = Math.max(0, box.left), right = Math.min(innerWidth, box.right);
    let top = Math.max(0, box.top), bottom = Math.min(innerHeight, box.bottom);
    for (let node = element; node; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (node.hidden || style.display === 'none' || ['hidden', 'collapse'].includes(style.visibility)
          || Number(style.opacity) === 0) return false;
      if (node !== element) {
        const parent = node.getBoundingClientRect();
        if (/(auto|scroll|hidden|clip)/.test(style.overflowX)) { left = Math.max(left, parent.left); right = Math.min(right, parent.right); }
        if (/(auto|scroll|hidden|clip)/.test(style.overflowY)) { top = Math.max(top, parent.top); bottom = Math.min(bottom, parent.bottom); }
      }
    }
    return right > left && bottom > top;
  };
  const inputs = [...document.querySelectorAll('[data-testid="quick-paste-search"]')];
  const rows = [...document.querySelectorAll('[data-testid="quick-paste-item"]')];
  return { href: location.href, top_level: top === window, ready_state: document.readyState,
    document_visible: document.visibilityState === 'visible', input_count: inputs.length,
    input: inputs.length === 1 ? { value: String(inputs[0].value).slice(0, 513), visible: visible(inputs[0]), disabled: inputs[0].disabled } : null,
    row_count: rows.length, rows: rows.slice(0, 101).map(row => ({ visible: visible(row),
      content: (row.querySelector('.quick-paste-window__item-preview')?.textContent ?? '').slice(0, 4097) })) };
})()`;

export function quickPasteUrl(value) {
  try {
    const url = new URL(value);
    return !url.username && !url.password && !url.port && !url.hash
      && ((['http:', 'https:'].includes(url.protocol) && url.hostname === 'tauri.localhost')
        || (url.protocol === 'tauri:' && url.hostname === 'localhost'))
      && ['/', '/index.html'].includes(url.pathname) && url.searchParams.getAll('window').length === 1
      && url.searchParams.get('window') === 'quick-paste';
  } catch { return false; }
}

export function selectQuickPasteTarget(targets, port) {
  assert(Number.isSafeInteger(port) && port >= 1024 && port <= 65535);
  assert(Array.isArray(targets) && targets.length <= 256);
  const matches = targets.filter(item => item?.type === 'page' && quickPasteUrl(item.url));
  assert.equal(matches.length, 1, 'One actual Quick Paste WebView is required');
  const target = matches[0], socket = new URL(target.webSocketDebuggerUrl);
  assert.equal(socket.protocol, 'ws:'); assert.equal(socket.hostname, '127.0.0.1');
  assert.equal(Number(socket.port), port); assert(!socket.username && !socket.password && !socket.hash && !socket.search);
  assert.match(socket.pathname, /^\/devtools\/page\/[a-zA-Z0-9_-]+$/);
  assert.equal(typeof target.id, 'string'); assert(target.id.length > 0 && target.id.length <= 256);
  assert.equal(socket.pathname, '/devtools/page/' + target.id);
  return { id: target.id, url: target.url, webSocketDebuggerUrl: socket.href };
}

export function domQueryObservation(snapshot, panel, query, rows, selected = null) {
  const context = snapshot?.top_level === true && quickPasteUrl(snapshot.href)
    && ['interactive', 'complete'].includes(snapshot.ready_state) && snapshot.document_visible === true;
  const input = snapshot?.input;
  const inputMatches = snapshot?.input_count === 1 && input?.visible === true && input.disabled === false
    && input.value === query;
  const bounded = Number.isSafeInteger(snapshot?.row_count) && snapshot.row_count >= 0 && snapshot.row_count <= 100
    && Array.isArray(snapshot.rows) && snapshot.rows.length === snapshot.row_count
    && snapshot.rows.every(row => typeof row.visible === 'boolean' && typeof row.content === 'string' && row.content.length <= 4096);
  const bodies = bounded ? rows.filter(row => snapshot.rows.some(item => item.visible && item.content === row.content)).map(row => row.content) : [];
  const visibleCards = bounded ? snapshot.rows.filter(item => item.visible) : [];
  const knownCards = visibleCards.every(item => rows.some(row => item.content === row.content));
  const resultsMatch = knownCards && (selected ? visibleCards.length === 1 && bodies.length === 1 && bodies[0] === selected.content : bodies.length >= 2);
  return { status: context && inputMatches && bounded && resultsMatch ? 'verified' : 'unverified',
    query, observer: 'owned_tauri_cdp_dom_v4', panel, input_value_verified: inputMatches,
    top_level_context_verified: context, visible_fixture_rows: bodies.length, visible_fixture_contents: bodies,
    stored_rows: rows.length, expected_matching_rows: selected ? 1 : null,
    scope: 'Viewport-intersecting card DOM text; not full text pixels, a painted frame, or search-ready latency.' };
}

export async function readTauriQuery(port, panel, milliseconds, env = process.env) {
  assert.equal(process.platform, 'win32'); assert.equal(env.GITHUB_ACTIONS, 'true');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted'); assert.equal(env.GITHUB_REPOSITORY, 'qiyuanhuakai/dezirclip');
  assert.equal(env.GITHUB_EVENT_NAME, 'workflow_dispatch'); assert.equal(env.DZC_WINDOWS_PERFORMANCE_ENABLED, 'true');
  assert(Number.isSafeInteger(port) && port >= 1024 && port <= 65535);
  assert(Number.isSafeInteger(panel.pid) && panel.pid > 0 && Number.isSafeInteger(panel.hwnd) && panel.hwnd > 0);
  assert(Number.isSafeInteger(milliseconds) && milliseconds > 0 && milliseconds <= 8000);
  const deadline = performance.now() + milliseconds;
  const budget = () => { const left = Math.floor(deadline - performance.now()); assert(left > 0, 'Original DOM observation deadline expired'); return left; };
  const abort = new AbortController(), timer = setTimeout(() => abort.abort(), budget());
  let socket, closed, result, failure;
  try {
    const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: abort.signal, redirect: 'error' });
    assert.equal(response.status, 200);
    const reader = response.body.getReader(), chunks = []; let size = 0;
    try {
      while (true) {
        const { done, value } = await reader.read(); if (done) break;
        size += value.byteLength; assert(size <= MAX_BYTES, 'Bounded CDP discovery exceeded'); chunks.push(value); budget();
      }
    } finally { await reader.cancel(); }
    const target = selectQuickPasteTarget(JSON.parse(Buffer.concat(chunks).toString('utf8')), port); budget();
    socket = new WebSocket(target.webSocketDebuggerUrl);
    closed = new Promise(resolve => socket.addEventListener('close', resolve, { once: true }));
    const snapshot = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Original DOM observation deadline expired')), budget());
      let messages = 0, bytes = 0;
      const fail = error => { clearTimeout(timeout); reject(error); };
      socket.addEventListener('open', () => {
        try { budget(); socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate',
          params: { expression: DOM_QUERY_EXPRESSION, returnByValue: true, awaitPromise: false } })); }
        catch (error) { fail(error); }
      }, { once: true });
      socket.addEventListener('message', event => {
        try {
          budget(); assert(++messages <= 32 && typeof event.data === 'string');
          bytes += Buffer.byteLength(event.data); assert(bytes <= MAX_BYTES, 'Bounded CDP message total exceeded');
          const value = JSON.parse(event.data); if (value.id !== 1) return;
          assert(!value.error && !value.result?.exceptionDetails && value.result?.result?.type === 'object');
          clearTimeout(timeout); resolve(value.result.result.value);
        } catch (error) { fail(error); }
      });
      socket.addEventListener('error', () => fail(new Error('Owned Quick Paste CDP connection failed')), { once: true });
      socket.addEventListener('close', () => fail(new Error('Owned Quick Paste CDP connection closed')), { once: true });
    });
    budget(); assert(quickPasteUrl(snapshot?.href) && snapshot.href === target.url && snapshot.top_level === true);
    result = { observer: 'owned_tauri_cdp_dom_v4', panel, target, snapshot,
      expression_sha256: createHash('sha256').update(DOM_QUERY_EXPRESSION).digest('hex') };
  } catch (error) { failure = error; }
  finally {
    clearTimeout(timer); abort.abort();
    if (socket) {
      try {
        socket.close();
        await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => reject(new Error('Owned DOM observer close was not confirmed within its original deadline')), budget());
          closed.then(() => { clearTimeout(timeout); resolve(); });
        });
        if (result) result.socket_closed = true;
      } catch (error) { failure = failure ? new AggregateError([failure, error], 'DOM observation and close failed') : error; }
    }
  }
  if (failure) throw failure;
  return result;
}
