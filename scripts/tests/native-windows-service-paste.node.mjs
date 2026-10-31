import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { Bridge } from './native-windows-surface.mjs';
import { within } from './native-windows-lifecycle.mjs';
import { assertNoFixtureInput, pollRead, startupBudget, exactHostUnavailable, qualifyServiceHost, clipboardProducerProof } from './native-windows-service-paste.mjs';

const snapshot = () => ({ target: { hwnd: '11', root_hwnd: '11', pid: 17, thread_id: 19, creation_time_100ns: '23' },
  observation_epoch: 2, observation_label: 'stale_original_target_refused',
  alternate: { hwnd: 29, pid: 17, thread_id: 19, root_hwnd: 29 }, alternate_edit_hwnd: 31,
  target_text: '', alternate_text: '', callback_errors: [], focus: { foreground_hwnd: 29, messages: [
    { role: 'alternate-edit', message: 7, observed_monotonic_ns: '31' }] } });
test('negative-case baseline is after activation and permits only later non-input observations', () => {
  const before = snapshot(), after = snapshot();
  after.focus.messages.push({ role: 'alternate', message: 6, observed_monotonic_ns: '37' });
  assert.equal(assertNoFixtureInput(before, after).fixture_input_messages, 0);
});
test('text, identity, callback, foreground and message loss cannot masquerade as refusal', () => {
  for (const change of [
    value => { value.target_text = 'unexpected'; }, value => { value.alternate_text = 'unexpected'; },
    value => { value.target.hwnd = '13'; }, value => { value.callback_errors.push('native callback error'); },
    value => { value.alternate.pid = 99; }, value => { value.alternate_edit_hwnd = 99; },
    value => { value.observation_epoch = 3; }, value => { value.observation_label = 'another-case'; },
    value => { value.focus.foreground_hwnd = 11; }, value => { value.focus.messages = []; },
    value => { value.focus.messages[0].observed_monotonic_ns = '33'; },
    value => { value.focus.messages = Array.from({ length: 64 }, () => value.focus.messages[0]); },
  ]) {
    const after = snapshot(); change(after); assert.throws(() => assertNoFixtureInput(snapshot(), after));
  }
});
test('a temporary observed activation/focus of A is refused even if final foreground is B', () => {
  for (const message of [{ role: 'target', message: 6, wparam: 1 }, { role: 'target-edit', message: 7, wparam: 0 }]) {
    const after = snapshot(); after.focus.messages.push(message); assert.throws(() => assertNoFixtureInput(snapshot(), after));
  }
});
test('a missing diagnostic epoch cannot qualify even if both snapshots omit it', () => {
  const before = snapshot(), after = snapshot(); delete before.observation_epoch; delete after.observation_epoch;
  assert.throws(() => assertNoFixtureInput(before, after));
});
test('every subscribed keyboard/paste input is rejected even with unchanged empty text', () => {
  for (const message of [0x100, 0x101, 0x104, 0x105, 0x302]) {
    const after = snapshot(); after.focus.messages.push({ message, observed_monotonic_ns: '37' });
    assert.throws(() => assertNoFixtureInput(snapshot(), after));
  }
});
test('read completion at or beyond the original deadline cannot qualify a late target', async () => {
  for (const completeAt of [12000, 15000]) {
    let clock = 0, calls = 0;
    await assert.rejects(pollRead('fixture', async () => { calls++; clock = completeAt; return true; },
      12000, () => clock, async () => { throw new Error('must stop before another observation'); }), /timed out/);
    assert.equal(calls, 1);
  }
});
test('each observation and sleep receives only the remaining budget; no input is retried', async () => {
  let clock = 0, calls = 0; const budgets = [], sleeps = [];
  const bound = async (promise, remaining) => { budgets.push(remaining); return promise; };
  const result = await pollRead('fixture', async remaining => { calls++; assert.equal(remaining, 12000 - clock); clock += 1; return calls === 2 ? 'ready' : null; },
    12000, () => clock, async wait => { sleeps.push(wait); clock += wait; }, bound);
  assert.equal(result, 'ready'); assert.equal(calls, 2); assert.deepEqual(budgets, [12000, 11969]); assert.deepEqual(sleeps, [30]);
});
const expectedHost = { service_instance_id: 'original-service', privacy_epoch: 0 };
const readyHost = () => ({ mode: 'service', capture_owner: false, state: { ...expectedHost } });
const unavailable = value => new Error(JSON.stringify({ exception: { type: 'string', value } }));
test('only the exact structured temporary Host error may precede real Service qualification', async () => {
  let clock = 0, calls = 0; const observations = [];
  const result = await qualifyServiceHost(async () => { calls++; clock += 2; if (calls === 1) throw unavailable('ClipboardHostUnavailable'); return readyHost(); },
    expectedHost, 45000, observations, () => clock, async ms => { clock += ms; });
  assert.equal(result.mode, 'service'); assert.equal(calls, 2); assert.equal(observations.length, 2);
  assert.match(observations[0].error, /ClipboardHostUnavailable/); assert.deepEqual(observations[1].response, readyHost());
  assert(!exactHostUnavailable(new Error('ClipboardHostUnavailable')));
  assert(!exactHostUnavailable(unavailable('ClipboardHostPermissionDenied')));
});
test('wrong mode, capture, instance, epoch and non-temporary errors fail without another read', async () => {
  for (const change of [value => { value.mode = 'embedded'; }, value => { value.capture_owner = true; },
    value => { value.state.service_instance_id = 'replacement'; }, value => { value.state.privacy_epoch = 1; }]) {
    let calls = 0; const value = readyHost(); change(value);
    await assert.rejects(qualifyServiceHost(async () => { calls++; return value; }, expectedHost, 45000, [], () => 0, async () => {}));
    assert.equal(calls, 1);
  }
  for (const error of [new Error('transport failed'), unavailable('ClipboardHostPermissionDenied'), new Error('ClipboardHostUnavailable')]) {
    let calls = 0;
    await assert.rejects(qualifyServiceHost(async () => { calls++; throw error; }, expectedHost, 45000, [], () => 0, async () => {}));
    assert.equal(calls, 1);
  }
});
test('persistent unavailability and late success cannot extend the shared startup deadline', async () => {
  let clock = 0, calls = 0; const observations = [];
  await assert.rejects(qualifyServiceHost(async () => { calls++; clock += 5; throw unavailable('ClipboardHostUnavailable'); },
    expectedHost, 50, observations, () => clock, async ms => { clock += ms; }), /timed out|deadline/);
  assert.equal(calls, 2); assert.equal(observations.length, 2);
  clock = 44990; calls = 0;
  await assert.rejects(qualifyServiceHost(async budget => { calls++; assert.equal(budget, 10); clock += 11; return readyHost(); },
    expectedHost, 45000, [], () => clock, async () => {}), /deadline/);
  assert.equal(calls, 1);
  clock = 45000; calls = 0;
  await assert.rejects(qualifyServiceHost(async () => { calls++; return readyHost(); }, expectedHost, 45000, [], () => clock), /deadline/);
  assert.equal(calls, 0); assert.equal(startupBudget(45000, 20000, () => 44990), 10);
});
test('a discovery or RPC crossing the shared deadline cannot create later sockets or commands', async () => {
  const source = readFileSync(new URL('./native-windows-service-paste.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('class ServiceStartupBridge extends Bridge {'), end = source.indexOf('\nexport async function runServicePaste', start);
  assert(start >= 0 && end > start);
  for (const phase of ['discovery', 'open-budget', 'RPC']) {
    let clock = 0, sockets = 0, sends = 0, openListeners = 0;
    class Socket {
      constructor() { sockets++; this.readyState = 0; this.handlers = new Map(); if (phase === 'open-budget') clock = 11; queueMicrotask(() => { this.readyState = 1; this.emit('open'); }); }
      addEventListener(name, callback) { if (name === 'open') openListeners++; const list = this.handlers.get(name) ?? []; list.push(callback); this.handlers.set(name, list); }
      emit(name, event = {}) { for (const callback of this.handlers.get(name) ?? []) callback(event); }
      send(raw) { sends++; const request = JSON.parse(raw); queueMicrotask(() => { clock = 11; this.emit('message', { data: JSON.stringify({ id: request.id, result: { result: { value: { ready: true } } } }) }); }); }
      close() { this.readyState = 3; this.emit('error'); this.emit('close'); }
    }
    const scope = { Bridge, assert, URL, within, performance: { now: () => clock }, WebSocket: Socket,
      AbortSignal: { timeout: milliseconds => ({ milliseconds }) }, mainWebviewTargets: targets => targets,
      mainBridgeReady: value => value.ready,
      startupBudget: (deadline, maximum) => startupBudget(deadline, maximum, () => clock),
      pollRead: (label, read, duration) => pollRead(label, read, duration, () => clock, async ms => { clock += ms; }),
      fetch: async () => { if (phase === 'discovery') clock = 11; return { status: 200, json: async () => [{ webSocketDebuggerUrl: 'ws://127.0.0.1:9222/devtools/page/fixture' }] }; } };
    vm.runInNewContext(source.slice(start, end) + '\nglobalThis.Subject = ServiceStartupBridge;', scope);
    const bridge = new scope.Subject();
    await assert.rejects(bridge.connect(9222, [], 10), /deadline/);
    const finalized = await bridge.finalize(); assert.equal(finalized.close_confirmed, true);
    assert.equal(sockets, phase === 'discovery' ? 0 : 1); assert.equal(sends, phase === 'RPC' ? 1 : 0);
    assert.equal(openListeners, phase === 'RPC' ? 1 : 0, 'expired open budget must not create an unhandled opening promise');
    assert(!bridge.pending || bridge.pending.size === 0); clock = 100;
    await Promise.resolve(); assert.equal(sends, phase === 'RPC' ? 1 : 0);
  }
});
test('a spawn crossing startup deadline preserves its caller role before the postcheck', () => {
  const source = readFileSync(new URL('./native-windows-service-paste.mjs', import.meta.url), 'utf8');
  const line = source.split('\n').find(value => value.trimStart().startsWith('const managed = (...values) =>'));
  assert(line); assert.match(source, /native = managed\([^\n]+\);\n    startupBudget\(startupDeadline\);/);
  let clock = 0;
  const scope = { startupDeadline: 10, startupBudget: deadline => startupBudget(deadline, Infinity, () => clock),
    Managed: class { constructor() { clock = 11; this.child = { pid: 17 }; } }, all: [] };
  vm.runInNewContext(line + '\nlet native; try { native = managed(); startupBudget(startupDeadline); } catch(error) { globalThis.failure = error; } globalThis.original = native;', scope);
  assert(scope.failure); assert(scope.original); assert.equal(scope.original.child.pid, 17); assert.equal(scope.all[0], scope.original);
  assert.match(source, /const portBudget = startupBudget\(startupDeadline\);\n    const cdpPort = await within\(port\(\), portBudget/);
  assert(source.indexOf('report.clipboard_producer_early_cleanup =') < source.indexOf('await stopManagedChild(native'));
});
test('clipboard producer proof requires the same original object exit and stdio close', () => {
  const original = () => ({ child: { pid: 17 }, record: { pid: 17, closed_at: 'closed',
    process_exit: { observed: true, exit_code: 0 }, stdio_close: { observed: true } } });
  assert.equal(clipboardProducerProof(original()).pid, 17);
  for (const change of [value => { value.record.pid = 19; }, value => { value.record.process_exit.observed = false; },
    value => { value.record.stdio_close.observed = false; }, value => { value.record.closed_at = null; }]) {
    const value = original(); change(value); assert.throws(() => clipboardProducerProof(value));
  }
});
