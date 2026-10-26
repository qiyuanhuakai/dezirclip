import assert from 'node:assert/strict';
import test from 'node:test';
import { validateCiContext, validateBuildProof, validateDesktopIdentity, verifyPresented } from './native-windows-surface.mjs';

const source = 'a'.repeat(40);
const expected = { component_version: '0.2.0-alpha.5', source_revision: source, source_state: 'verified_checkout',
  build_id: '0.2.0-alpha.5@' + source, target: 'x86_64-pc-windows-msvc', toolkit_patch_id: 'mygo-v0.6.4-no-startup-cleanup-v1' };
const info = { ...expected, component: 'surface', protocol_version: { major: 1, minor: 0 },
  mygo_version: 'v0.6.4', mygo_source_revision: '6255ea40139d06e0762a7c4a32626148ba410478' };
const proof = { schema_version: 1, kind: 'surface-build', identity_verification: 'executed_native_build_info',
  sha256: 'b'.repeat(64), expected, build_info: info, inputs: [{ path: 'frontends/dzc-surface/go.mod', sha256: 'c'.repeat(64) }],
  toolkit_patch: { id: expected.toolkit_patch_id, strategy: 'isolated_module_copy',
    upstream_app_sha256: 'af53a7f7c0dbe36d42edc6de34f9f7735569ae53170715264af6618b99e281b9',
    patched_app_sha256: 'e79050f98a9aae53d3413e0680eb91e66d3d18cc677163acca97ed101ac1fa68', changed_files: ['app.go'] } };
const helper = 'C:/fixture/candidate/dzc-surface.exe';
const status = { renderer: 'mygo', managed_pid: 20, session_id: 'actual-session', visible: true, last_error: null };
const observation = { processes: [{ pid: 20, running: true, image: helper, creation_time_100ns: 100 }],
  windows: [{ hwnd: 30, pid: 20, visible: true, rect: { x: -100, y: 200, width: 560, height: 440 } }], foreground: { pid: 20 } };

test('native QA context accepts actual event shapes without requiring a release tag', () => {
  for (const event of ['pull_request', 'push', 'workflow_dispatch']) validateCiContext({
    GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_SHA: source, GITHUB_RUN_ID: '12', GITHUB_RUN_ATTEMPT: '1', GITHUB_EVENT_NAME: event, RUNNER_TEMP: 'C:/runner/temp',
  }, 'win32', source);
});

test('native QA rejects a user machine, another repository and a different source', () => {
  const env = { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_SHA: source, GITHUB_RUN_ID: '12', GITHUB_RUN_ATTEMPT: '1', GITHUB_EVENT_NAME: 'pull_request', RUNNER_TEMP: 'C:/runner/temp' };
  assert.throws(() => validateCiContext(env, 'linux', source));
  for (const patch of [{ GITHUB_ACTIONS: 'false' }, { RUNNER_ENVIRONMENT: 'self-hosted' },
    { GITHUB_REPOSITORY: 'someone/example' }, { GITHUB_SHA: 'd'.repeat(40) }, { GITHUB_RUN_ID: '0' }]) {
    assert.throws(() => validateCiContext({ ...env, ...patch }, 'win32', source));
  }
});

test('Surface proof binds actual source/version/target and raw binary hash', () => {
  assert.doesNotThrow(() => validateBuildProof(proof, info, expected, proof.sha256));
  assert.throws(() => validateBuildProof(proof, info, expected, 'd'.repeat(64)));
  for (const patch of [{ component: 'desktop' }, { source_state: 'modified_checkout' },
    { source_revision: 'd'.repeat(40) }, { target: 'aarch64-pc-windows-msvc' },
    { toolkit_patch_id: undefined },
    { mygo_source_revision: 'd'.repeat(40) }, { protocol_version: { major: 2, minor: 0 } }]) {
    assert.throws(() => validateBuildProof(proof, { ...info, ...patch }, expected, proof.sha256));
  }
  assert.throws(() => validateBuildProof({ ...proof, identity_verification: 'cross_target_not_executed' }, info, expected, proof.sha256));
  assert.throws(() => validateBuildProof({ ...proof, toolkit_patch: { ...proof.toolkit_patch, strategy: 'modified_module_cache' } }, info, expected, proof.sha256));
  assert.throws(() => validateBuildProof({ ...proof, toolkit_patch: { ...proof.toolkit_patch, upstream_app_sha256: 'd'.repeat(64) } }, info, expected, proof.sha256));
  assert.throws(() => validateBuildProof({ ...proof, toolkit_patch: { ...proof.toolkit_patch, patched_app_sha256: 'd'.repeat(64) } }, info, expected, proof.sha256));
});

test('presented status alone never substitutes for real native process, window and foreground', () => {
  assert.equal(verifyPresented(status, observation, helper).hwnd, 30);
  assert.throws(() => verifyPresented(status, { ...observation, windows: [] }, helper));
  assert.throws(() => verifyPresented(status, { ...observation, foreground: { kind: 'other-uninspected' } }, helper));
  assert.throws(() => verifyPresented(status, { ...observation, processes: [{ ...observation.processes[0], running: false }] }, helper));
  assert.throws(() => verifyPresented(status, observation, 'C:/another/dzc-surface.exe'));
});

test('Desktop identity keeps its actual Rust role, Host inventory and source constraints', () => {
  const desktop = { ...expected, component: 'desktop', metadata_version: 1, database_schema_version: 24,
    revision_origin: 'explicit_checkout', protocol_version: { major: 1, minor: 0 },
    linked_components: ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host'].map(component => ({ component, version: expected.component_version })) };
  assert.doesNotThrow(() => validateDesktopIdentity(desktop, expected));
  assert.throws(() => validateDesktopIdentity({ ...desktop, source_state: 'modified_checkout' }, expected));
  assert.throws(() => validateDesktopIdentity({ ...desktop, database_schema_version: 21 }, expected));
  assert.throws(() => validateDesktopIdentity({ ...desktop, linked_components: desktop.linked_components.slice(0, 3) }, expected));
});

test('two visible native windows and an unexpected warm process are rejected', () => {
  assert.throws(() => verifyPresented(status, { ...observation, windows: [...observation.windows, { ...observation.windows[0], hwnd: 31 }] }, helper));
  assert.throws(() => verifyPresented(status, observation, helper, 21));
  assert.throws(() => verifyPresented({ ...status, visible: false }, observation, helper));
  assert.throws(() => verifyPresented(status, { ...observation, processes: [...observation.processes,
    { ...observation.processes[0], pid: 21 }] }, helper));
});
