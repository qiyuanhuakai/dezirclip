import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import { createWriteStream, existsSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { finished } from 'node:stream/promises';
import { mainWebviewTargets } from './release-windows-upgrade.mjs';

assert.equal(process.platform, 'win32');
assert.equal(process.env.GITHUB_ACTIONS, 'true');
assert.equal(process.env.RUNNER_ENVIRONMENT, 'github-hosted');
const [binary, profile, python, source] = process.argv.slice(2);
assert(binary && profile && python && path.isAbsolute(binary) && path.isAbsolute(profile) && path.isAbsolute(python));
assert.match(source, /^[a-f0-9]{40}$/);
for (const value of [binary, profile]) {
  const relative = path.relative(path.resolve(process.env.RUNNER_TEMP), path.resolve(value));
  assert(relative && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative), 'Only this disposable receipt directory may execute');
}
await mkdir(profile, { recursive: false });
const port = await new Promise((resolve, reject) => {
  const listener = net.createServer(); listener.once('error', reject);
  listener.listen(0, '127.0.0.1', () => { const port = listener.address().port; listener.close(error => error ? reject(error) : resolve(port)); });
});
const root = path.dirname(profile);
const policyState = path.join(root, 'desktop-webview-policy.log');
const helper = fileURLToPath(new URL('./release_windows_webview.py', import.meta.url));
const execute = promisify(execFile);
const diagnostics = createWriteStream(path.join(profile, 'cdp-discovery.log'), { flags: 'wx', flush: true });
const stderrLog = createWriteStream(path.join(profile, 'desktop.stderr.log'), { flags: 'wx', flush: true });
let diagnosticBytes = 0;
let omittedDiagnostics = 0;
let diagnosticFailure;
for (const stream of [diagnostics, stderrLog]) stream.on('error', error => { diagnosticFailure ??= error; });
function observe(value, final = false) {
  let line = JSON.stringify(value) + '\n';
  if (Buffer.byteLength(line) > 2 * 1024 * 1024) {
    diagnosticFailure ??= new Error('Desktop discovery observation exceeds its retained bound');
    line = JSON.stringify({ stage: 'observation-over-bound', originalBytes: Buffer.byteLength(line) }) + '\n';
  }
  if (!final && diagnosticBytes + Buffer.byteLength(line) > 4 * 1024 * 1024) { omittedDiagnostics++; return; }
  diagnosticBytes += Buffer.byteLength(line);
  try { diagnostics.write(line); } catch (error) { diagnosticFailure ??= error; }
}
async function policy(mode, args = []) {
  try {
    const result = await execute(python, ['-B', helper, mode, '--source', source, '--root', root, '--state', policyState, ...args],
      { windowsHide: true, env: process.env, timeout: 30000, maxBuffer: 1024 * 1024 });
    observe({ stage: 'policy', mode, stdout: result.stdout.slice(0, 16384), stderr: result.stderr.slice(0, 16384) });
  } catch (error) {
    observe({ stage: 'policy', mode, error: String(error).slice(0, 4096),
      stdout: String(error.stdout ?? '').slice(0, 16384), stderr: String(error.stderr ?? '').slice(0, 16384) });
    throw error;
  }
}
let child;
let childClosed = false;
let socket;
let sequence = 0;
let stderrBytes = 0;
const pending = new Map();
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function call(method, params) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('Desktop metadata CDP deadline')); }, 10000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
let failure;
try {
  await Promise.all([once(diagnostics, 'open'), once(stderrLog, 'open')]);
  await policy('configure', ['--profile', profile, '--port', String(port)]);
  child = spawn(binary, [], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'], env: {
    ...process.env, WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  } });
  child.once('close', () => { childClosed = true; });
  child.stderr.on('data', data => {
    const allowed = Math.max(0, 1024 * 1024 - stderrBytes);
    stderrBytes += data.length; stderrLog.write(data.subarray(0, allowed));
    if (stderrBytes > 1024 * 1024) child.kill();
  });
  await new Promise((resolve, reject) => { child.once('spawn', resolve); child.once('error', reject); });
  observe({ stage: 'spawned', pid: child.pid, binary, port, profile });
  await policy('inspect', ['--pid', String(child.pid), '--executable', binary]);
  let pages;
  let attempt = 0;
  let lastDiscovery;
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    if (diagnosticFailure) throw diagnosticFailure;
    attempt++;
    assert.equal(child.exitCode, null, 'Actual packaged Desktop exited');
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) });
      const text = await response.text(); assert(text.length <= 1024 * 1024);
      lastDiscovery = { stage: 'discovery', attempt, status: response.status,
        contentType: response.headers.get('content-type'), bodyPrefix: text.slice(0, 2048), bodyCharacters: text.length };
      observe(lastDiscovery);
      assert(response.ok);
      const targets = JSON.parse(text);
      pages = mainWebviewTargets(targets);
      lastDiscovery = { stage: 'targets', attempt, targets: targets.map(target => ({
        type: target?.type, id: String(target?.id ?? '').slice(0, 512), url: String(target?.url ?? '').slice(0, 512),
        title: String(target?.title ?? '').slice(0, 256), webSocketDebuggerUrl: String(target?.webSocketDebuggerUrl ?? '').slice(0, 512),
      })), mainCount: pages.length };
      observe(lastDiscovery);
      if (pages.length === 1) break;
    } catch (error) {
      lastDiscovery = { stage: 'discovery-error', attempt, error: String(error).slice(0, 1024),
        cause: String(error.cause ?? '').slice(0, 1024), code: error.cause?.code };
      observe(lastDiscovery);
    }
    await delay(100);
  }
  observe({ stage: 'discovery-finished', attempts: attempt, last: lastDiscovery, mainCount: pages?.length }, true);
  assert.equal(pages?.length, 1, 'Expected one private main Desktop WebView');
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
} catch (error) {
  failure = error;
} finally {
  try {
    socket?.close();
    for (const item of pending.values()) item.reject(new Error('Desktop metadata closed'));
    if (child) {
      child.kill();
      const deadline = Date.now() + 5000;
      while (!childClosed && Date.now() < deadline) await delay(50);
      observe({ stage: 'child-closed', childClosed, exitCode: child.exitCode, signalCode: child.signalCode });
      assert(childClosed, 'Owned Desktop process did not close');
    }
    assert(stderrBytes <= 1024 * 1024, 'Actual Desktop stderr exceeded its retained bound');
  } catch (error) {
    observe({ stage: 'cleanup-error', error: String(error).slice(0, 4096) }); failure ??= error;
  }
  try { if (existsSync(policyState)) await policy('restore'); }
  catch (error) { failure ??= error; }
  failure ??= diagnosticFailure;
  observe({ stage: 'finished', status: failure ? 'failed' : 'completed', omittedDiagnostics,
    error: failure ? String(failure).slice(0, 4096) : undefined }, true);
  for (const stream of [stderrLog, diagnostics]) {
    try { const completion = finished(stream); stream.end(); await completion; }
    catch (error) { failure ??= error; }
  }
}
if (failure) throw failure;
