import assert from 'node:assert/strict';
import test from 'node:test';
import { patchVerifiedMyGoApp, removeStartupCleanupCall } from '../mygo-surface-patch.mjs';

test('the startup patch removes only the single asynchronous cleanup invocation', () => {
  const before = 'func Run() {\n\tgo cleanUpdateLeftovers()\n\tstartWindow()\n}\n';
  assert.equal(removeStartupCleanupCall(before), 'func Run() {\n\tstartWindow()\n}\n');
});

test('missing or duplicated cleanup calls fail closed', () => {
  for (const value of ['', '\tcleanUpdateLeftovers()\n', '\tgo cleanUpdateLeftovers()\n\tgo cleanUpdateLeftovers()\n']) {
    assert.throws(() => removeStartupCleanupCall(value), /exactly one/);
  }
});

test('even a matching call cannot patch an unreviewed upstream file', () => {
  assert.throws(() => patchVerifiedMyGoApp(Buffer.from('\tgo cleanUpdateLeftovers()\n')), /reviewed source/);
});
