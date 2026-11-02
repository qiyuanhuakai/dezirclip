import assert from 'node:assert/strict';
import test from 'node:test';
import { collectOwnedCrashEvents, desktopExitVerdict, selectOwnedCrashEvents, validateCrashEventContext } from './native-windows-crash-events.mjs';

const image = String.raw`D:\a\_temp\dezirclip-surface-b1b5dc0c39ff455ab18aa841a6f664f1\candidate\dezirclip.exe`;
const identity = { pid: 8, creation_time_100ns: '134376754256927223', original_handle_retained: true,
  image, started_at: '2026-10-28T15:37:05.699Z', closed_at: '2026-10-28T15:37:13.879Z' };
const cleanup = '2026-10-28T15:37:34.335Z';
const exitRecord = { pid: 8, executable: image, started_at: identity.started_at, closed_at: identity.closed_at,
  exit_code: 3221226356, signal: null };
const env = { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
  GITHUB_SHA: 'a92018194ead9514367085a24432e0ccbb9f168c', GITHUB_RUN_ID: '40714662961', GITHUB_RUN_ATTEMPT: '1',
  GITHUB_EVENT_NAME: 'pull_request', RUNNER_TEMP: String.raw`D:\a\_temp`, SystemRoot: String.raw`C:\Windows` };
const id = 'd18cef3c-1525-42f4-8b79-839768791353';
const crash = (overrides = {}) => ({ id: 1000, provider: 'Application Error', time_created: '2026-10-28T15:37:14.000Z', record_id: '42',
  fields: { AppPath: image.toUpperCase(), ProcessId: '0x8', ProcessCreationTime: '0x' + BigInt(identity.creation_time_100ns).toString(16),
    ModuleName: 'ntdll.dll', ModulePath: String.raw`C:\Windows\System32\ntdll.dll`, ExceptionCode: 'c0000374', FaultingOffset: '00000001',
    IntegratorReportId: '{' + id + '}', unrelated_private_field: 'must not be returned' }, ...overrides });
const wer = (report = id) => ({ id: 1001, provider: 'Windows Error Reporting', time_created: '2026-10-28T15:37:15.000Z', record_id: '43', fields: { ReportId: report } });

test('only normal exit and explicitly recorded cleanup termination are accepted', () => {
  assert.equal(desktopExitVerdict({ ...exitRecord, exit_code: 0 }, cleanup).allowed, true);
  const terminated = { ...exitRecord, exit_code: 1, closed_at: '2026-10-28T15:37:35.000Z' };
  assert.equal(desktopExitVerdict(terminated, cleanup, [8]).allowed, true);
  assert.equal(desktopExitVerdict(terminated, cleanup, [9]).allowed, false);
  assert.equal(desktopExitVerdict({ ...exitRecord, exit_code: 1 }, cleanup, [8]).allowed, false);
});

test('heap corruption stays a failure before, during, and after cleanup, signed or unsigned', () => {
  for (const exit_code of [3221226356, -1073740940]) for (const closed_at of [identity.closed_at, cleanup, '2026-10-28T15:37:35.000Z']) {
    const result = desktopExitVerdict({ ...exitRecord, exit_code, closed_at }, cleanup, [8]);
    assert.equal(result.allowed, false);
    assert.equal(result.exit_code_hex, '0xc0000374');
    assert.equal(result.abnormal_before_cleanup, closed_at === identity.closed_at);
  }
});

test('signals and incomplete original process records never count as normal cleanup', () => {
  assert.equal(desktopExitVerdict({ ...exitRecord, exit_code: 0, signal: 'SIGTERM' }, cleanup).allowed, false);
  for (const bad of [{ pid: null }, { exit_code: null }, { closed_at: null }, { exit_code: 4294967296 }]) {
    assert.equal(desktopExitVerdict({ ...exitRecord, ...bad }, cleanup).allowed, false);
  }
});

test('Application Error requires exact PID, FILETIME, absolute path, provider, and interval', () => {
  const result = selectOwnedCrashEvents([crash()], identity);
  assert.equal(result.status, 'matched');
  assert.equal(result.events[0].exception_code, 'c0000374');
  assert.equal(result.events[0].report_id, id);
  assert.equal(result.events[0].creation_time_100ns, identity.creation_time_100ns);
  assert.equal(JSON.stringify(result).includes('must not be returned'), false);
  for (const bad of [
    { fields: { ...crash().fields, ProcessId: '9' } },
    { fields: { ...crash().fields, AppPath: String.raw`D:\other\dezirclip.exe` } },
    { fields: { ...crash().fields, AppPath: 'dezirclip.exe' } },
    { provider: 'Other Application' },
    { time_created: '2026-10-28T15:37:05.000Z' },
    { time_created: '2026-10-28T15:37:19.000Z' },
  ]) assert.equal(selectOwnedCrashEvents([crash(bad)], identity).status, 'missing');
});

test('missing or reused-PID creation time remains unbound even if all other fields match', () => {
  for (const ProcessCreationTime of [undefined, '', 'not-a-number', '134376754256927224']) {
    const result = selectOwnedCrashEvents([crash({ fields: { ...crash().fields, ProcessCreationTime } }), wer()], identity);
    assert.equal(result.status, 'unbound');
    assert.equal(result.unbound_count, 1);
    assert.deepEqual(result.events, []);
  }
});

test('WER is associated only through a report ID from an already bound Application Error', () => {
  assert.equal(selectOwnedCrashEvents([wer()], identity).status, 'missing');
  const result = selectOwnedCrashEvents([wer(), crash(), wer('d18cef3c-1525-42f4-8b79-839768791354')], identity);
  assert.deepEqual(result.events.map(row => row.id), [1000, 1001]);
  assert.equal(result.events[1].association, 'exact_report_id_from_bound_application_error');
  assert.equal(selectOwnedCrashEvents([crash({ fields: { ...crash().fields, IntegratorReportId: 'invalid' } }), wer()], identity).events.length, 1);
  const zeroId = '00000000-0000-0000-0000-000000000000';
  const zero = selectOwnedCrashEvents([crash({ fields: { ...crash().fields, IntegratorReportId: zeroId } }), wer(zeroId)], identity);
  assert.equal(zero.events.length, 1);
  assert.equal(zero.events[0].report_id, null);
});

test('identity must retain the original handle and lossless creation time', () => {
  for (const bad of [{ original_handle_retained: false }, { creation_time_100ns: 134376754256927223 },
    { creation_time_100ns: '0' }, { creation_time_100ns: '18446744073709551616' }, { pid: 0 }, { image: 'dezirclip.exe' }]) {
    assert.throws(() => selectOwnedCrashEvents([], { ...identity, ...bad }));
  }
  assert.throws(() => selectOwnedCrashEvents(Array(65).fill(crash()), identity));
});

test('event reader is restricted to the hosted repository workflow and exact private candidate', () => {
  assert.doesNotThrow(() => validateCrashEventContext(identity, env, 'win32'));
  for (const bad of [{ GITHUB_ACTIONS: 'false' }, { RUNNER_ENVIRONMENT: 'self-hosted' }, { GITHUB_REPOSITORY: 'other/repository' },
    { GITHUB_SHA: '' }, { GITHUB_RUN_ID: '0' }, { RUNNER_TEMP: String.raw`D:\other` }, { GITHUB_EVENT_NAME: 'pull_request_target' }]) {
    assert.throws(() => validateCrashEventContext(identity, { ...env, ...bad }, 'win32'));
  }
  assert.throws(() => validateCrashEventContext(identity, env, 'linux'));
  assert.throws(() => validateCrashEventContext({ ...identity, image: String.raw`D:\a\_temp\dezirclip.exe` }, env, 'win32'));
});

test('only one exact performance cohort directory may wrap the private candidate', () => {
  const cohort = 'dezirclip-performance-123456789012345678901234567890ab';
  const privateRelative = image.slice(env.RUNNER_TEMP.length + 1);
  assert.doesNotThrow(() => validateCrashEventContext({ ...identity, image: env.RUNNER_TEMP + '\\' + cohort + '\\' + privateRelative }, env, 'win32'));
  for (const prefix of ['other', 'dezirclip-performance-123', cohort + '\\other', cohort + '\\' + cohort]) {
    assert.throws(() => validateCrashEventContext({ ...identity, image: env.RUNNER_TEMP + '\\' + prefix + '\\' + privateRelative }, env, 'win32'));
  }
});

test('normal or cleanup-time exits skip event collection without touching Windows APIs', async () => {
  assert.equal((await collectOwnedCrashEvents(identity, { ...exitRecord, exit_code: 0 }, cleanup, {}, 'not-windows')).status, 'not_applicable');
  assert.equal((await collectOwnedCrashEvents(identity, { ...exitRecord, closed_at: cleanup }, cleanup, {}, 'not-windows')).status, 'not_applicable');
});

test('unverified local context and mismatched original records fail closed before spawning', async () => {
  assert.equal((await collectOwnedCrashEvents(identity, exitRecord, cleanup, {}, 'not-windows')).status, 'unavailable');
  for (const bad of [{ pid: 9 }, { executable: String.raw`D:\other\dezirclip.exe` }, { started_at: '2026-10-28T15:37:06.000Z' },
    { closed_at: '2026-10-28T15:37:14.000Z' }]) {
    const result = await collectOwnedCrashEvents(identity, { ...exitRecord, ...bad }, cleanup, env, 'win32');
    assert.equal(result.status, 'unavailable');
    assert.deepEqual(result.events, []);
    assert.equal(result.verdict.allowed, false);
  }
});
