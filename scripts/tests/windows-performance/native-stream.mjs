import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { closeSync, fstatSync, fsyncSync, openSync, readSync, writeSync } from 'node:fs';
import path from 'node:path';

export const nativeStreamPolicy = Object.freeze({
  id: 'performance-native-stream-v1', combined_disk_bytes: 128 * 1024 * 1024,
  stdout_frame_bytes: 1024 * 1024,
});
const names = ['stdout', 'stderr'];
const digest = hash => hash.copy().digest('hex');

function readback(file, expectedBytes) {
  const fd = openSync(file, 'r'), hash = createHash('sha256'), buffer = Buffer.alloc(64 * 1024);
  let bytes = 0;
  try {
    assert(fstatSync(fd).isFile(), 'Native log is not a regular file');
    assert.equal(fstatSync(fd).size, expectedBytes, 'Native log size changed before readback');
    for (;;) {
      const count = readSync(fd, buffer, 0, buffer.length, null);
      if (!count) break;
      bytes += count; hash.update(buffer.subarray(0, count));
    }
    assert.equal(fstatSync(fd).size, bytes, 'Native log changed during readback');
    return { bytes, sha256: hash.digest('hex') };
  } finally { closeSync(fd); }
}

export class NativeStreamLog {
  constructor(files, root, label, onFailure, io = { writeSync, fsyncSync, closeSync, readback }, policy = nativeStreamPolicy) {
    this.files = files; this.onFailure = onFailure; this.io = io; this.policy = policy;
    this.failure = null; this.firstError = null; this.sealed = false; this.totalPersisted = 0;
    this.streams = names.map(name => ({ path: path.join(root, label + '.' + name + '.log'),
      received: 0, persisted: 0, receivedHash: createHash('sha256'), persistedHash: createHash('sha256') }));
  }
  fail(stage, stream, error) {
    if (this.failure) return;
    this.firstError = { stage, stream, message: String(error?.message ?? error).slice(0, 2048),
      received_bytes: this.streams.map(item => item.received), persisted_bytes: this.streams.map(item => item.persisted) };
    this.failure = new Error('Native stream ' + stage + ': ' + this.firstError.message);
    this.onFailure(this.failure);
  }
  receive(index, chunk) {
    const stream = this.streams[index];
    stream.received += chunk.length; stream.receivedHash.update(chunk);
    if (this.sealed) { this.fail('after-seal', names[index], 'Output arrived after log finalization'); return; }
    if (this.failure) return;
    if (this.totalPersisted + chunk.length > this.policy.combined_disk_bytes) {
      this.fail('quota', names[index], 'Combined native raw log quota exceeded'); return;
    }
    let offset = 0;
    try {
      while (offset < chunk.length) {
        const count = this.io.writeSync(this.files[index], chunk, offset, chunk.length - offset);
        assert(Number.isSafeInteger(count) && count > 0 && count <= chunk.length - offset, 'Native log write made invalid progress');
        stream.persistedHash.update(chunk.subarray(offset, offset + count));
        offset += count; stream.persisted += count; this.totalPersisted += count;
      }
    } catch (error) { this.fail('write', names[index], error); }
  }
  finish(stdioClosed) {
    if (this.result) return this.result;
    this.sealed = true;
    const finalization = [];
    for (const [index, fd] of this.files.entries()) {
      const state = { stream: names[index], fsync_attempted: true, fsync_ok: false, close_attempted: false, close_ok: false };
      try { this.io.fsyncSync(fd); state.fsync_ok = true; }
      catch (error) { this.fail('fsync', names[index], error); state.fsync_error = String(error).slice(0, 2048); }
      try { state.close_attempted = true; this.io.closeSync(fd); state.close_ok = true; }
      catch (error) { this.fail('close', names[index], error); state.close_error = String(error).slice(0, 2048); }
      finalization.push(state);
    }
    const logs = this.streams.map((stream, index) => {
      const persisted = { path: stream.path, bytes: stream.persisted, sha256: digest(stream.persistedHash) };
      try {
        const actual = this.io.readback(stream.path, persisted.bytes);
        assert.equal(actual.bytes, persisted.bytes, 'Persisted native log byte count changed');
        assert.equal(actual.sha256, persisted.sha256, 'Persisted native log SHA256 changed');
        finalization[index].readback_verified = true;
      } catch (error) {
        finalization[index].readback_verified = false;
        finalization[index].readback_error = String(error).slice(0, 2048); this.fail('readback', names[index], error);
      }
      return persisted;
    });
    this.result = { stdout: logs[0], stderr: logs[1], native_stream: {
      policy: this.policy, complete: Boolean(stdioClosed) && this.frameSnapshot?.().ended === true && !this.failure,
      stdio_close_observed: Boolean(stdioClosed), first_error: this.firstError,
      streams: this.streams.map((stream, index) => ({ stream: names[index], received_bytes: stream.received,
        received_sha256: digest(stream.receivedHash), persisted_bytes: stream.persisted,
        persisted_sha256: digest(stream.persistedHash), unpersisted_bytes: stream.received - stream.persisted })),
      persistence_scope: 'Each persisted log is an exact received-byte prefix. After the first error neither stream resumes persistence.',
      framing: this.frameSnapshot?.() ?? null, finalization,
    } };
    return this.result;
  }
}

export class NativeJsonLines {
  constructor(log, onValue) {
    this.log = log; this.onValue = onValue;
    this.buffer = Buffer.alloc(log.policy.stdout_frame_bytes); this.used = 0;
    this.offset = 0; this.frameStart = 0; this.discarding = false; this.ended = false;
    this.frames = 0; this.discardedBytes = 0; this.discardedFrames = 0; this.discardRanges = [];
    log.frameSnapshot = () => ({ parsed_frames: this.frames, received_bytes: this.offset, buffered_bytes: this.used,
      ended: this.ended, discarded_bytes: this.discardedBytes, discarded_frames: this.discardedFrames,
      first_discarded_ranges: [...this.discardRanges], ranges_truncated: this.discardedFrames > this.discardRanges.length });
  }
  discardEnd(end) {
    this.discardedFrames += 1; this.discardedBytes += end - this.frameStart;
    if (this.discardRanges.length < 8) this.discardRanges.push({ start: this.frameStart, end });
  }
  push(chunk) {
    let cursor = 0;
    while (cursor < chunk.length) {
      const newline = chunk.indexOf(10, cursor), end = newline < 0 ? chunk.length : newline;
      const length = end - cursor;
      if (!this.discarding && this.used + length > this.buffer.length) {
        this.discarding = true; this.used = 0;
        this.log.fail('frame-size', 'stdout', 'Native stdout JSONL frame exceeded its byte limit at ' + this.frameStart);
      }
      if (!this.discarding) { chunk.copy(this.buffer, this.used, cursor, end); this.used += length; }
      this.offset += length; cursor = end;
      if (newline < 0) break;
      cursor += 1; this.offset += 1;
      if (this.discarding) this.discardEnd(this.offset);
      else {
        try {
          const text = new TextDecoder('utf-8', { fatal: true }).decode(this.buffer.subarray(0, this.used));
          const value = JSON.parse(text);
          assert(value !== null && typeof value === 'object' && !Array.isArray(value), 'Native JSONL response must be an object');
          this.frames += 1; this.onValue(value);
        } catch (error) {
          this.discardEnd(this.offset); this.log.fail('frame-decode', 'stdout', error);
        }
      }
      this.used = 0; this.discarding = false; this.frameStart = this.offset;
    }
  }
  end() {
    this.ended = true;
    if (this.used || this.discarding) {
      this.discardEnd(this.offset); this.used = 0; this.discarding = false;
      this.log.fail('frame-eof', 'stdout', 'Native stdout ended in an incomplete JSONL frame');
    }
  }
}
