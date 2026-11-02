import assert from 'node:assert/strict';
import test from 'node:test';
import { EventEmitter } from 'node:events';
import { DeadlineError, trackManagedChild, within, finalizeManagedChild, stopManagedChild, exitFailedController } from './native-windows-lifecycle.mjs';
import { Bridge, completedCommand } from './native-windows-surface.mjs';

class Timers {
  now = 0; sequence = 0; pending = new Map();
  setTimeout = (callback, milliseconds) => {
    const id = ++this.sequence;
    this.pending.set(id, { callback, due: this.now + milliseconds }); return id;
  };
  clearTimeout = id => this.pending.delete(id);
  next() {
    const [id, entry] = [...this.pending].sort((a, b) => a[1].due - b[1].due)[0] ?? [];
    if (!entry) return false;
    this.now = entry.due; this.pending.delete(id); entry.callback(); return true;
  }
}

async function settle(operation, timers) {
  let done = false, value, error;
  operation.then(result => { value = result; done = true; }, caught => { error = caught; done = true; });
  for (let index = 0; index < 80 && !done; index++) {
    for (let turn = 0; turn < 12; turn++) await Promise.resolve();
    if (!done) timers.next();
  }
  assert(done, 'The fake operation must settle within its bounded timers');
  return { value, error };
}

function managed(timers, { kill = true, closeAfterKill = false, exited = false } = {}) {
  const calls = [], record = {};
  if (exited) record.process_exit = { observed: true, exit_code: 7, signal: null, observed_at: 'original-exit' };
  let resolveClose;
  const value = { record, closed: new Promise(resolve => { resolveClose = resolve; }),
    finish() { calls.push('finish'); }, cancelPending() { calls.push('cancel'); },
    child: { kill() {
      calls.push('kill');
      if (kill instanceof Error) throw kill;
      if (closeAfterKill) timers.setTimeout(() => {
        record.process_exit = { observed: true, exit_code: 1, signal: null, observed_at: 'original-exit' };
        record.closed_at = 'original-close'; resolveClose({ exit_code: 1, signal: null });
      }, 100);
      return kill;
    }, unref() { calls.push('unref'); } } };
  for (const stream of ['stdin', 'stdout', 'stderr']) value.child[stream] = {
    end() { calls.push('end-' + stream); }, destroy() { calls.push('destroy-' + stream); },
  };
  value.close = code => {
    record.closed_at = 'original-close'; resolveClose({ exit_code: code, signal: null });
  };
  return { value, calls };
}

test('within cancels its timer after success or the original rejection', async () => {
  const timers = new Timers(), original = new Error('original');
  assert.equal(await within(Promise.resolve(4), 100, 'deadline', timers), 4);
  await assert.rejects(within(Promise.reject(original), 100, 'deadline', timers), error => error === original);
  assert.equal(timers.pending.size, 0);
});

test('original exit and later stdio close remain separate receipts', async () => {
  const child = new EventEmitter(), record = {};
  let timestamp = 0, sealed = false;
  const closed = trackManagedChild(child, record, () => { sealed = true; }, () => ++timestamp);
  child.emit('exit', 0xc0000374, null);
  assert.equal(record.process_exit.exit_code, 0xc0000374);
  assert.equal(record.stdio_close.observed, false); assert.equal(record.closed_at, undefined);
  assert.equal(sealed, false);
  child.emit('close', 0xc0000374, null);
  assert.deepEqual(await closed, { exit_code: 0xc0000374, signal: null });
  assert.equal(record.process_exit.observed_at, 1);
  assert.equal(record.stdio_close.observed_at, 2); assert.equal(sealed, true);
});

test('a failed original stdin rejects requests and records failure without fabricating exit', () => {
  const child = new EventEmitter(), record = {}, errors = [];
  child.stdin = new EventEmitter();
  trackManagedChild(child, record, undefined, undefined, error => errors.push(error));
  const original = new Error('EPIPE');
  child.stdin.emit('error', original);
  assert.deepEqual(record.stream_errors, [{ stream: 'stdin', error: 'Error: EPIPE' }]);
  assert.equal(errors[0], original); assert.equal(record.process_exit.observed, false);
  assert.equal(record.stdio_close.observed, false); assert.equal(record.closed_at, undefined);
});

test('metadata and clipboard deadlines remain bounded even when kill never closes', async () => {
  for (const milliseconds of [10000, 15000]) {
    const timers = new Timers(), item = managed(timers);
    const result = await settle(completedCommand(item.value, milliseconds, timers), timers);
    assert(result.error instanceof DeadlineError);
    assert.equal(timers.now, milliseconds + 2000);
    assert.deepEqual(item.calls, ['kill', 'cancel', 'destroy-stdin', 'destroy-stdout', 'destroy-stderr', 'unref', 'finish']);
    assert.equal(item.value.record.teardown.kill_accepted, true);
    assert.equal(item.value.record.teardown.termination_confirmed, false);
    assert.equal(item.value.record.closed_at, undefined);
  }
});

test('failed kill requests cannot extend the close grace or invent an exit', async () => {
  for (const kill of [false, new Error('kill denied')]) {
    const timers = new Timers(), item = managed(timers, { kill });
    const result = await settle(finalizeManagedChild(item.value, 5000, timers), timers);
    assert.ifError(result.error); assert.equal(timers.now, 7000);
    assert.equal(result.value.kill_accepted, false);
    assert.equal(result.value.close_confirmed, false);
    assert.equal(item.value.record.process_exit, undefined);
  }
});

test('observed original exit with inherited open pipes is never signalled again', async () => {
  const timers = new Timers(), item = managed(timers, { exited: true });
  const result = await settle(finalizeManagedChild(item.value, 5000, timers), timers);
  assert.ifError(result.error); assert(!item.calls.includes('kill'));
  assert.equal(result.value.termination_confirmed, true);
  assert.equal(result.value.close_confirmed, false);
  assert.equal(item.value.record.process_exit.exit_code, 7);
  assert.equal(item.value.record.closed_at, undefined);
});

test('fallback close acknowledgement records real termination and avoids detaching pipes', async () => {
  const timers = new Timers(), item = managed(timers, { closeAfterKill: true });
  const result = await settle(finalizeManagedChild(item.value, 5000, timers), timers);
  assert.ifError(result.error); assert.equal(timers.now, 5100);
  assert.equal(result.value.close_confirmed, true);
  assert.equal(result.value.close_deadline_exceeded, true);
  assert.equal(result.value.stdio_detached, false);
  assert.deepEqual(item.calls, ['kill']);
  assert.equal(timers.pending.size, 0);
});

test('already closed successful commands neither terminate nor detach anything', async () => {
  const timers = new Timers(), item = managed(timers);
  item.value.close(0);
  const result = await settle(completedCommand(item.value, 10000, timers), timers);
  assert.ifError(result.error); assert.equal(result.value.exit_code, 0);
  assert.deepEqual(item.calls, ['finish']); assert.equal(timers.pending.size, 0);
});

test('natural close after the original deadline stays a deadline failure without signalling an exited PID', async () => {
  const timers = new Timers(), item = managed(timers, { exited: true });
  timers.setTimeout(() => item.value.close(7), 6000);
  const result = await settle(finalizeManagedChild(item.value, 5000, timers), timers);
  assert.ifError(result.error); assert.equal(result.value.close_confirmed, true);
  assert.equal(result.value.close_deadline_exceeded, true); assert(!item.calls.includes('kill'));
  assert.equal(item.value.record.process_exit.exit_code, 7);
});

test('original nonzero command failure survives a secondary log sealing failure', async () => {
  const timers = new Timers(), item = managed(timers);
  item.value.close(9); item.value.finish = () => { throw new Error('secondary write failure'); };
  const result = await settle(completedCommand(item.value, 10000, timers), timers);
  assert.equal(result.error.code, 'ERR_ASSERTION');
  assert.equal(item.value.record.log_finalization_error, 'Error: secondary write failure');
  assert(!item.calls.includes('kill'));
});

test('native stop rejection still reaches its independent bounded close finalization', async () => {
  const timers = new Timers(), item = managed(timers);
  const result = await settle(stopManagedChild(item.value, () => Promise.reject(new Error('original stop failed')), 15000, timers), timers);
  assert.ifError(result.error); assert.equal(result.value.stop_error, 'Error: original stop failed');
  assert.equal(timers.now, 22000); assert(item.calls.includes('end-stdin'));
  assert.equal(result.value.termination_confirmed, false); assert.equal(result.value.stdio_detached, true);
});

test('never answered native stop has a deadline independent of its pending requests', async () => {
  const timers = new Timers(), item = managed(timers);
  const result = await settle(stopManagedChild(item.value, () => new Promise(() => {}), 15000, timers), timers);
  assert.ifError(result.error); assert.match(result.value.stop_error, /stop request timed out/);
  assert.equal(timers.now, 37000); assert(item.calls.includes('cancel'));
});

test('unconfirmed CDP close stops waiting and rejects remaining requests without a false success', async () => {
  const timers = new Timers(), bridge = new Bridge(), rejected = [];
  bridge.socket = { readyState: 1, close() { this.readyState = 2; } };
  bridge.closed = new Promise(() => {});
  const timer = timers.setTimeout(() => assert.fail('Pending request timer must be cancelled'), 20000);
  bridge.pending = new Map([[1, { timeout: timer, reject: error => rejected.push(String(error)) }]]);
  const result = await settle(bridge.finalize(2000, timers), timers);
  assert.ifError(result.error); assert.equal(timers.now, 2000);
  assert.equal(result.value.close_confirmed, false); assert.match(result.value.error, /not confirmed/);
  assert.equal(rejected.length, 1); assert.equal(bridge.pending.size, 0); assert.equal(timers.pending.size, 0);
});

test('already closed CDP socket cancels pending requests without issuing another close', async () => {
  const timers = new Timers(), bridge = new Bridge();
  bridge.socket = { readyState: 3, close() { assert.fail('Already closed'); } };
  bridge.pending = new Map();
  assert.equal((await bridge.finalize(2000, timers)).close_confirmed, true);
  assert.equal(timers.pending.size, 0);
});

test('failed CLI exits nonzero after both streams flush without waiting for unrelated handles', () => {
  const timers = new Timers(), calls = [], output = [];
  const runtime = { stdout: { write: (text, done) => done() }, stderr: { write: (text, done) => { output.push(text); done(); } }, exit: code => calls.push(code) };
  exitFailedController(new Error('original failure'), runtime, timers);
  assert.equal(runtime.exitCode, 1); assert.deepEqual(calls, [1]);
  assert.match(output[0], /original failure/); assert.equal(timers.pending.size, 0);
});

test('failed CLI flush deadline cannot be held open or exit twice on late callbacks', () => {
  const timers = new Timers(), calls = [], callbacks = [];
  const runtime = { stdout: { write: (text, done) => callbacks.push(done) }, stderr: { write: (text, done) => callbacks.push(done) }, exit: code => calls.push(code) };
  exitFailedController(new Error('original failure'), runtime, timers);
  timers.next(); assert.equal(timers.now, 1000);
  callbacks.forEach(done => done()); assert.deepEqual(calls, [1]);
});
