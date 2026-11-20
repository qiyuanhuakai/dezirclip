import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { MYGO_TOOLKIT, patchVerifiedMyGoApp, removeStartupCleanupCall } from '../mygo-surface-patch.mjs';

test('Go module locks match the reviewed MyGo toolkit manifest', () => {
  const { version, module, module_sum: moduleSum, module_go_mod_sum: goModSum } = MYGO_TOOLKIT;
  for (const directory of ['frontends/dzc-surface', 'packages/ui-native', 'packages/ui-presets']) {
    const read = file => readFileSync(new URL(`../../${directory}/${file}`, import.meta.url), 'utf8');
    const requirements = read('go.mod').split(/\r?\n/).map(line => line.trim().replace(/^require\s+/, ''));
    assert(requirements.includes(`${module} ${version}`), `${directory}/go.mod needs ${module} ${version}`);
    const sums = read('go.sum').split(/\r?\n/);
    assert(sums.includes(`${module} ${version} ${moduleSum}`), `${directory}/go.sum has a different MyGo module checksum`);
    assert(sums.includes(`${module} ${version}/go.mod ${goModSum}`), `${directory}/go.sum has a different MyGo go.mod checksum`);
  }
});

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
