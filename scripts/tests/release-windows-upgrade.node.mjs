import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { assertAuthority, checkNames, combine, httpFailureDetails, inspectPe, mainWebviewTargets, nsisMembers, oldPackages, prepareFailure, releaseAssets,
  safeFailureText, sha256, updaterNames, validateAssets, validateHistoricalInfo, validateInfo, validateNative, validateRecoveryAuthority, verifyDownloads, version } from '../release-windows-upgrade.mjs';

// Synthetic report observations test pure rejection contracts. Application
// discovery extracts only its helper and never starts its empty fixture files.
// The Windows-only refusal check exits at the first hosted-runner guard.
const source = 'a'.repeat(40);
const releaseId = '42';
const policyHash = 'b'.repeat(64);
const clone = value => structuredClone(value);
const encode = value => Buffer.from(value).toString('base64');
const assetName = suffix => `DezirClip_${version}_${suffix}`;
const frozen = { 'app.quick_paste_hotkey': 'Ctrl+Alt+Shift+F9', 'app.classification_enabled': 'false',
  'app.hidden_filter_chips': 'release-upgrade-fixture', 'app.screenshot_enabled': 'false', 'app.ocr_enabled': 'false' };

test('Desktop discovery requires the unique main app target among auxiliary WebViews', () => {
  const page = url => ({ type: 'page', url, webSocketDebuggerUrl: 'ws://127.0.0.1:12345/devtools/page/synthetic' });
  const main = page('http://tauri.localhost/index.html');
  const auxiliary = [page('http://tauri.localhost/?window=quick-paste'), page('http://tauri.localhost/?window=region-select')];
  assert.deepEqual(mainWebviewTargets([...auxiliary, main]), [main]);
  assert.deepEqual(mainWebviewTargets(auxiliary), []);
  assert.equal(mainWebviewTargets([main, { ...main }]).length, 2);
  assert.equal(mainWebviewTargets([page('tauri://localhost/')]).length, 1);
});

test('Desktop discovery rejects foreign pages, query windows and malformed target lists', () => {
  const page = url => ({ type: 'page', url, webSocketDebuggerUrl: 'ws://127.0.0.1:12345/devtools/page/synthetic' });
  const bad = ['https://example.invalid/', 'http://tauri.localhost.example.invalid/', 'http://tauri.localhost@example.invalid/',
    'http://user:password@tauri.localhost/', 'tauri://other/', 'http://tauri.localhost/?window=',
    'http://tauri.localhost/?%77indow=auxiliary', 'not a URL'];
  assert.deepEqual(mainWebviewTargets(bad.map(page)), []);
  assert.deepEqual(mainWebviewTargets([null, {}, { ...page('http://tauri.localhost/'), type: 'worker' },
    { ...page('http://tauri.localhost/'), webSocketDebuggerUrl: '' }]), []);
  for (const value of [null, {}, Array(257).fill(null)]) assert.throws(() => mainWebviewTargets(value), /bounded Desktop target list/);
});

function signedDownloads() {
  const key = generateKeyPairSync('ed25519');
  const id = Buffer.from('0102030405060708', 'hex');
  const rawKey = key.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const publicKey = encode(`untrusted comment: synthetic public key\n${Buffer.concat([Buffer.from('Ed'), id, rawKey]).toString('base64')}`);
  const bytes = new Map();
  const assets = [];
  for (const suffix of ['x64-setup.exe', 'arm64-setup.exe', 'amd64.AppImage', 'aarch64.AppImage']) {
    const name = assetName(suffix);
    const payload = Buffer.from('Synthetic non-executable payload ' + name);
    const signed = sign(null, createHash('blake2b512').update(payload).digest(), key.privateKey);
    const comment = `timestamp:1\tfile:${name}\tversion:${version}`;
    const global = sign(null, Buffer.concat([signed, Buffer.from(comment)]), key.privateKey);
    const packet = Buffer.concat([Buffer.from('ED'), id, signed]);
    const signature = Buffer.from(encode(`untrusted comment: synthetic signature\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}`));
    for (const [asset, data] of [[name, payload], [name + '.sig', signature]]) {
      bytes.set(asset, data);
      assets.push({ id: assets.length + 1, name: asset, size: data.length, state: 'uploaded', updated_at: '2026-10-23T00:00:00Z', digest: 'sha256:' + sha256(data) });
    }
  }
  return { publicKey, bytes, release: { id: Number(releaseId), tag_name: 'v' + version, prerelease: true, draft: true, assets } };
}
const downloads = signedDownloads();
const identities = downloads.release.assets.map(({ name, id, size, updated_at }) => ({ name, id, size, updated_at, sha256: sha256(downloads.bytes.get(name)) }));

function buildInfo(role) {
  return { metadata_version: 1, component: role, component_version: version, source_revision: source,
    build_id: version + '@' + source, source_state: 'verified_checkout', revision_origin: 'explicit_checkout',
    target: 'x86_64-pc-windows-msvc', database_schema_version: 24, protocol_version: { major: 1, minor: 0 },
    linked_components: ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host', ...(role === 'service' ? ['dzc-service'] : [])]
      .map(component => ({ component, version })) };
}

function nativeFixture(scenario = 'LegacyToAlpha2') {
  const inputs = { source_revision: source, release_id: Number(releaseId), policy_sha256: policyHash, scenario,
    old: clone(oldPackages[scenario]), signed_assets: clone(identities), marker: 'nativeupgradefixturemarker' + '1'.repeat(32),
    candidate: clone(identities.find(item => item.name === assetName('x64-setup.exe'))),
    signature: clone(identities.find(item => item.name === assetName('x64-setup.exe.sig'))) };
  const marker = { id: 7, content: inputs.marker, content_type: 'text', is_pinned: 1, tags: JSON.stringify(['releaseupgradetag']) };
  const oldProfile = { status: 'passed', schema_version: scenario === 'PublishedAlpha2ToRelease' ? 24 : scenario === 'PublishedAlphaToAlpha2' ? 21 : 17,
    marker: clone(marker), frozen_preferences: clone(frozen) };
  const migrated = { ...clone(oldProfile), schema_version: 24 };
  const extracted = ['desktop', 'cli', 'service'].map((role, index) => ({ role,
    member: 'synthetic/' + { desktop: 'dezirclip.exe', cli: 'dzc.exe', service: 'dzc-service.exe' }[role],
    bytes: 100 + index, sha256: String(index + 1).repeat(64) }));
  const installed = extracted.map(({ role, bytes, sha256: hash }) => ({ role, bytes, sha256: hash }));
  const roles = Object.fromEntries(['desktop', 'cli', 'service'].map(role => [role, buildInfo(role)]));
  const settings = [
    ['Software\\Microsoft\\Clipboard', 'EnableClipboardHistory'], ['Software\\Microsoft\\Clipboard', 'EnableCloudClipboard'],
    ['Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced', 'DisabledHotkeys'],
    ['Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer', 'DisallowClipboardHistory'],
    ['Software\\Policies\\Microsoft\\Windows\\System', 'AllowClipboardHistory'], ['Software\\Policies\\Microsoft\\Windows\\System', 'AllowCrossDeviceClipboard'],
  ].map(([key, name]) => ({ key, name, kind: name === 'DisabledHotkeys' ? 'String' : 'DWord', value: name === 'DisabledHotkeys' ? 'XV' : 7, exists: true }));
  const explorer = [{ pid: 55, started_at: '2026-10-23T00:00:00Z' }];
  const labels = ['verify-staged', 'extract-packaged-roles', 'install-original-published', 'old-cli-version', 'old-cli-create-profile',
    'seed-owned-old-profile', 'install-final-release-upgrade', 'final-cli-build-info', 'final-service-build-info', 'final-cli-retained-history',
    'upgraded-profile-check', 'configure-final-webview', 'actual-final-gui', 'inspect-final-webview-token',
    'actual-final-gui-bridge', 'restore-final-webview', 'after-gui-profile-check', 'uninstall-actual-final-release',
    'uninstalled-profile-check', ...(inputs.old.source_revision ? ['old-cli-build-info'] : [])];
  const processes = labels.map((label, index) => ({ label, pid: 100 + index,
    original_handle_retained: true, exited: true, status: 'passed', exit_code: label === 'actual-final-gui' ? -1 : 0,
    ...(label === 'actual-final-gui' ? { terminated_original_tree: true } : {}) }));
  const evidence = {
    original_installer_hash: { path: 'old-setup.exe', sha256: inputs.old.sha256, expected: inputs.old.sha256 },
    candidate_signature_and_hash: { verifier: 'verify-staged', candidate: clone(inputs.candidate), signature: clone(inputs.signature) },
    actual_old_install: { installer_sha256: inputs.old.sha256, cli_version: 'dzc ' + inputs.old.version },
    actual_old_profile_created: { actual: clone(oldProfile), backup_sha256: 'c'.repeat(64) },
    actual_upgrade_same_directory: { old_directory: 'C:\\synthetic-owned\\installed', final_directory: 'C:\\synthetic-owned\\installed',
      installer_sha256: inputs.candidate.sha256, arguments: ['/S', '/UPDATE', '/D=C:\\synthetic-owned\\installed'] },
    installed_three_role_bytes: clone(installed), actual_three_role_build_info: clone(roles),
    history_and_schema24: { before: clone(marker), after: clone(migrated), actual_gui_evidence: 'final-gui-evidence.json' },
    frozen_preferences_preserved: { before: clone(frozen), after: clone(frozen) },
    windows_settings_preserved: { before: clone(settings), after: clone(settings) },
    explorer_not_restarted: { before: clone(explorer), after_upgrade: clone(explorer), after_uninstall: clone(explorer), had_existing_explorer: true },
    controlled_processes_exited: { count: processes.length, original_handles_verified: true },
    actual_release_uninstall: { uninstaller_sha256: 'd'.repeat(64), registered_installation_absent: true,
      missing: ['dezirclip.exe', 'dzc.exe', 'dzc-service.exe'] },
    uninstall_preserves_profile_and_settings: { profile: clone(migrated), windows_settings: clone(settings) },
  };
  const report = { schema_version: 1, kind: 'release_windows_upgrade_native', status: 'passed', failure: null,
    version, source_revision: source, release_id: Number(releaseId), policy_sha256: policyHash, scenario,
    old: clone(inputs.old), signed_assets: clone(inputs.signed_assets), roles, packaged: clone(extracted), installed,
    old_profile: clone(oldProfile), old_profile_backup_sha256: 'c'.repeat(64), uninstaller_sha256: 'd'.repeat(64), processes,
    checks: checkNames.map(name => ({ name, status: 'passed', evidence: evidence[name] })),
    gui_process_identity: { verified: true }, gui_exit: { original_process_exited: true, cdp_port_released: true }, cleanup: [] };
  return { inputs, report, extracted };
}
const evidence = (report, name) => report.checks.find(item => item.name === name).evidence;
const verify = fixture => validateNative(fixture.report, fixture.inputs, fixture.extracted);
function rejectsNative(change) {
  const fixture = nativeFixture();
  change(fixture);
  assert.throws(() => verify(fixture));
}

test('synthetic authority permits only exact final-tag hosted checkout and actual x64 Windows', () => {
  const env = { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_REF: 'refs/tags/v' + version, GITHUB_SHA: source, PROCESSOR_ARCHITECTURE: 'AMD64' };
  assert.doesNotThrow(() => assertAuthority(env, source, releaseId, 'win32', 'x64', true));
  assert.doesNotThrow(() => assertAuthority(env, source, releaseId, 'linux', 'x64'));
  for (const [key, value] of Object.entries({ GITHUB_ACTIONS: 'false', RUNNER_ENVIRONMENT: 'self-hosted', GITHUB_REPOSITORY: 'other/repo',
    GITHUB_REF: 'refs/heads/master', GITHUB_SHA: 'f'.repeat(40), PROCESSOR_ARCHITECTURE: 'ARM64', PROCESSOR_ARCHITEW6432: 'AMD64' })) {
    assert.throws(() => assertAuthority({ ...env, [key]: value }, source, releaseId, 'win32', 'x64', true));
  }
  for (const [platform, arch] of [['linux', 'x64'], ['win32', 'arm64'], ['win32', 'ia32']]) assert.throws(() => assertAuthority(env, source, releaseId, platform, arch, true));
  for (const id of ['0', '-1', '01', '42x']) assert.throws(() => assertAuthority(env, source, id, 'win32', 'x64', true));
});

test('an explicit zero recovery flag retains the normal exact-tag authority contract', () => {
  const env = { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_REF: 'refs/tags/v' + version, GITHUB_SHA: source, DZC_RELEASE_RECOVERY: '0',
    DZC_RELEASE_TOOLING_SOURCE: 'f'.repeat(40), PROCESSOR_ARCHITECTURE: 'AMD64' };
  assert.doesNotThrow(() => assertAuthority(env, source, releaseId, 'win32', 'x64', true));
  assert.throws(() => assertAuthority({ ...env, GITHUB_REF: 'refs/heads/master' }, source, releaseId, 'win32', 'x64', true));
  assert.throws(() => assertAuthority({ ...env, GITHUB_SHA: 'f'.repeat(40) }, source, releaseId, 'win32', 'x64', true));
  assert.throws(() => assertAuthority({ ...env, DZC_RELEASE_RECOVERY: '2' }, source, releaseId, 'win32', 'x64', true));
});

const recoverySource = '037f40cf014cd4bb3dce4a0a0c5cbcaee23affae';
const recoveryReleaseId = '422045565';
const workflowSource = 'f'.repeat(40);
function recoveryEnvironment() {
  return { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted', GITHUB_REPOSITORY: 'qiyuanhuakai/dezirclip',
    GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/master', GITHUB_SHA: workflowSource,
    GITHUB_WORKFLOW_REF: 'qiyuanhuakai/dezirclip/.github/workflows/release.yml@refs/heads/master',
    GITHUB_RUN_ID: '123456', GITHUB_RUN_ATTEMPT: '2', DZC_RELEASE_RECOVERY: '1',
    DZC_RELEASE_TOOLING_SOURCE: workflowSource, PROCESSOR_ARCHITECTURE: 'AMD64' };
}
function recoveryReceipt() {
  return { schema_version: 1, mode: 'recovery', source_revision: recoverySource, workflow_source_revision: workflowSource,
    tag: 'v' + version, release_id: Number(recoveryReleaseId), run_id: '123456', run_attempt: '2' };
}
test('pure recovery context validation retains actual dispatch fields without executing its independent authority', () => {
  const env = recoveryEnvironment();
  if (version !== '0.2.0-alpha.2') {
    assert.throws(() => assertAuthority(env, recoverySource, recoveryReleaseId, 'win32', 'x64', true), /original alpha.2/);
    assert.throws(() => validateRecoveryAuthority(recoveryReceipt(), env, recoverySource, recoveryReleaseId), /original alpha.2/);
    return;
  }
  assert.doesNotThrow(() => assertAuthority(env, recoverySource, recoveryReleaseId, 'win32', 'x64', true));
  assert.equal(validateRecoveryAuthority(recoveryReceipt(), env, recoverySource, recoveryReleaseId).workflow_source_revision, workflowSource);
  for (const [key, value] of Object.entries({ GITHUB_ACTIONS: 'false', RUNNER_ENVIRONMENT: 'self-hosted',
    GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/tags/v' + version, GITHUB_SHA: source,
    GITHUB_WORKFLOW_REF: 'qiyuanhuakai/dezirclip/.github/workflows/release.yml@refs/heads/other',
    GITHUB_RUN_ID: '0', GITHUB_RUN_ATTEMPT: '0', DZC_RELEASE_RECOVERY: '0', DZC_RELEASE_TOOLING_SOURCE: 'invalid' })) {
    assert.throws(() => assertAuthority({ ...env, [key]: value }, recoverySource, recoveryReleaseId, 'win32', 'x64', true));
  }
  for (const key of ['GITHUB_EVENT_NAME', 'GITHUB_WORKFLOW_REF', 'DZC_RELEASE_TOOLING_SOURCE', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT']) {
    const missing = { ...env }; delete missing[key];
    assert.throws(() => assertAuthority(missing, recoverySource, recoveryReleaseId, 'win32', 'x64', true));
  }
  assert.throws(() => assertAuthority({ ...env, PROCESSOR_ARCHITECTURE: 'ARM64' }, recoverySource, recoveryReleaseId, 'win32', 'x64', true));
  for (const [key, value] of Object.entries({ schema_version: 2, mode: 'tag', source_revision: source,
    workflow_source_revision: source, tag: 'v0.2.0-alpha.1', release_id: 42, run_id: '999', run_attempt: '1' })) {
    assert.throws(() => validateRecoveryAuthority({ ...recoveryReceipt(), [key]: value }, env, recoverySource, recoveryReleaseId));
  }
  assert.throws(() => validateRecoveryAuthority(recoveryReceipt(), {}, recoverySource, recoveryReleaseId));
});

test('HTTP failure details retain permission diagnostics and exclude credentials, redirects and arbitrary body fields', () => {
  const token = 'private-read-token-for-tests';
  const headers = new Headers({ 'X-GitHub-Request-Id': 'request-123', 'X-Accepted-GitHub-Permissions': 'contents=write',
    'X-RateLimit-Remaining': '4999', 'Content-Type': 'application/json', Authorization: 'Bearer ' + token,
    Cookie: token, 'Set-Cookie': token, Location: 'https://objects.example/payload?sig=secretSig' });
  const value = httpFailureDetails(403, headers, { message: 'Resource not accessible ' + token + ' ghp_syntheticToken123 https://objects.example/payload?sig=secretSig',
    status: '403', documentation_url: 'https://docs.github.com/rest/releases/releases', debug: { Authorization: token } }, token);
  assert.deepEqual(value.headers, { 'x-github-request-id': 'request-123', 'x-accepted-github-permissions': 'contents=write',
    'x-ratelimit-remaining': '4999', 'content-type': 'application/json' });
  assert.equal(value.status, 403);
  assert.equal(value.response_body.documentation_url, 'https://docs.github.com/rest/releases/releases');
  assert.deepEqual(Object.keys(value.response_body).sort(), ['documentation_url', 'message', 'status']);
  assert.doesNotMatch(JSON.stringify(value), /private-read-token|ghp_|secretSig|Authorization|Cookie|Location/);
  for (const url of ['https://docs.github.com/rest?token=secret', 'https://docs.github.com/rest#secret',
    'https://user:password@docs.github.com/rest', 'https://other.example/rest', 'http://docs.github.com/rest',
    'https://docs.github.com/' + token]) {
    assert.equal(httpFailureDetails(403, {}, { documentation_url: url }, token).response_body.documentation_url, undefined);
  }
  assert.equal(httpFailureDetails(403, {}, { message: 'x'.repeat(9000) }).response_body.message.length, 2048);
  assert.throws(() => httpFailureDetails(0, {}, {}));
});

test('a pre-policy failure remains failed with a bounded stage and independently bound recovery workflow source', () => {
  const token = 'private-read-token-for-tests';
  const error = new Error('Metadata denied ' + token + ' https://objects.example/payload?sig=secretSig');
  error.http = { ...httpFailureDetails(403, { 'x-accepted-github-permissions': 'contents=write' }, { message: 'denied ' + token }, token),
    authorization: token, body_truncated: false, body_unavailable: false };
  const value = prepareFailure(recoverySource, recoveryReleaseId, 'release_metadata', error, recoveryReceipt(), token);
  assert.equal(value.kind, 'release_windows_upgrade_prepare_failure');
  assert.equal(value.status, 'failed');
  assert.equal(value.source_revision, recoverySource);
  assert.equal(value.workflow_source_revision, workflowSource);
  assert.equal(value.release_id, Number(recoveryReleaseId));
  assert.equal(value.failure.http.status, 403);
  assert.equal(value.failure.http.headers['x-accepted-github-permissions'], 'contents=write');
  assert.equal(Object.hasOwn(value, 'signed_assets'), false);
  assert.equal(Object.hasOwn(value, 'policy'), false);
  assert.equal(Object.hasOwn(value, 'checks'), false);
  assert.doesNotMatch(JSON.stringify(value), /private-read-token|secretSig|authorization/);
  assert.equal(Object.hasOwn(prepareFailure(source, releaseId, 'asset_inventory', new Error('missing'), { mode: 'tag' }), 'workflow_source_revision'), false);
  assert.throws(() => prepareFailure(source, releaseId, 'passed', error, { mode: 'tag' }));
  assert.throws(() => prepareFailure(source, releaseId, 'download_asset', error, { mode: 'tag' }, token, 'https://objects.example/payload?sig=secretSig'));
  assert.equal(safeFailureText('\u0000' + token + ' ghp_syntheticToken123 https://example.test/?secret=1', token), '[redacted] [redacted] [url]');
});

test('synthetic release metadata requires all unique four updater payloads and four signatures', () => {
  assert.deepEqual(releaseAssets(downloads.release, releaseId).map(item => item.name), updaterNames);
  for (const change of [value => value.assets.pop(), value => value.assets.push(clone(value.assets[0])),
    value => { value.tag_name = 'v0.2.0-alpha.1'; }, value => { value.id++; }, value => { value.prerelease = false; }, value => { value.draft = false; },
    value => { value.assets[0].state = 'starter'; },
    value => { value.assets[0].id = 0; }, value => { value.assets[0].size = 0; }, value => { value.assets[1].size = 16385; }]) {
    const value = clone(downloads.release); change(value); assert.throws(() => releaseAssets(value, releaseId));
  }
});

test('in-memory Ed25519 downloads verify signatures, exact sizes, server digests and public key', () => {
  assert.deepEqual(verifyDownloads(downloads.release, releaseId, downloads.bytes, downloads.publicKey), identities);
  const changed = new Map(downloads.bytes); changed.set(assetName('x64-setup.exe'), Buffer.from('different payload'));
  assert.throws(() => verifyDownloads(downloads.release, releaseId, changed, downloads.publicKey));
  const sameSize = new Map(downloads.bytes); const payload = Buffer.from(sameSize.get(assetName('x64-setup.exe'))); payload[0] ^= 1;
  sameSize.set(assetName('x64-setup.exe'), payload);
  const noDigest = clone(downloads.release); for (const asset of noDigest.assets) delete asset.digest;
  assert.throws(() => verifyDownloads(noDigest, releaseId, sameSize, downloads.publicKey));
  const absent = new Map(downloads.bytes); absent.delete(assetName('x64-setup.exe.sig'));
  assert.throws(() => verifyDownloads(downloads.release, releaseId, absent, downloads.publicKey));
  const serverMismatch = clone(downloads.release); serverMismatch.assets[0].digest = 'sha256:' + 'f'.repeat(64);
  assert.throws(() => verifyDownloads(serverMismatch, releaseId, downloads.bytes, downloads.publicKey));
  assert.throws(() => verifyDownloads(downloads.release, releaseId, downloads.bytes, signedDownloads().publicKey));
});

test('synthetic asset receipts reject duplicate IDs, names, missing bytes and invalid hashes', () => {
  assert.doesNotThrow(() => validateAssets(identities));
  for (const change of [value => value.pop(), value => value.push(clone(value[0])), value => { value[1].id = value[0].id; },
    value => { value[1].name = value[0].name; }, value => { value[0].sha256 = 'x'.repeat(64); }, value => { value[0].size = 0; },
    value => { value[0].updated_at = 'invalid'; }]) {
    const value = clone(identities); change(value); assert.throws(() => validateAssets(value));
  }
});

test('synthetic NSIS listing requires three safe unique role members and rejects links or folders', () => {
  const listing = names => names.map(name => `Path = ${name}\nSize = 100\nFolder = -`).join('\n\n');
  const names = ['app/dezirclip.exe', 'app/dzc.exe', 'app/dzc-service.exe'];
  assert.deepEqual(nsisMembers(listing(names)), names);
  for (const invalid of [names.slice(1), [...names, 'duplicate/dzc.exe'], ['../dezirclip.exe', ...names.slice(1)],
    ['/dezirclip.exe', ...names.slice(1)], ['C:/dezirclip.exe', ...names.slice(1)], ['-dezirclip.exe', ...names.slice(1)]]) {
    assert.throws(() => nsisMembers(listing(invalid)));
  }
  for (const extra of ['Symbolic Link = target', 'Hard Link = target', 'Folder = +']) {
    assert.throws(() => nsisMembers(listing(names) + '\n' + extra));
  }
});

test('synthetic PE headers reject wrong machine, bitness and bounded-header escapes without execution', () => {
  const bytes = Buffer.alloc(160); bytes.write('MZ'); bytes.writeUInt32LE(64, 60); bytes.write('PE\0\0', 64);
  bytes.writeUInt16LE(0x8664, 68); bytes.writeUInt16LE(0x20b, 88);
  assert.doesNotThrow(() => inspectPe(bytes));
  for (const change of [value => value.write('NO'), value => value.writeUInt32LE(1000, 60),
    value => value.writeUInt16LE(0xaa64, 68), value => value.writeUInt16LE(0x10b, 88)]) {
    const value = Buffer.from(bytes); change(value); assert.throws(() => inspectPe(value));
  }
  assert.throws(() => inspectPe(Buffer.alloc(20)));
});

test('synthetic BuildInfo distinguishes desktop/CLI four and Service five linked components', () => {
  for (const role of ['desktop', 'cli', 'service']) assert.doesNotThrow(() => validateInfo(buildInfo(role), role, source));
  for (const change of [value => { value.source_revision = 'f'.repeat(40); }, value => { value.component_version = '0.2.0-alpha.1'; },
    value => { value.database_schema_version = 21; }, value => { value.target = 'aarch64-pc-windows-msvc'; },
    value => { value.protocol_version.minor = 1; }, value => { value.source_state = 'unknown'; },
    value => { value.linked_components.pop(); }, value => { value.linked_components[1] = clone(value.linked_components[0]); }]) {
    const value = buildInfo('service'); change(value); assert.throws(() => validateInfo(value, 'service', source));
  }
  assert.throws(() => validateInfo(buildInfo('cli'), 'desktop', source));
});

test('synthetic raw observations accept all three historical scenarios only as pure validation data', () => {
  for (const scenario of Object.keys(oldPackages)) assert.doesNotThrow(() => verify(nativeFixture(scenario)));
});

test('synthetic raw observations require exactly fourteen distinct passed checks and evidence', () => {
  for (const change of [value => value.report.checks.pop(), value => value.report.checks.push(clone(value.report.checks[0])),
    value => { value.report.checks[1] = clone(value.report.checks[0]); }, value => { value.report.checks[0].status = 'not_run'; },
    value => { value.report.checks[0].evidence = null; }]) rejectsNative(change);
});

test('synthetic raw observations reject source, policy, assets, original installer or candidate substitutions', () => {
  for (const change of [value => { value.report.source_revision = 'f'.repeat(40); }, value => { value.report.policy_sha256 = 'f'.repeat(64); },
    value => { value.report.release_id++; }, value => { value.report.old.sha256 = 'f'.repeat(64); },
    value => { value.report.signed_assets[0].sha256 = 'f'.repeat(64); },
    value => { evidence(value.report, 'original_installer_hash').sha256 = 'f'.repeat(64); },
    value => { evidence(value.report, 'actual_old_install').cli_version = 'dzc 99.0.0'; },
    value => { evidence(value.report, 'candidate_signature_and_hash').candidate.sha256 = 'f'.repeat(64); },
    value => { evidence(value.report, 'actual_upgrade_same_directory').final_directory += '-other'; }]) rejectsNative(change);
});

test('synthetic raw observations reject changed installed bytes and role inventories', () => {
  for (const change of [value => { value.report.installed[0].sha256 = 'f'.repeat(64); }, value => { value.report.installed[0].bytes++; },
    value => { value.report.packaged[0].sha256 = 'f'.repeat(64); }, value => { value.report.installed[1].role = 'desktop'; },
    value => { value.report.roles.service = buildInfo('cli'); }, value => { value.report.roles.cli.source_revision = 'f'.repeat(40); }]) rejectsNative(change);
});

function eachOriginal(report, change) {
  const history = evidence(report, 'history_and_schema24');
  for (const marker of [report.old_profile.marker, evidence(report, 'actual_old_profile_created').actual.marker,
    history.before, history.after.marker, evidence(report, 'uninstall_preserves_profile_and_settings').profile.marker]) change(marker);
}
test('synthetic raw observations reject an unpinned or retagged original even if every copy agrees', () => {
  for (const change of [marker => { marker.is_pinned = 0; }, marker => { marker.tags = '[]'; },
    marker => { marker.content += 'changed'; }, marker => { marker.id = -1; }]) rejectsNative(value => eachOriginal(value.report, change));
});

test('synthetic raw observations require all five frozen keys and preserve exact values through uninstall', () => {
  rejectsNative(({ report }) => {
    for (const profile of [report.old_profile, evidence(report, 'actual_old_profile_created').actual,
      evidence(report, 'history_and_schema24').after, evidence(report, 'uninstall_preserves_profile_and_settings').profile]) delete profile.frozen_preferences['app.ocr_enabled'];
    for (const pair of ['before', 'after']) delete evidence(report, 'frozen_preferences_preserved')[pair]['app.ocr_enabled'];
  });
  rejectsNative(({ report }) => { evidence(report, 'history_and_schema24').after.frozen_preferences['app.ocr_enabled'] = 'true'; });
  rejectsNative(({ report }) => { evidence(report, 'uninstall_preserves_profile_and_settings').profile.frozen_preferences['app.quick_paste_hotkey'] = 'Ctrl+F1'; });
  rejectsNative(({ report }) => { evidence(report, 'history_and_schema24').after.schema_version = 23; });
});

test('synthetic raw observations reject unexited or substituted process handles, GUI and cleanup failures', () => {
  for (const change of [value => { value.report.processes[0].exited = false; }, value => { value.report.processes[0].original_handle_retained = false; },
    value => { value.report.processes[0].pid = 0; }, value => { value.report.processes[1].label = value.report.processes[0].label; },
    value => { value.report.processes[0].exit_code = 3; }, value => { value.report.processes[0].label = 'different-unique-process'; },
    value => { value.report.processes.find(item => item.label === 'actual-final-gui').terminated_original_tree = false; },
    value => { evidence(value.report, 'controlled_processes_exited').count--; }, value => { value.report.gui_exit.cdp_port_released = false; },
    value => { value.report.gui_process_identity.verified = false; }, value => value.report.cleanup.push({ error: 'synthetic failure' }),
    value => { evidence(value.report, 'actual_release_uninstall').registered_installation_absent = false; },
    value => { evidence(value.report, 'windows_settings_preserved').after[0].value++; },
    value => { evidence(value.report, 'explorer_not_restarted').after_upgrade[0].pid++; }]) rejectsNative(change);
});

test('Explorer preservation requires an active original identity, not three matching empty snapshots', () => {
  rejectsNative(({ report }) => {
    const explorer = evidence(report, 'explorer_not_restarted');
    explorer.before = []; explorer.after_upgrade = []; explorer.after_uninstall = [];
  });
  rejectsNative(({ report }) => { evidence(report, 'explorer_not_restarted').had_existing_explorer = false; });
  rejectsNative(({ report }) => { delete evidence(report, 'explorer_not_restarted').had_existing_explorer; });
  for (const invalid of [{ pid: 0 }, { started_at: null }, { started_at: 'invalid' }]) rejectsNative(({ report }) => {
    const explorer = evidence(report, 'explorer_not_restarted');
    for (const stage of ['before', 'after_upgrade', 'after_uninstall']) Object.assign(explorer[stage][0], invalid);
  });
});

test('synthetic native gate requires normal configuration, token inspection and restoration exits', () => {
  for (const label of ['configure-final-webview', 'inspect-final-webview-token', 'restore-final-webview']) {
    rejectsNative(({ report }) => {
      report.processes = report.processes.filter(item => item.label !== label);
      evidence(report, 'controlled_processes_exited').count = report.processes.length;
    });
    rejectsNative(({ report }) => { report.processes.find(item => item.label === label).exit_code = 1; });
  }
});

test('synthetic GUI exit accepts natural success or confirmed original-tree termination only', () => {
  for (const terminated of [false, true]) {
    const fixture = nativeFixture();
    const gui = fixture.report.processes.find(item => item.label === 'actual-final-gui');
    gui.exit_code = 0;
    gui.terminated_original_tree = terminated;
    assert.doesNotThrow(() => verify(fixture));
  }
  assert.doesNotThrow(() => verify(nativeFixture()));
  rejectsNative(({ report }) => { report.processes.find(item => item.label === 'actual-final-gui').terminated_original_tree = false; });
  rejectsNative(({ report }) => { delete report.processes.find(item => item.label === 'actual-final-gui').terminated_original_tree; });
});

function sealedScenario(scenario) {
  const { report } = nativeFixture(scenario);
  const value = { ...report, kind: 'release_windows_upgrade_report', raw_native_sha256: 'e'.repeat(64) };
  return { value, sha256: sha256(Buffer.from(JSON.stringify(value, null, 2) + '\n')) };
}
test('synthetic aggregation binds all three distinct scenarios, source, release, policy and original report-byte hashes', () => {
  const reports = Object.keys(oldPackages).map(sealedScenario);
  const combined = combine(reports, source, releaseId, policyHash);
  assert.deepEqual(combined.checks.map(item => item.report_sha256), reports.map(item => item.sha256));
  assert.deepEqual(combined.signed_assets, identities);
  assert.equal(combined.checks.length, 3);
  for (const change of [value => value.pop(), value => { value[1].value.scenario = value[0].value.scenario; },
    value => { value[1].value.source_revision = 'f'.repeat(40); }, value => { value[1].value.release_id++; },
    value => { value[1].value.policy_sha256 = 'f'.repeat(64); }, value => { value[1].sha256 = 'invalid'; },
    value => { value[1].value.signed_assets[0].sha256 = 'f'.repeat(64); }, value => { value[1].value.status = 'failed'; }]) {
    const value = clone(reports); change(value); assert.throws(() => combine(value, source, releaseId, policyHash));
  }
});

test('candidate version comes from the canonical package and published historical bytes stay pinned', () => {
  const packageVersion = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8')).version;
  assert.equal(version, packageVersion);
  assert.deepEqual(Object.keys(oldPackages), ['LegacyToAlpha2', 'PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease']);
  assert.equal(oldPackages.LegacyToAlpha2.version, '0.1.20');
  assert.equal(oldPackages.PublishedAlphaToAlpha2.version, '0.2.0-alpha.1');
  assert.deepEqual(oldPackages.PublishedAlpha2ToRelease, { version: '0.2.0-alpha.2',
    sha256: '9e96fcd49e4e36c903edd19a1424856383e229ea131f455fbabdcf816622db05',
    source_revision: '037f40cf014cd4bb3dce4a0a0c5cbcaee23affae' });
});

function historicalInfo(scenario) {
  const old = oldPackages[scenario];
  return { metadata_version: 1, component: 'cli', component_version: old.version, source_revision: old.source_revision,
    build_id: old.version + '@' + old.source_revision, source_state: 'verified_checkout', revision_origin: 'explicit_checkout',
    database_schema_version: scenario === 'PublishedAlpha2ToRelease' ? 24 : 21,
    target: 'x86_64-pc-windows-msvc', protocol_version: { major: 1, minor: 0 },
    linked_components: ['dzc-core', 'dzc-platform', 'dzc-protocol', ...(scenario === 'PublishedAlpha2ToRelease' ? ['dzc-host'] : [])]
      .map(component => ({ component, version: old.version })) };
}
test('historical alpha1 and alpha2 BuildInfo retain their distinct pinned sources, schemas and linked inventory', () => {
  for (const scenario of ['PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease']) {
    assert.doesNotThrow(() => validateHistoricalInfo(historicalInfo(scenario), scenario));
    for (const change of [info => { info.source_revision = source; }, info => { info.build_id = version + '@' + source; },
      info => { info.component_version = version; }, info => { info.database_schema_version = 23; },
      info => { info.linked_components.pop(); }, info => { info.linked_components[0].version = version; }]) {
      const info = historicalInfo(scenario); change(info);
      assert.throws(() => validateHistoricalInfo(info, scenario));
    }
  }
  for (const change of [info => { info.metadata_version = 2; }, info => { info.revision_origin = 'unknown'; },
    info => { info.protocol_version.major = 2; }, info => { info.protocol_version.minor = 1; }]) {
    const info = historicalInfo('PublishedAlpha2ToRelease'); change(info);
    assert.throws(() => validateHistoricalInfo(info, 'PublishedAlpha2ToRelease'));
  }
  assert.throws(() => validateHistoricalInfo(historicalInfo('PublishedAlphaToAlpha2'), 'LegacyToAlpha2'));
});

test('historical alpha2 schema24 admission never widens legacy or alpha1 schema guards', () => {
  for (const [scenario, rejected] of [['LegacyToAlpha2', [0, 24]], ['PublishedAlphaToAlpha2', [17, 24]],
    ['PublishedAlpha2ToRelease', [17, 21, 23, 25]]]) {
    for (const schema of rejected) {
      const fixture = nativeFixture(scenario);
      fixture.report.old_profile.schema_version = schema;
      evidence(fixture.report, 'actual_old_profile_created').actual.schema_version = schema;
      assert.throws(() => verify(fixture));
    }
  }
});

test('third upgrade scenario cannot omit its real old BuildInfo process or diverge from shared signed assets', () => {
  const fixture = nativeFixture('PublishedAlpha2ToRelease');
  fixture.report.processes = fixture.report.processes.filter(item => item.label !== 'old-cli-build-info');
  evidence(fixture.report, 'controlled_processes_exited').count--;
  assert.throws(() => verify(fixture));
  for (const change of [value => { value[2].value.signed_assets[0].sha256 = 'f'.repeat(64); },
    value => { value[2].value.workflow_source_revision = 'f'.repeat(40); },
    value => { value[2].value.old.sha256 = 'f'.repeat(64); }]) {
    const reports = Object.keys(oldPackages).map(sealedScenario); change(reports);
    assert.throws(() => combine(reports, source, releaseId, policyHash));
  }
});

test('native and aggregate evidence retain one recovery workflow source while normal reports stay compatible', () => {
  const fixture = nativeFixture();
  fixture.inputs.workflow_source_revision = workflowSource;
  fixture.report.workflow_source_revision = workflowSource;
  assert.doesNotThrow(() => verify(fixture));
  fixture.report.workflow_source_revision = source;
  assert.throws(() => verify(fixture));
  fixture.report.workflow_source_revision = workflowSource;
  delete fixture.inputs.workflow_source_revision;
  assert.throws(() => verify(fixture));
  const reports = Object.keys(oldPackages).map(sealedScenario);
  assert.equal(Object.hasOwn(combine(reports, source, releaseId, policyHash), 'workflow_source_revision'), false);
  for (const item of reports) {
    item.value.workflow_source_revision = workflowSource;
    item.sha256 = sha256(Buffer.from(JSON.stringify(item.value)));
  }
  assert.equal(combine(reports, source, releaseId, policyHash).workflow_source_revision, workflowSource);
  reports[1].value.workflow_source_revision = source;
  assert.throws(() => combine(reports, source, releaseId, policyHash));
  delete reports[1].value.workflow_source_revision;
  assert.throws(() => combine(reports, source, releaseId, policyHash));
  reports[1].value.workflow_source_revision = 'invalid';
  assert.throws(() => combine(reports, source, releaseId, policyHash));
});

test('Windows local entry point refuses before any product operation', { skip: process.platform !== 'win32' }, () => {
  const script = fileURLToPath(new URL('../test-release-windows-upgrade.ps1', import.meta.url));
  const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-File', script,
    '-FixtureDirectory', 'unused-local-release-refusal-fixture', '-ExpectedSource', source,
    '-ExpectedReleaseId', '1', '-ExpectedPolicySha256', 'a'.repeat(64)],
  { encoding: 'utf8', timeout: 20000, maxBuffer: 65536,
    env: { ...process.env, GITHUB_ACTIONS: 'false', RUNNER_ENVIRONMENT: 'local' } });
  assert.equal(result.error, undefined);
  assert.notEqual(result.status, 0);
  assert.match(result.stdout + result.stderr, /Final release upgrade execution requires|GitHub-hosted runner/);
});

test('PowerShell application discovery selects one PATH-first file without running fixtures', { skip: process.platform !== 'win32' }, () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'dzc-release-application-path-'));
  const first = path.join(directory, 'actions setup first');
  const second = path.join(directory, 'global tools second');
  const names = ['node.exe', 'python.exe', 'pwsh.exe'];
  try {
    mkdirSync(first); mkdirSync(second);
    for (const parent of [first, second]) for (const name of names) writeFileSync(path.join(parent, name), '', { flag: 'wx' });
    const program = String.raw`
$ErrorActionPreference = 'Stop'
$PSStyle.OutputRendering = 'PlainText'
$tokens = $null; $errors = $null
$tree = [System.Management.Automation.Language.Parser]::ParseFile($env:DZC_DISCOVERY_SOURCE, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw 'The release script must remain syntactically valid.' }
$functions = @($tree.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Resolve-ApplicationPath' }, $true))
if ($functions.Count -ne 1) { throw 'Expected exactly one isolated application resolver.' }
. ([scriptblock]::Create($functions[0].Extent.Text))
$originalPath = $env:PATH
try {
    $observed = @()
    foreach ($name in @('node.exe', 'python.exe', 'pwsh.exe')) {
        $env:PATH = $env:DZC_DISCOVERY_FIRST + ';' + $env:DZC_DISCOVERY_SECOND
        if (@(Get-Command -Name $name -CommandType Application -All -ErrorAction Stop).Count -lt 2) { throw 'Both discovery candidates must exist.' }
        $forward = @(Resolve-ApplicationPath $name)
        $env:PATH = $env:DZC_DISCOVERY_SECOND + ';' + $env:DZC_DISCOVERY_FIRST
        $reverse = @(Resolve-ApplicationPath $name)
        if ($forward.Count -ne 1 -or $reverse.Count -ne 1) { throw 'Application resolution must return one scalar path.' }
        $observed += @{ name = $name; forward = $forward[0]; reverse = $reverse[0] }
    }
    $missingRejected = $false
    try { Resolve-ApplicationPath 'dzc-missing-discovery-fixture.exe' | Out-Null } catch { $missingRejected = $true }
    $emptyRejected = $false
    try { Resolve-ApplicationPath '' | Out-Null } catch { $emptyRejected = $true }
    ConvertTo-Json -InputObject @{ observed = $observed; missing_rejected = $missingRejected; empty_rejected = $emptyRejected } -Depth 6 -Compress
} finally { $env:PATH = $originalPath }
`;
    const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', program], {
      encoding: 'utf8', timeout: 20000, maxBuffer: 65536,
      env: { ...process.env, DZC_DISCOVERY_SOURCE: fileURLToPath(new URL('../test-release-windows-upgrade.ps1', import.meta.url)),
        DZC_DISCOVERY_FIRST: first, DZC_DISCOVERY_SECOND: second },
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const observed = JSON.parse(result.stdout);
    assert.equal(observed.missing_rejected, true);
    assert.equal(observed.empty_rejected, true);
    assert.deepEqual(observed.observed.map(item => item.name), names);
    for (const item of observed.observed) {
      assert.equal(path.resolve(item.forward).toLowerCase(), path.join(first, item.name).toLowerCase());
      assert.equal(path.resolve(item.reverse).toLowerCase(), path.join(second, item.name).toLowerCase());
    }
  } finally {
    for (const parent of [first, second]) {
      for (const name of names) if (existsSync(path.join(parent, name))) unlinkSync(path.join(parent, name));
      if (existsSync(parent)) rmdirSync(parent);
    }
    rmdirSync(directory);
  }
});
