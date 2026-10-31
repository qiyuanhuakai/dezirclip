import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { closeSync, fsyncSync, mkdtempSync, openSync, readFileSync, rmSync, writeSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { NativeJsonLines, NativeStreamLog, nativeStreamPolicy } from './native-stream.mjs';
import { trackManagedChild } from './native-windows-lifecycle.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
function memoryLog(policy = nativeStreamPolicy, overrides = {}) {
  const bytes = [Buffer.alloc(0), Buffer.alloc(0)], failures = [], actions = [];
  const io = {
    writeSync(fd, chunk, offset, count) { bytes[fd] = Buffer.concat([bytes[fd], chunk.subarray(offset, offset + count)]); return count; },
    fsyncSync(fd) { actions.push(['fsync', fd]); }, closeSync(fd) { actions.push(['close', fd]); },
    readback(file) { const data = bytes[file.endsWith('.stdout.log') ? 0 : 1]; return { bytes: data.length, sha256: sha(data) }; },
    ...overrides,
  };
  const log = new NativeStreamLog([0, 1], '.', 'native', error => failures.push(error), io, policy);
  const values = [], parser = new NativeJsonLines(log, value => values.push(value));
  return { log, parser, values, failures, bytes, actions, io,
    feed(data, index = 0) { log.receive(index, data); if (!index) parser.push(data); },
    finish(closed = true) { parser.end(); return log.finish(closed); } };
}

test('more than 16 MiB of legitimate Native output stays byte-exact without retained whole-stream buffers', () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'dzc-stream-pure-'));
  try {
    const files = ['stdout', 'stderr'].map(name => openSync(path.join(directory, 'native.' + name + '.log'), 'wx'));
    const failures = [], log = new NativeStreamLog(files, directory, 'native', error => failures.push(error));
    const frame = Buffer.from(JSON.stringify({ id: 1, value: 'a'.repeat(32768) }) + '\n'), expected = createHash('sha256');
    let frames = 0; const parser = new NativeJsonLines(log, () => frames++);
    for (let index = 0; index < 520; index++) { log.receive(0, frame); parser.push(frame); expected.update(frame); }
    parser.end(); const result = log.finish(true), expectedSha = expected.digest('hex');
    assert(result.stdout.bytes > 16 * 1024 * 1024); assert.equal(result.stdout.sha256, expectedSha);
    assert.equal(result.native_stream.streams[0].received_sha256, expectedSha);
    assert.equal(result.native_stream.complete, true); assert.equal(frames, 520); assert.equal(failures.length, 0);
    assert.equal(parser.buffer.length, 1024 * 1024); assert.equal(parser.used, 0);
    assert.equal(log.stdout, undefined); assert.equal(log.stderr, undefined);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('UTF8 split at every byte, multiple frames and CRLF preserve original bytes and decoded values', () => {
  const fixture = memoryLog(), data = Buffer.from('{"value":"中文🌸"}\r\n{"id":2,"value":"ok"}\n');
  for (const byte of data) fixture.feed(Buffer.from([byte]));
  const result = fixture.finish();
  assert.deepEqual(fixture.values, [{ value: '中文🌸' }, { id: 2, value: 'ok' }]);
  assert(fixture.bytes[0].equals(data)); assert.equal(result.stdout.sha256, sha(data)); assert.equal(result.native_stream.complete, true);
});

test('quota failure permanently stops BOTH streams and records received versus persisted hashes', () => {
  const fixture = memoryLog({ ...nativeStreamPolicy, combined_disk_bytes: 20 });
  fixture.feed(Buffer.from('{"id":1}\n')); fixture.feed(Buffer.from('x'.repeat(21)), 1);
  fixture.feed(Buffer.from('{"id":2}\n')); fixture.feed(Buffer.from('tiny'), 1);
  const result = fixture.finish();
  assert.equal(fixture.failures.length, 1); assert.equal(result.native_stream.first_error.stage, 'quota');
  assert.equal(fixture.bytes[0].toString(), '{"id":1}\n'); assert.equal(fixture.bytes[1].length, 0);
  assert.deepEqual(fixture.values, [{ id: 1 }, { id: 2 }]); assert.equal(result.native_stream.complete, false);
  assert.equal(result.native_stream.streams[0].unpersisted_bytes, 9);
  assert.equal(result.native_stream.streams[1].unpersisted_bytes, 25);
  assert.equal(result.native_stream.streams[0].received_sha256, sha(Buffer.from('{"id":1}\n{"id":2}\n')));
});

test('partial writes are accounted exactly; subsequent write failure latches an exact prefix', () => {
  const fixture = memoryLog(), original = fixture.io.writeSync; let count = 0;
  fixture.io.writeSync = (fd, chunk, offset, length) => {
    if (++count > 1) throw new Error('synthetic disk failure');
    return original(fd, chunk, offset, Math.min(3, length));
  };
  fixture.feed(Buffer.from('{"id":1}\n')); fixture.feed(Buffer.from('{"id":2}\n'));
  const result = fixture.finish();
  assert.equal(result.stdout.bytes, 3); assert.equal(result.stdout.sha256, sha(Buffer.from('{"i')));
  assert.equal(result.native_stream.streams[0].received_bytes, 18); assert.equal(fixture.failures.length, 1);
  assert.equal(result.native_stream.first_error.stage, 'write'); assert.equal(result.native_stream.complete, false);
});

test('zero write fails closed; partial successful writes are retried only for remaining bytes', () => {
  const zero = memoryLog(nativeStreamPolicy, { writeSync: () => 0 }); zero.feed(Buffer.from('{}\n'));
  assert.equal(zero.finish().native_stream.complete, false); assert.equal(zero.log.firstError.stage, 'write');
  const partial = memoryLog(), original = partial.io.writeSync;
  partial.io.writeSync = (fd, chunk, offset, count) => original(fd, chunk, offset, Math.min(2, count));
  partial.feed(Buffer.from('{}\n')); assert.equal(partial.finish().native_stream.complete, true);
});

test('overlong frame is bounded and resynchronizes for a later cleanup response only', () => {
  const fixture = memoryLog({ ...nativeStreamPolicy, stdout_frame_bytes: 16 });
  fixture.feed(Buffer.from('x'.repeat(17))); fixture.feed(Buffer.from('xxx\n{"id":2}\n'));
  const result = fixture.finish();
  assert.equal(result.native_stream.complete, false); assert.equal(result.native_stream.first_error.stage, 'frame-size');
  assert.deepEqual(fixture.values, [{ id: 2 }]); assert.equal(fixture.parser.buffer.length, 16);
  assert.deepEqual(result.native_stream.framing.first_discarded_ranges, [{ start: 0, end: 21 }]);
  assert.equal(fixture.bytes[0].length, 17); assert.equal(result.native_stream.streams[0].unpersisted_bytes, 13);
});

test('invalid UTF8, invalid JSON and incomplete EOF fail without substituting replacement characters', () => {
  for (const data of [Buffer.from([123, 34, 120, 34, 58, 34, 255, 34, 125, 10]), Buffer.from('bad\n'), Buffer.from('{"id":1}')]) {
    const fixture = memoryLog(); fixture.feed(data); const result = fixture.finish();
    assert.equal(result.native_stream.complete, false); assert.equal(fixture.values.length, 0);
    assert.equal(result.native_stream.framing.discarded_bytes, data.length);
  }
});

test('each fsync/close/readback is attempted and a missing stdio close cannot be complete', () => {
  const fixture = memoryLog(); fixture.feed(Buffer.from('{}\n'));
  fixture.io.fsyncSync = fd => { fixture.actions.push(['fsync', fd]); if (fd === 0) throw new Error('sync failed'); };
  fixture.io.closeSync = fd => { fixture.actions.push(['close', fd]); if (fd === 0) throw new Error('close failed'); };
  fixture.io.readback = () => ({ bytes: -1, sha256: 'bad' });
  const result = fixture.finish();
  assert.deepEqual(fixture.actions, [['fsync', 0], ['close', 0], ['fsync', 1], ['close', 1]]);
  assert.equal(result.native_stream.first_error.stage, 'fsync'); assert.equal(fixture.failures.length, 1);
  assert.equal(result.native_stream.complete, false); assert(result.native_stream.finalization.every(item => !item.readback_verified));
  const detached = memoryLog(); detached.feed(Buffer.from('{}\n'));
  assert.equal(detached.finish(false).native_stream.complete, false);
});

function channelFixture() {
  const source = readFileSync(new URL('./native-plumbing.mjs', import.meta.url), 'utf8');
  const logged = source.slice(source.indexOf('class LoggedProcess {'), source.indexOf('export async function completedCommand('));
  const native = source.slice(source.indexOf('class NativeChannel extends LoggedProcess {'), source.indexOf('export class Bridge {'));
  const children = [], outputs = [], files = new Map(); let nextFd = 0;
  const spawn = () => {
    const child = new EventEmitter(); child.pid = 101 + children.length;
    child.stdout = new EventEmitter(); child.stderr = new EventEmitter(); child.stdin = new EventEmitter();
    child.stdin.write = text => { child.lastInput = JSON.parse(text); };
    children.push(child); return child;
  };
  class FakeNativeLog extends NativeStreamLog {
    constructor(fds, root, label, onFailure) {
      super(fds, root, label, onFailure, {
        writeSync(fd, chunk, offset, length) { files.set(fd, Buffer.concat([files.get(fd), chunk.subarray(offset, offset + length)])); return length; },
        fsyncSync() {}, closeSync() {}, readback(file) { const index = file.endsWith('.stdout.log') ? 0 : 1, bytes = files.get(fds[index]); return { bytes: bytes.length, sha256: sha(bytes) }; },
      });
    }
  }
  const scope = { assert, Buffer, path, Date, Error, Promise, setTimeout, clearTimeout, trackManagedChild,
    NativeStreamLog: FakeNativeLog, NativeJsonLines, repository: '.', nativeSource: 'not-executed.py', maximumLog: 16 * 1024 * 1024,
    environment: () => ({}), spawn, openSync: () => { const fd = nextFd++; files.set(fd, Buffer.alloc(0)); return fd; },
    save: (file, value) => outputs.push({ file, value }), fsyncSync, closeSync, writeSync,
    regular: () => { throw new Error('Streaming logs must not call whole-file regular()'); },
  };
  vm.createContext(scope); vm.runInContext(logged + native + '\nthis.NativeChannel = NativeChannel; this.LoggedProcess = LoggedProcess;', scope);
  const channel = new scope.NativeChannel('fake-python', '.', 'source', 1);
  return { channel, child: children[0], outputs, scope };
}

test('actual derived channel rejects pending and future ordinary operations after first failure, but receives stop and original close', async () => {
  const { channel, child, outputs } = channelFixture();
  child.stdout.emit('data', Buffer.from('{"event":"ready"}\n')); await channel.ready;
  const ordinary = channel.request('observe'); const rejected = assert.rejects(ordinary, /Native stream/);
  child.stdout.emit('data', Buffer.from('broken\n')); await rejected;
  assert.throws(() => channel.request('observe'), /Native stream/);
  const stop = channel.request('stop'), id = child.lastInput.id;
  child.stdout.emit('data', Buffer.from(JSON.stringify({ id, value: { stopped: true } }) + '\n'));
  assert.equal((await stop).stopped, true); assert.equal(channel.pending.size, 0);
  child.stdout.emit('end'); child.emit('exit', 0, null); child.emit('close', 0, null); await channel.closed;
  channel.finish(); assert.equal(outputs.length, 1); assert.equal(outputs[0].value.native_stream.complete, false);
  assert.equal(outputs[0].value.process_exit.observed, true); assert.equal(outputs[0].value.stdio_close.observed, true);
  assert.equal(channel.stdout.length, 0); assert.equal(channel.stderr.length, 0);
});

test('derived channel early parse failure has a handled ready rejection and unconfirmed close stays incomplete', async () => {
  const { channel, child, outputs } = channelFixture();
  child.stdout.emit('data', Buffer.from('broken\n')); await assert.rejects(channel.ready, /Native stream/);
  channel.record.teardown = { stdio_detached: true }; channel.finish();
  assert.equal(outputs[0].value.native_stream.complete, false);
  child.emit('exit', 1, null); child.emit('close', 1, null); await channel.closed;
});

test('actual inherited stderr pipe error preserves an already pending stop while latching persistence', async () => {
  const { channel, child, outputs } = channelFixture();
  child.stdout.emit('data', Buffer.from('{"event":"ready"}\n')); await channel.ready;
  const stop = channel.request('stop'), id = child.lastInput.id;
  child.stderr.emit('error', new Error('synthetic pipe failure'));
  assert.equal(channel.nativeLog.firstError.stage, 'pipe');
  assert.equal(channel.pending.get(id).operation, 'stop');
  child.stdout.emit('data', Buffer.from(JSON.stringify({ id, value: { stopped: true } }) + '\n'));
  assert.equal((await stop).stopped, true);
  child.stdout.emit('end'); child.emit('exit', 0, null); child.emit('close', 0, null); await channel.closed;
  channel.finish(); assert.equal(outputs[0].value.native_stream.complete, false);
  assert.equal(outputs[0].value.native_stream.streams[0].persisted_bytes, Buffer.byteLength('{"event":"ready"}\n'));
  assert.equal(outputs[0].value.stream_errors.length, 1);
});

test('preserving stop after a pipe error cannot accept an original child exit without the stop response', async () => {
  const { channel, child, outputs } = channelFixture();
  child.stdout.emit('data', Buffer.from('{"event":"ready"}\n')); await channel.ready;
  const stop = channel.request('stop'), rejected = assert.rejects(stop, /Native target exited/);
  child.stdout.emit('error', new Error('synthetic stdout pipe failure'));
  assert.equal(channel.pending.size, 1);
  child.stdout.emit('end'); child.emit('exit', 1, null); child.emit('close', 1, null);
  await rejected; await channel.closed; channel.finish();
  assert.equal(channel.pending.size, 0); assert.equal(outputs[0].value.native_stream.complete, false);
  assert.equal(outputs[0].value.process_exit.exit_code, 1);
});
