import { createHash, randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { makeManifest, verifyTauriSignature } from './updater-release.mjs';

export const version = '0.2.0-alpha.2';
const repository = 'qiyuanhuakai/dezirclip';
const filename = 'DezirClip_' + version + '_x64-setup.exe';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const maximum = 512 * 1024 * 1024;
export const oldPackages = Object.freeze({
  LegacyToAlpha2: { version: '0.1.20', sha256: 'd20033124c74bea451301716afb46623607602bae37647748ae7d25ab7dd4b8e' },
  PublishedAlphaToAlpha2: { version: '0.2.0-alpha.1', sha256: '31d8b88064e1185c9f74b5fdd327902b9bc860e523b9a5ac0d71620ec7adbb87',
    source_revision: '90ba9960744b6cd57caaaf597f68533a7e41be45' },
});
export const updaterNames = ['x64-setup.exe', 'arm64-setup.exe', 'amd64.AppImage', 'aarch64.AppImage']
  .flatMap(suffix => ['DezirClip_' + version + '_' + suffix, 'DezirClip_' + version + '_' + suffix + '.sig']);
const requireValue = (value, message) => { if (!value) throw new Error(message); };
export const checkNames = Object.freeze(['original_installer_hash', 'candidate_signature_and_hash', 'actual_old_install',
  'actual_old_profile_created', 'actual_upgrade_same_directory', 'installed_three_role_bytes', 'actual_three_role_build_info',
  'history_and_schema24', 'frozen_preferences_preserved', 'windows_settings_preserved', 'explorer_not_restarted',
  'controlled_processes_exited', 'actual_release_uninstall', 'uninstall_preserves_profile_and_settings']);
const roleFiles = Object.freeze({ desktop: 'dezirclip.exe', cli: 'dzc.exe', service: 'dzc-service.exe' });
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function json(path) { return JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, '')); }
function save(path, value) { writeFileSync(path, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' }); }
function regular(path, limit = maximum) {
  const stat = lstatSync(path);
  requireValue(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= limit, 'Expected a bounded regular owned file');
  return readFileSync(path);
}
function owned(path) {
  const boundary = realpathSync(process.env.RUNNER_TEMP);
  const actual = resolve(path);
  const child = relative(boundary, actual);
  requireValue(child && child !== '..' && !child.startsWith('..' + sep) && !isAbsolute(child), 'Path must stay beneath RUNNER_TEMP');
  for (let current = actual; ; current = dirname(current)) {
    if (existsSync(current)) requireValue(!lstatSync(current).isSymbolicLink(), 'Owned path crosses a link');
    if (current.toLowerCase() === boundary.toLowerCase()) break;
  }
  return actual;
}
export function assertAuthority(env, source, releaseId, platform, arch, native = false) {
  requireValue(env.GITHUB_ACTIONS === 'true' && env.RUNNER_ENVIRONMENT === 'github-hosted'
    && env.GITHUB_REPOSITORY === repository && env.GITHUB_REF === 'refs/tags/v' + version, 'Only the final tag on a disposable GitHub runner is allowed');
  requireValue(/^[a-f0-9]{40}$/.test(source) && env.GITHUB_SHA === source && /^[1-9]\d*$/.test(releaseId), 'Final source/release identity is invalid');
  if (native) requireValue(platform === 'win32' && arch === 'x64'
    && !env.PROCESSOR_ARCHITEW6432 && env.PROCESSOR_ARCHITECTURE?.toUpperCase() === 'AMD64', 'Native x64 Windows is required');
}
function authority(source, releaseId, native) {
  assertAuthority(process.env, source, releaseId, process.platform, process.arch, native);
  requireValue(execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() === source
    && execFileSync('git', ['-C', root, 'status', '--porcelain', '--untracked-files=no'], { encoding: 'utf8' }).trim() === '', 'The exact final source checkout must be clean');
}
export function releaseAssets(release, releaseId) {
  requireValue(release?.id === Number(releaseId) && release.tag_name === 'v' + version && release.prerelease === true
    && release.draft === true && Array.isArray(release.assets), 'Release metadata differs from the final draft prerelease');
  return updaterNames.map(name => {
    const found = release.assets.filter(asset => asset.name === name);
    requireValue(found.length === 1, 'Missing or ambiguous signed updater asset');
    const asset = found[0];
    requireValue(Number.isSafeInteger(asset.id) && asset.id > 0 && Number.isSafeInteger(asset.size) && asset.size > 0
      && asset.size <= (name.endsWith('.sig') ? 16384 : maximum) && asset.state === 'uploaded'
      && typeof asset.updated_at === 'string' && Number.isFinite(Date.parse(asset.updated_at)), 'Invalid asset identity or size');
    return asset;
  });
}
async function download(url, limit, token) {
  const response = await fetch(url, { headers: token ? { Authorization: 'Bearer ' + token, Accept: 'application/octet-stream' } : {},
    signal: AbortSignal.timeout(180000) });
  requireValue(response.ok && response.body, 'Release download failed with HTTP ' + response.status);
  const chunks = []; let length = 0;
  for await (const chunk of response.body) {
    length += chunk.length;
    requireValue(length <= limit, 'Release download exceeded its bound');
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks);
}
async function metadata(releaseId, token) {
  const response = await fetch('https://api.github.com/repos/' + repository + '/releases/' + releaseId,
    { headers: { Authorization: 'Bearer ' + token, Accept: 'application/vnd.github+json' }, signal: AbortSignal.timeout(30000) });
  requireValue(response.ok, 'Release metadata request failed with HTTP ' + response.status);
  return response.json();
}
function publicKey() { return json(join(root, 'src-tauri', 'tauri.conf.json')).plugins.updater.pubkey; }
export function verifyDownloads(release, releaseId, bytes, key) {
  const assets = releaseAssets(release, releaseId);
  const identities = assets.map(asset => {
    const data = bytes.get(asset.name);
    requireValue(Buffer.isBuffer(data) && data.length === asset.size, 'Actual asset bytes differ from release metadata');
    const digest = sha256(data);
    if (asset.digest) requireValue(asset.digest === 'sha256:' + digest, 'Server asset digest differs');
    return { name: asset.name, id: asset.id, size: asset.size, updated_at: asset.updated_at, sha256: digest };
  });
  makeManifest(version, assets, name => {
    requireValue(bytes.has(name), 'Unsigned or missing updater bytes');
    return bytes.get(name);
  }, key, '');
  return identities;
}
async function prepare(source, releaseId, output) {
  authority(source, releaseId, false);
  output = owned(output);
  requireValue(!existsSync(output), 'Prepare output already exists');
  const token = process.env.GH_TOKEN;
  requireValue(token, 'The same-repository read token is required');
  const release = await metadata(releaseId, token);
  const assets = releaseAssets(release, releaseId);
  const bytes = new Map();
  for (const asset of assets) {
    bytes.set(asset.name, await download('https://api.github.com/repos/' + repository + '/releases/assets/' + asset.id,
      asset.name.endsWith('.sig') ? 16384 : maximum, token));
  }
  const key = publicKey();
  const signed_assets = verifyDownloads(release, releaseId, bytes, key);
  mkdirSync(output);
  for (const name of [filename, filename + '.sig']) writeFileSync(join(output, name), bytes.get(name), { flag: 'wx' });
  const policy = { schema_version: 1, kind: 'release_windows_upgrade_inputs', version, source_revision: source,
    release_id: Number(releaseId), release_was_draft: release.draft, signed_assets, public_key_sha256: sha256(key),
    candidate: signed_assets.find(asset => asset.name === filename), signature: signed_assets.find(asset => asset.name === filename + '.sig') };
  save(join(output, 'policy.json'), policy);
  process.stdout.write(sha256(readFileSync(join(output, 'policy.json'))) + '\n');
}
function policy(input, expectedHash, source, releaseId, stagedFiles = false) {
  input = owned(input);
  const bytes = regular(join(input, 'policy.json'), 1024 * 1024);
  requireValue(/^[a-f0-9]{64}$/.test(expectedHash) && sha256(bytes) === expectedHash, 'Input policy changed after preparation');
  const value = JSON.parse(bytes);
  requireValue(value.schema_version === 1 && value.kind === 'release_windows_upgrade_inputs' && value.version === version
    && value.source_revision === source && value.release_id === Number(releaseId) && value.release_was_draft === true, 'Policy identity differs');
  validateAssets(value.signed_assets);
  requireValue(equal(value.candidate, value.signed_assets.find(asset => asset.name === filename))
    && equal(value.signature, value.signed_assets.find(asset => asset.name === filename + '.sig')), 'Candidate differs from the signed asset inventory');
  const data = regular(join(input, stagedFiles ? 'candidate-setup.exe' : filename));
  const signature = regular(join(input, stagedFiles ? 'candidate-setup.exe.sig' : filename + '.sig'), 16384);
  const key = publicKey();
  requireValue(sha256(data) === value.candidate.sha256 && sha256(signature) === value.signature.sha256
    && sha256(key) === value.public_key_sha256, 'Signed candidate bytes or public key changed');
  verifyTauriSignature(data, signature.toString('utf8').trim(), key, version, filename);
  return { value, data, signature, bytes };
}
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  return value;
}
const equal = (left, right) => JSON.stringify(canonical(left)) === JSON.stringify(canonical(right));
export function validateAssets(assets) {
  requireValue(Array.isArray(assets) && assets.length === updaterNames.length
    && new Set(assets.map(asset => asset?.id)).size === assets.length, 'Exactly eight distinct signed assets are required');
  for (const name of updaterNames) {
    const found = assets.filter(asset => asset?.name === name);
    requireValue(found.length === 1, 'Signed asset inventory is missing or duplicated');
    const asset = found[0];
    requireValue(Number.isSafeInteger(asset.id) && asset.id > 0 && Number.isSafeInteger(asset.size) && asset.size > 0
      && asset.size <= (name.endsWith('.sig') ? 16384 : maximum) && /^[a-f0-9]{64}$/.test(asset.sha256)
      && typeof asset.updated_at === 'string' && Number.isFinite(Date.parse(asset.updated_at)), 'Invalid signed asset receipt');
  }
}
function staged(source, releaseId, output, expectedHash) {
  output = owned(output);
  const inputs = JSON.parse(regular(join(output, 'inputs.json'), 1024 * 1024));
  const verified = policy(output, expectedHash, source, releaseId, true);
  requireValue(Object.hasOwn(oldPackages, inputs.scenario) && inputs.policy_sha256 === expectedHash
    && /^nativeupgradefixturemarker[a-f0-9]{32}$/.test(inputs.marker), 'Invalid staged scenario identity');
  const old = oldPackages[inputs.scenario];
  const url = 'https://github.com/' + repository + '/releases/download/v' + old.version + '/DezirClip_' + old.version + '_x64-setup.exe';
  requireValue(equal(inputs, { ...verified.value, policy_sha256: expectedHash, scenario: inputs.scenario, old: { ...old, url }, marker: inputs.marker })
    && sha256(regular(join(output, 'old-setup.exe'))) === old.sha256, 'Staged inputs or original installer differ from the independently hashed policy');
  return inputs;
}
function verifyStaged(source, releaseId, output, expectedHash) {
  authority(source, releaseId, true);
  staged(source, releaseId, output, expectedHash);
  process.stdout.write('The original installer and signed final candidate match the independently hashed policy.\n');
}
async function stage(source, releaseId, input, expectedHash, scenario, output) {
  authority(source, releaseId, true);
  requireValue(Object.hasOwn(oldPackages, scenario), 'Unknown published upgrade scenario');
  output = owned(output); requireValue(!existsSync(output), 'Native output must be new');
  const verified = policy(input, expectedHash, source, releaseId);
  const old = oldPackages[scenario];
  const url = 'https://github.com/' + repository + '/releases/download/v' + old.version + '/DezirClip_' + old.version + '_x64-setup.exe';
  const oldBytes = await download(url, maximum);
  requireValue(sha256(oldBytes) === old.sha256, 'Published old installer differs from the pinned historical bytes');
  mkdirSync(output);
  writeFileSync(join(output, 'candidate-setup.exe'), verified.data, { flag: 'wx' });
  writeFileSync(join(output, 'candidate-setup.exe.sig'), verified.signature, { flag: 'wx' });
  writeFileSync(join(output, 'policy.json'), verified.bytes, { flag: 'wx' });
  writeFileSync(join(output, 'old-setup.exe'), oldBytes, { flag: 'wx' });
  save(join(output, 'inputs.json'), { ...verified.value, policy_sha256: expectedHash, scenario, old: { ...old, url },
    marker: 'nativeupgradefixturemarker' + randomUUID().replaceAll('-', '') });
}
export function nsisMembers(listing) {
  const records = listing.replaceAll('\r', '').split(/\n\s*\n/).map(block => Object.fromEntries(
    block.split('\n').filter(line => line.includes(' = ')).map(line => [line.slice(0, line.indexOf(' = ')), line.slice(line.indexOf(' = ') + 3)])));
  return ['dezirclip.exe', 'dzc.exe', 'dzc-service.exe'].map(name => {
    const found = records.filter(record => record.Path && record.Path.replaceAll('\\', '/').split('/').at(-1).toLowerCase() === name);
    requireValue(found.length === 1, 'Packaged role is missing or duplicated');
    const member = found[0].Path.replaceAll('\\', '/');
    requireValue(!member.startsWith('/') && !member.startsWith('-') && !member.includes(':') && !member.includes('\0')
      && member.split('/').every(part => part && part !== '.' && part !== '..')
      && !found[0]['Symbolic Link'] && !found[0]['Hard Link'] && found[0].Folder !== '+', 'Unsafe packaged role path');
    return member;
  });
}
export function inspectPe(bytes) {
  requireValue(bytes.length >= 64 && bytes.subarray(0, 2).toString() === 'MZ', 'Expected an actual PE executable');
  const offset = bytes.readUInt32LE(60);
  requireValue(offset >= 64 && offset <= bytes.length - 26 && bytes.subarray(offset, offset + 4).equals(Buffer.from('PE\0\0'))
    && bytes.readUInt16LE(offset + 4) === 0x8664 && bytes.readUInt16LE(offset + 24) === 0x20b, 'Expected the actual native x64 PE role');
}
function extract(source, releaseId, output) {
  authority(source, releaseId, true); output = owned(output);
  const inputs = json(join(output, 'inputs.json'));
  staged(source, releaseId, output, inputs.policy_sha256);
  const archive = join(output, 'candidate-setup.exe');
  requireValue(inputs.source_revision === source && inputs.release_id === Number(releaseId)
    && sha256(regular(archive)) === inputs.candidate.sha256, 'Staged release installer changed');
  const listing = execFileSync('7z', ['l', '-slt', archive], { encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
  writeFileSync(join(output, 'nsis-listing.txt'), listing, { flag: 'wx' });
  const members = nsisMembers(listing);
  const directory = join(output, 'extracted'); requireValue(!existsSync(directory), 'Extraction output exists');
  execFileSync('7z', ['x', '-y', '-o' + directory, archive, '--', ...members], { timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
  const roles = members.map(member => {
    const path = owned(join(directory, member)); const bytes = regular(path); inspectPe(bytes);
    const role = Object.keys(roleFiles).find(role => roleFiles[role] === basename(member).toLowerCase());
    return { role, member, bytes: bytes.length, sha256: sha256(bytes) };
  });
  save(join(output, 'extracted-roles.json'), roles);
}
export function validateInfo(info, role, source) {
  requireValue(Object.hasOwn(roleFiles, role) && info?.component === role && info.component_version === version && info.source_revision === source
    && info.build_id === version + '@' + source && info.source_state === 'verified_checkout'
    && ['local_git', 'explicit_checkout'].includes(info.revision_origin) && info.target === 'x86_64-pc-windows-msvc'
    && info.metadata_version === 1 && info.database_schema_version === 24
    && info.protocol_version?.major === 1 && info.protocol_version?.minor === 0, 'Actual installed role identity differs');
  const names = ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host', ...(role === 'service' ? ['dzc-service'] : [])];
  requireValue(Array.isArray(info.linked_components) && info.linked_components.length === names.length
    && names.every(name => info.linked_components.filter(item => item?.component === name && item.version === version).length === 1), 'Actual installed linked inventory differs');
}
export function validateNative(report, inputs, extracted) {
  requireValue(report?.schema_version === 1 && report.kind === 'release_windows_upgrade_native' && report.status === 'passed'
    && report.failure === null && report.version === version && report.source_revision === inputs.source_revision
    && report.release_id === inputs.release_id && report.policy_sha256 === inputs.policy_sha256
    && report.scenario === inputs.scenario && equal(report.old, inputs.old)
    && equal(report.signed_assets, inputs.signed_assets), 'Native report belongs to different inputs or did not pass');
  validateAssets(report.signed_assets);
  requireValue(Array.isArray(report.checks) && report.checks.length === checkNames.length
    && checkNames.every(name => report.checks.filter(check => check?.name === name && check.status === 'passed'
      && check.evidence !== null && typeof check.evidence === 'object').length === 1), 'All fourteen actual checks and their observations are required');
  const evidence = name => report.checks.find(check => check.name === name).evidence;
  requireValue(equal(report.packaged, extracted) && Array.isArray(extracted) && extracted.length === 3
    && Array.isArray(report.installed) && report.installed.length === 3
    && Object.keys(report.roles ?? {}).sort().join(',') === 'cli,desktop,service', 'The three packaged and installed roles are required');
  for (const role of Object.keys(roleFiles)) {
    validateInfo(report.roles[role], role, inputs.source_revision);
    const packaged = extracted.filter(item => item.role === role);
    const installed = report.installed.filter(item => item.role === role);
    requireValue(packaged.length === 1 && installed.length === 1 && Number.isSafeInteger(packaged[0].bytes) && packaged[0].bytes > 0
      && /^[a-f0-9]{64}$/.test(packaged[0].sha256) && basename(packaged[0].member).toLowerCase() === roleFiles[role]
      && installed[0].bytes === packaged[0].bytes && installed[0].sha256 === packaged[0].sha256, 'Installed role bytes differ from their own original package');
  }
  requireValue(equal(evidence('installed_three_role_bytes'), report.installed)
    && equal(evidence('actual_three_role_build_info'), report.roles), 'Actual role observations differ from the checks');
  const original = evidence('original_installer_hash');
  requireValue(original.sha256 === inputs.old.sha256 && original.expected === inputs.old.sha256
    && evidence('actual_old_install').installer_sha256 === inputs.old.sha256
    && evidence('actual_old_install').cli_version === 'dzc ' + inputs.old.version, 'Historical installer or actual CLI differs');
  const signature = evidence('candidate_signature_and_hash');
  requireValue(signature.verifier === 'verify-staged' && equal(signature.candidate, inputs.candidate)
    && equal(signature.signature, inputs.signature), 'Actual signature verification is missing');
  const upgrade = evidence('actual_upgrade_same_directory');
  requireValue(typeof upgrade.old_directory === 'string' && upgrade.old_directory === upgrade.final_directory
    && upgrade.installer_sha256 === inputs.candidate.sha256
    && equal(upgrade.arguments, ['/S', '/UPDATE', '/D=' + upgrade.old_directory]), 'Actual same-directory NSIS upgrade is missing');
  const oldProfile = evidence('actual_old_profile_created');
  requireValue(equal(oldProfile.actual, report.old_profile) && oldProfile.backup_sha256 === report.old_profile_backup_sha256
    && /^[a-f0-9]{64}$/.test(oldProfile.backup_sha256) && report.old_profile.status === 'passed'
    && Number.isInteger(report.old_profile.schema_version) && report.old_profile.schema_version > 0 && report.old_profile.schema_version <= 21
    && (inputs.scenario !== 'PublishedAlphaToAlpha2' || report.old_profile.schema_version === 21), 'Actual original profile and backup are missing');
  const frozen = report.old_profile.frozen_preferences;
  requireValue(Object.keys(frozen ?? {}).sort().join(',') === ['app.classification_enabled', 'app.hidden_filter_chips', 'app.ocr_enabled', 'app.quick_paste_hotkey', 'app.screenshot_enabled'].join(','), 'All five frozen preferences are required');
  const marker = report.old_profile.marker;
  requireValue(Number.isSafeInteger(marker?.id) && marker.id > 0 && marker.content === inputs.marker && marker.content_type === 'text'
    && marker.is_pinned === 1 && equal(JSON.parse(marker.tags), ['releaseupgradetag']), 'The original marker, pin and tag are missing');
  const history = evidence('history_and_schema24');
  const uninstalled = evidence('uninstall_preserves_profile_and_settings');
  for (const observed of [history.after, uninstalled.profile]) requireValue(observed?.status === 'passed' && observed.schema_version === 24
    && equal(observed.marker, marker) && equal(observed.frozen_preferences, frozen), 'Migrated or uninstalled profile lost history or preferences');
  requireValue(equal(history.before, marker) && history.actual_gui_evidence === 'final-gui-evidence.json'
    && equal(evidence('frozen_preferences_preserved'), { before: frozen, after: frozen }), 'Original history or preference observations differ');
  const settings = evidence('windows_settings_preserved');
  requireValue(Array.isArray(settings.before) && settings.before.length === 6 && equal(settings.before, settings.after)
    && equal(settings.before, uninstalled.windows_settings), 'Upgrade or uninstall changed Windows settings');
  const explorer = evidence('explorer_not_restarted');
  requireValue(Array.isArray(explorer.before) && explorer.before.length > 0 && explorer.had_existing_explorer === true
    && new Set(explorer.before.map(item => item.pid)).size === explorer.before.length
    && explorer.before.every(item => Number.isSafeInteger(item.pid) && item.pid > 0 && typeof item.started_at === 'string'
      && Number.isFinite(Date.parse(item.started_at))) && equal(explorer.before, explorer.after_upgrade)
    && equal(explorer.before, explorer.after_uninstall), 'Explorer identity changed');
  const uninstall = evidence('actual_release_uninstall');
  requireValue(/^[a-f0-9]{64}$/.test(uninstall.uninstaller_sha256) && uninstall.uninstaller_sha256 === report.uninstaller_sha256
    && uninstall.registered_installation_absent === true && equal(uninstall.missing, Object.values(roleFiles)), 'Actual release uninstall is incomplete');
  requireValue(Array.isArray(report.processes) && report.processes.length >= 16
    && new Set(report.processes.map(item => item.label)).size === report.processes.length
    && report.processes.every(item => item.original_handle_retained === true && item.exited === true && item.status === 'passed'
      && Number.isSafeInteger(item.pid) && item.pid > 0 && Number.isInteger(item.exit_code)
      && (item.exit_code === 0 || (item.label === 'actual-final-gui' && item.terminated_original_tree === true)))
    && evidence('controlled_processes_exited').count === report.processes.length
    && evidence('controlled_processes_exited').original_handles_verified === true, 'An original controlled process was not confirmed exited');
  const labels = ['verify-staged', 'extract-packaged-roles', 'install-original-published', 'old-cli-version', 'old-cli-create-profile',
    'seed-owned-old-profile', 'install-final-release-upgrade', 'final-cli-build-info', 'final-service-build-info', 'final-cli-retained-history',
    'upgraded-profile-check', 'actual-final-gui', 'actual-final-gui-bridge', 'after-gui-profile-check', 'uninstall-actual-final-release',
    'uninstalled-profile-check', ...(inputs.scenario === 'PublishedAlphaToAlpha2' ? ['old-cli-build-info'] : [])];
  requireValue(equal(report.processes.map(item => item.label).sort(), labels.sort()), 'A required actual native process observation is missing');
  requireValue(report.gui_process_identity?.verified === true && report.gui_exit?.original_process_exited === true
    && report.gui_exit.cdp_port_released === true && Array.isArray(report.cleanup) && report.cleanup.every(item => !item.error), 'GUI identity, shutdown or cleanup is incomplete');
}
function observedReport(source, releaseId, directory, expectedHash) {
  const inputs = staged(source, releaseId, directory, expectedHash);
  const rawBytes = regular(join(directory, 'raw-native.json'), 8 * 1024 * 1024);
  const report = JSON.parse(rawBytes);
  const extracted = JSON.parse(regular(join(directory, 'extracted-roles.json'), 1024 * 1024));
  validateNative(report, inputs, extracted);
  requireValue(report.input_sha256 === sha256(regular(join(directory, 'inputs.json')))
    && report.old_profile_backup_sha256 === sha256(regular(join(directory, 'old-profile-backup.db'))), 'An original observation input changed');
  for (const item of extracted) {
    requireValue(typeof item.member === 'string' && !item.member.includes('\\') && !item.member.includes(':') && !item.member.startsWith('/')
      && item.member.split('/').every(part => part && part !== '.' && part !== '..'), 'Invalid extracted member path');
    const bytes = regular(owned(join(directory, 'extracted', item.member))); inspectPe(bytes);
    requireValue(bytes.length === item.bytes && sha256(bytes) === item.sha256, 'Retained packaged role changed');
  }
  const gui = JSON.parse(regular(join(directory, 'final-gui-evidence.json'), 8 * 1024 * 1024));
  const guiInput = JSON.parse(regular(join(directory, 'final-gui-input.json')));
  const windowsPath = value => typeof value === 'string' ? value.replaceAll('\\', '/').replace(/\/$/, '').toLowerCase() : null;
  requireValue(gui.result === 'passed' && gui.gui?.oldMarkerId === report.old_profile.marker.id && gui.gui.oldHistoryRendered === true
    && equal(gui.gui.buildInfo, report.roles.desktop) && Array.isArray(gui.nativeClipboardWrites)
    && gui.nativeClipboardWrites.length === 0 && guiInput.acquireOwnership === false && guiInput.verifyCapture === false
    && guiInput.expectedVersion === version && guiInput.marker === inputs.marker && equal(guiInput.expectedBuildInfo, report.roles.cli)
    && windowsPath(gui.gui.dataPath) === windowsPath(guiInput.profileDirectory)
    && windowsPath(report.old_profile.database) === windowsPath(guiInput.profileDirectory) + '/clipboard.db', 'Actual rendered GUI evidence is missing');
  const files = ['raw-native.json', 'inputs.json', 'policy.json', 'extracted-roles.json', 'old-profile-backup.db', 'old-profile.json',
    'profile-check.py', 'upgraded-profile.json', 'final-gui-evidence.json', 'final-gui-input.json', 'final-gui-process-identity.json',
    'nsis-listing.txt', 'windows-settings-original.json', 'windows-settings-before-upgrade.json', 'windows-settings-after-upgrade.json',
    'windows-settings-after-uninstall.json', 'explorer-before-upgrade.json', 'explorer-after-upgrade.json', 'explorer-after-uninstall.json'];
  if (inputs.scenario === 'PublishedAlphaToAlpha2') files.push('old-build-info.json');
  const evidence_files = files.map(name => {
    const bytes = regular(owned(join(directory, name)));
    return { name, bytes: bytes.length, sha256: sha256(bytes) };
  });
  for (const process of report.processes) {
    requireValue(/^[a-z0-9-]+$/.test(process.label), 'Invalid native process log label');
    for (const stream of ['stdout', 'stderr']) {
      const name = process.label + '.' + stream + '.log';
      const path = owned(join(directory, name));
      const stat = lstatSync(path);
      requireValue(stat.isFile() && !stat.isSymbolicLink() && stat.size <= 8 * 1024 * 1024, 'Missing or oversized original native process log');
      const bytes = readFileSync(path);
      evidence_files.push({ name, bytes: bytes.length, sha256: sha256(bytes) });
    }
  }
  for (const [label, info] of [['final-cli-build-info', report.roles.cli], ['final-service-build-info', report.roles.service]]) {
    requireValue(equal(JSON.parse(regular(join(directory, label + '.stdout.log'), 1024 * 1024)), info), 'Actual role stdout differs from its reported metadata');
  }
  const check = name => report.checks.find(item => item.name === name).evidence;
  const comparisons = [
    ['old-profile.json', report.old_profile], ['seed-owned-old-profile.stdout.log', report.old_profile],
    ['upgraded-profile.json', check('history_and_schema24').after], ['upgraded-profile-check.stdout.log', check('history_and_schema24').after],
    ['after-gui-profile-check.stdout.log', check('history_and_schema24').after],
    ['uninstalled-profile-check.stdout.log', check('uninstall_preserves_profile_and_settings').profile],
    ['final-gui-process-identity.json', report.gui_process_identity],
    ['windows-settings-before-upgrade.json', check('windows_settings_preserved').before],
    ['windows-settings-after-upgrade.json', check('windows_settings_preserved').after],
    ['windows-settings-after-uninstall.json', check('uninstall_preserves_profile_and_settings').windows_settings],
    ['explorer-before-upgrade.json', check('explorer_not_restarted').before],
    ['explorer-after-upgrade.json', check('explorer_not_restarted').after_upgrade],
    ['explorer-after-uninstall.json', check('explorer_not_restarted').after_uninstall],
  ];
  for (const [name, expected] of comparisons) requireValue(equal(JSON.parse(regular(join(directory, name), 8 * 1024 * 1024)), expected), 'Retained evidence differs from its native observation: ' + name);
  requireValue(regular(join(directory, 'old-cli-version.stdout.log')).toString('utf8').trim() === 'dzc ' + inputs.old.version, 'Original historical CLI stdout differs');
  if (inputs.scenario === 'PublishedAlphaToAlpha2') {
    const oldInfo = JSON.parse(regular(join(directory, 'old-build-info.json')));
    requireValue(equal(oldInfo, JSON.parse(regular(join(directory, 'old-cli-build-info.stdout.log'))))
      && oldInfo.component === 'cli' && oldInfo.component_version === inputs.old.version && oldInfo.source_revision === inputs.old.source_revision
      && oldInfo.build_id === inputs.old.version + '@' + inputs.old.source_revision && oldInfo.source_state === 'verified_checkout'
      && oldInfo.database_schema_version === 21 && oldInfo.target === 'x86_64-pc-windows-msvc'
      && Array.isArray(oldInfo.linked_components) && oldInfo.linked_components.length === 3
      && ['dzc-core', 'dzc-platform', 'dzc-protocol'].every(name => oldInfo.linked_components.filter(item => item.component === name && item.version === inputs.old.version).length === 1), 'Actual historical alpha.1 stdout/source/schema/inventory differs');
  }
  return { schema_version: 1, kind: 'release_windows_upgrade_report', status: 'passed', version, source_revision: source,
    release_id: Number(releaseId), scenario: inputs.scenario, policy_sha256: expectedHash, old: inputs.old,
    signed_assets: inputs.signed_assets, checks: report.checks, roles: report.roles, installed: report.installed,
    packaged: report.packaged, raw_native_sha256: sha256(rawBytes), evidence_files };
}
function seal(source, releaseId, directory) {
  authority(source, releaseId, true); directory = owned(directory);
  const inputs = json(join(directory, 'inputs.json'));
  save(join(directory, 'report.json'), observedReport(source, releaseId, directory, inputs.policy_sha256));
}
export function combine(reports, source, releaseId, expectedPolicy) {
  requireValue(/^[a-f0-9]{64}$/.test(expectedPolicy) && reports.length === 2
    && new Set(reports.map(item => item.value.scenario)).size === 2, 'Both published upgrade scenarios are required');
  for (const { value: report, sha256: digest } of reports) requireValue(report?.schema_version === 1
    && report.kind === 'release_windows_upgrade_report' && Object.hasOwn(oldPackages, report.scenario) && report.status === 'passed'
    && report.version === version && report.source_revision === source && report.release_id === Number(releaseId)
    && report.policy_sha256 === expectedPolicy && /^[a-f0-9]{64}$/.test(digest)
    && /^[a-f0-9]{64}$/.test(report.raw_native_sha256) && report.old?.sha256 === oldPackages[report.scenario].sha256
    && Array.isArray(report.checks) && report.checks.length === checkNames.length
    && checkNames.every(name => report.checks.filter(check => check?.name === name && check.status === 'passed').length === 1), 'A validated scenario report is missing');
  for (const { value: report } of reports) {
    validateAssets(report.signed_assets);
    for (const role of Object.keys(roleFiles)) validateInfo(report.roles?.[role], role, source);
  }
  requireValue(equal(reports[0].value.signed_assets, reports[1].value.signed_assets), 'Scenarios observed different release asset identities');
  return { schema_version: 1, kind: 'windows_upgrade', status: 'passed', version, source_revision: source,
    release_id: Number(releaseId), policy_sha256: expectedPolicy, signed_assets: reports[0].value.signed_assets,
    checks: reports.map(({ value: report, sha256: digest }) => ({ name: report.scenario, status: 'passed', report_sha256: digest,
      raw_native_sha256: report.raw_native_sha256 })) };
}
function aggregate(source, releaseId, expectedPolicy, directory, output) {
  authority(source, releaseId, false); directory = owned(directory); output = owned(output);
  const reports = Object.keys(oldPackages).map(scenario => {
    const folder = readdirSync(directory).filter(name => name === 'release-windows-upgrade-' + scenario + '-attempt-' + process.env.GITHUB_RUN_ATTEMPT);
    requireValue(folder.length === 1, 'Missing exact current-run upgrade artifact');
    const reportDirectory = owned(join(directory, folder[0]));
    const bytes = regular(join(reportDirectory, 'report.json'), 8 * 1024 * 1024);
    const value = JSON.parse(bytes);
    requireValue(value.scenario === scenario && equal(value, observedReport(source, releaseId, reportDirectory, expectedPolicy)), 'Sealed report differs from its retained native evidence');
    return { value, sha256: sha256(bytes) };
  });
  save(output, combine(reports, source, releaseId, expectedPolicy));
}
async function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'prepare' && args.length === 3) await prepare(...args);
  else if (command === 'stage' && args.length === 6) await stage(...args);
  else if (command === 'verify-staged' && args.length === 4) verifyStaged(...args);
  else if (command === 'extract' && args.length === 3) extract(...args);
  else if (command === 'seal' && args.length === 3) seal(...args);
  else if (command === 'aggregate' && args.length === 5) aggregate(...args);
  else throw new Error('Unknown release-only upgrade controller command');
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
