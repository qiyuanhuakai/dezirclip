import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import { createWriteStream } from 'node:fs';
import net from 'node:net';
import path from 'node:path';

assert.equal(process.platform, 'win32');
assert.equal(process.env.GITHUB_ACTIONS, 'true');
assert.equal(process.env.RUNNER_ENVIRONMENT, 'github-hosted');
const [binary, profile] = process.argv.slice(2);
assert(binary && profile && path.isAbsolute(binary) && path.isAbsolute(profile));
for (const value of [binary, profile]) {
  const relative = path.relative(path.resolve(process.env.RUNNER_TEMP), path.resolve(value));
  assert(relative && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative), 'Only this disposable receipt directory may execute');
}
await mkdir(profile, { recursive: false });
const port = await new Promise((resolve, reject) => {
  const listener = net.createServer(); listener.once('error', reject);
  listener.listen(0, '127.0.0.1', () => { const port = listener.address().port; listener.close(error => error ? reject(error) : resolve(port)); });
});
const child = spawn(binary, [], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'], env: {
  ...process.env, WEBVIEW2_USER_DATA_FOLDER: profile,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
} });
const stderrLog = createWriteStream(path.join(profile, 'desktop.stderr.log'), { flags: 'wx' });
let childClosed = false;
child.once('close', () => { childClosed = true; });
let socket;
let sequence = 0;
let stderrBytes = 0;
const pending = new Map();
child.stderr.on('data', data => {
  const allowed = Math.max(0, 1024 * 1024 - stderrBytes);
  stderrBytes += data.length; stderrLog.write(data.subarray(0, allowed));
  if (stderrBytes > 1024 * 1024) child.kill();
});
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function call(method, params) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Desktop metadata CDP deadline')); }, 10000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
try {
  let pages;
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    assert.equal(child.exitCode, null, 'Actual packaged Desktop exited');
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) });
      assert(response.ok);
      const text = await response.text(); assert(text.length <= 1024 * 1024);
      pages = JSON.parse(text).filter(page => page.type === 'page' && typeof page.webSocketDebuggerUrl === 'string');
      if (pages.length === 1) break;
    } catch { /* Wait for this private WebView endpoint. */ }
    await delay(100);
  }
  assert.equal(pages?.length, 1, 'Expected one private Desktop WebView');
  const url = new URL(pages[0].webSocketDebuggerUrl);
  assert.equal(url.hostname, '127.0.0.1'); assert.equal(url.port, String(port));
  socket = new WebSocket(url);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Desktop metadata socket deadline')), 5000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Desktop metadata socket failed')); }, { once: true });
  });
  socket.addEventListener('message', event => {
    assert(String(event.data).length <= 1024 * 1024);
    const message = JSON.parse(String(event.data));
    if (!pending.has(message.id)) return;
    const request = pending.get(message.id); pending.delete(message.id);
    if (message.error) request.reject(new Error('Actual Desktop CDP command failed')); else request.resolve(message.result);
  });
  let result;
  while (Date.now() < deadline) {
    result = await call('Runtime.evaluate', { expression: '(async()=>typeof window.__TAURI_INTERNALS__?.invoke==="function"?await window.__TAURI_INTERNALS__.invoke("get_build_info"):null)()', awaitPromise: true, returnByValue: true });
    assert(!result.exceptionDetails, 'Actual Desktop get_build_info failed');
    if (result.result?.value) break;
    await delay(100);
  }
  assert.equal(result?.result?.value?.component, 'desktop', 'Actual Desktop metadata missing');
  process.stdout.write(JSON.stringify(result.result.value) + '\n');
} finally {
  socket?.close();
  for (const item of pending.values()) item.reject(new Error('Desktop metadata closed'));
  child.kill();
  const deadline = Date.now() + 5000;
  while (!childClosed && Date.now() < deadline) await delay(50);
  await new Promise(resolve => stderrLog.end(resolve));
  assert(childClosed, 'Owned Desktop process did not close');
  assert(stderrBytes <= 1024 * 1024, 'Actual Desktop stderr exceeded its retained bound');
}
