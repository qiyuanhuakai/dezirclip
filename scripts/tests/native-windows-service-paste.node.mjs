import test from 'node:test';
import assert from 'node:assert/strict';
import { assertNoFixtureInput, pollRead } from './native-windows-service-paste.mjs';

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
