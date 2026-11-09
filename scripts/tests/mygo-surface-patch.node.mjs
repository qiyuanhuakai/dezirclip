import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { MYGO_PATCH_ID, patchVerifiedMyGoApp, removeStartupCleanupCall } from '../mygo-surface-patch.mjs';

test('Rust admission and packaged Go/Linux helpers require the patch actually built', () => {
  const files = [
    ['../../src-tauri/src/app/surface_supervisor/identity.rs', /const TOOLKIT_PATCH: &str = "([^"]+)"/],
    ['../../frontends/dzc-surface/internal/buildinfo/buildinfo.go', /const RequiredToolkitPatch = "([^"]+)"/],
    ['../build-linux-release-inputs.py', /PATCH = '([^']+)'/],
    ['./native-windows-surface.mjs', /const toolkitPatchId = '([^']+)'/],
  ];
  for (const [file, pattern] of files) {
    const source = readFileSync(new URL(file, import.meta.url), 'utf8');
    assert.equal(pattern.exec(source)?.[1], MYGO_PATCH_ID, file);
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
