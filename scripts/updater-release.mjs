import { createHash, createPublicKey, verify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync, lstatSync, realpathSync,
  openSync, closeSync, fstatSync, readSync, constants } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const repository = 'qiyuanhuakai/dezirclip';
export const channelTag = 'updater-channels';
export const channelUrl = channel => `https://github.com/${repository}/releases/download/${channelTag}/${channel}.json`;
const expectedPlatforms = {
  'windows-x86_64': 'x64-setup.exe',
  'windows-aarch64': 'arm64-setup.exe',
  'linux-x86_64': 'amd64.AppImage',
  'linux-aarch64': 'aarch64.AppImage',
};

export function parseVersion(input) {
  const value = input.replace(/^v/, '');
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*))?(?:\+([\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*))?$/.exec(value);
  if (!match || match[0] !== value || (match[4] ?? '').split('.').some(item => /^0\d+$/.test(item))) {
    throw new Error('Release version must be valid SemVer');
  }
  return { value, numbers: match.slice(1, 4).map(BigInt), pre: match[4]?.split('.') ?? [] };
}

export function compareVersions(left, right) {
  const a = parseVersion(left), b = parseVersion(right);
  for (let i = 0; i < 3; i++) {
    if (a.numbers[i] !== b.numbers[i]) return a.numbers[i] > b.numbers[i] ? 1 : -1;
  }
  if (!a.pre.length || !b.pre.length) return Number(!a.pre.length) - Number(!b.pre.length);
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    if (a.pre[i] === undefined) return -1;
    if (b.pre[i] === undefined) return 1;
    if (a.pre[i] === b.pre[i]) continue;
    const an = /^\d+$/.test(a.pre[i]), bn = /^\d+$/.test(b.pre[i]);
    if (an && bn) return BigInt(a.pre[i]) > BigInt(b.pre[i]) ? 1 : -1;
    if (an !== bn) return an ? -1 : 1;
    return a.pre[i] > b.pre[i] ? 1 : -1;
  }
  return 0;
}

function decodeBase64(value) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(value.trim())) {
    throw new Error('Invalid base64 signing material');
  }
  return Buffer.from(value.trim(), 'base64');
}

export function readPublicKey(encoded) {
  const lines = decodeBase64(encoded).toString('utf8').trim().split(/\r?\n/);
  const packet = decodeBase64(lines[1]);
  if (packet.length !== 42 || packet.subarray(0, 2).toString() !== 'Ed') {
    throw new Error('A real Tauri public signing key must be configured');
  }
  return packet;
}

// Minisign's Ed25519/BLAKE2b format, also used by the official Tauri verifier.
// Both the artifact signature and the trusted-comment signature are mandatory.
export function verifyTauriSignature(data, encodedSignature, encodedPublicKey, version, filename) {
  const publicPacket = readPublicKey(encodedPublicKey);
  const lines = decodeBase64(encodedSignature).toString('utf8').trim().split(/\r?\n/);
  const packet = decodeBase64(lines[1]);
  const globalSignature = decodeBase64(lines[3]);
  if (lines.length !== 4 || !lines[2].startsWith('trusted comment: ')
      || packet.length !== 74 || globalSignature.length !== 64
      || !packet.subarray(2, 10).equals(publicPacket.subarray(2, 10))) {
    throw new Error('Invalid Tauri signature or signing key');
  }
  const algorithm = packet.subarray(0, 2).toString();
  if (!['Ed', 'ED'].includes(algorithm)) throw new Error('Unsupported signature algorithm');
  const comment = lines[2].slice('trusted comment: '.length);
  const key = createPublicKey({ key: Buffer.concat([
    Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10),
  ]), format: 'der', type: 'spki' });
  const signature = packet.subarray(10);
  const message = algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data;
  if (!verify(null, message, key, signature)
      || !verify(null, Buffer.concat([signature, Buffer.from(comment)]), key, globalSignature)) {
    throw new Error('Tauri artifact signature verification failed');
  }
  const fields = comment.split('\t');
  const signedVersions = fields.filter(field => field.startsWith('version:'));
  const signedFiles = fields.filter(field => field.startsWith('file:'));
  if (signedVersions.length !== 1 || parseVersion(signedVersions[0].slice(8)).value !== parseVersion(version).value) {
    throw new Error('Tauri signature does not bind the announced version');
  }
  if (filename && (signedFiles.length !== 1 || signedFiles[0].slice(5) !== filename)) {
    throw new Error('Tauri signature does not bind the artifact filename');
  }
}

export function preflight(config, pkg) {
  readPublicKey(config.plugins?.updater?.pubkey);
  if (config.bundle?.createUpdaterArtifacts !== true
      || config.plugins.updater.requireSignedVersion !== true
      || config.plugins.updater.windows?.installMode !== 'passive'
      || JSON.stringify(config.plugins.updater.endpoints) !== JSON.stringify([channelUrl('stable')])
      || pkg.devDependencies['@tauri-apps/cli'] !== '2.12.3') {
    throw new Error('Updater signing and fixed endpoint configuration is incomplete');
  }
}

export function makeManifest(version, assets, readAsset, publicKey, notes = '') {
  const parsed = parseVersion(version);
  const platforms = {};
  for (const [platform, suffix] of Object.entries(expectedPlatforms)) {
    const name = `DezirClip_${parsed.value}_${suffix}`;
    for (const required of [name, `${name}.sig`]) {
      if (assets.filter(asset => asset.name === required && asset.size > 0).length !== 1) {
        throw new Error(`Missing or duplicate release asset: ${required}`);
      }
    }
    const data = readAsset(name);
    const signature = readAsset(`${name}.sig`).toString('utf8').trim();
    if (data.length !== assets.find(asset => asset.name === name).size) {
      throw new Error(`Release asset size differs: ${name}`);
    }
    verifyTauriSignature(data, signature, publicKey, parsed.value, name);
    platforms[platform] = {
      url: `https://github.com/${repository}/releases/download/v${parsed.value}/${encodeURIComponent(name)}`,
      signature,
    };
  }
  return { version: parsed.value, notes, platforms };
}

export function promotionChannel(version) { return parseVersion(version).pre.length ? 'preview' : 'stable'; }
export function shouldPromote(current, candidate) {
  return !current || compareVersions(candidate, current) > 0;
}

const acceptanceGates = ['public_assets', 'appimage_updates', 'windows_upgrade', 'performance'];
const assetIdentity = assets => !Array.isArray(assets) || assets.length !== 8 ? null : JSON.stringify(
  assets.map(asset => ({ name: asset.name, id: asset.id, size: asset.size,
    updated_at: asset.updated_at, sha256: asset.sha256 })).sort((a, b) => a.name.localeCompare(b.name)));

function readEvidenceJson(path) {
  let descriptor;
  try {
    const info = lstatSync(path);
    if (!info.isFile() || info.isSymbolicLink() || info.size === 0 || info.size > 8 * 1024 * 1024) throw new Error();
    descriptor = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const unchanged = observed => observed.isFile() && !observed.isSymbolicLink()
      && observed.dev === info.dev && observed.ino === info.ino && observed.size === info.size
      && observed.mtimeMs === info.mtimeMs && observed.ctimeMs === info.ctimeMs;
    if (!unchanged(fstatSync(descriptor))) throw new Error();
    const buffer = Buffer.alloc(info.size + 1);
    let length = 0;
    while (length < buffer.length) {
      const count = readSync(descriptor, buffer, length, buffer.length - length, null);
      if (!count) break;
      length += count;
    }
    if (length !== info.size || !unchanged(fstatSync(descriptor)) || !unchanged(lstatSync(path))) throw new Error();
    const bytes = buffer.subarray(0, length);
    return { value: JSON.parse(bytes.toString('utf8')), sha256: createHash('sha256').update(bytes).digest('hex') };
  } catch { throw new Error('Public acceptance report is missing, malformed or changed'); }
  finally { if (descriptor !== undefined) closeSync(descriptor); }
}

function acceptanceBundle(path) {
  if (typeof path !== 'string' || !path) throw new Error('Promotion requires an acceptance bundle path');
  const file = resolve(path);
  const root = realpathSync(dirname(file));
  const acceptance = readEvidenceJson(file).value;
  const reports = {};
  for (const gate of acceptanceGates) {
    const reference = acceptance.gates?.[gate];
    const name = reference?.file;
    if (typeof name !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]*\.json$/.test(name)
        || typeof reference.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(reference.sha256)) {
      throw new Error('Promotion requires completed public acceptance report files');
    }
    const report = readEvidenceJson(join(root, name));
    if (report.sha256 !== reference.sha256 || report.value?.status !== 'passed'
        || report.value.schema_version !== 1 || report.value.kind !== gate
        || report.value.version !== acceptance.version || report.value.source_revision !== acceptance.source_revision
        || report.value.release_id !== acceptance.release_id
        || assetIdentity(report.value.signed_assets) !== assetIdentity(acceptance.signed_assets)
        || !Array.isArray(report.value.checks) || report.value.checks.length === 0
        || report.value.checks.some(check => !check || typeof check.name !== 'string' || !check.name.trim() || check.status !== 'passed')) {
      throw new Error('Promotion requires matching passed public acceptance reports');
    }
    reports[gate] = report.sha256;
  }
  return { acceptance, reports };
}

function gh(args, binary = false) {
  return execFileSync('gh', args, { encoding: binary ? undefined : 'utf8',
    maxBuffer: 1024 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
}
function githubClient(transport) {
  const api = path => JSON.parse(transport(['api', `repos/${repository}/${path}`]));
  function releaseByTag(tag, optional = false) {
    // The by-tag endpoint only returns published releases. Authenticated, fully
    // paginated listing includes drafts; only a successful empty match is absent.
    const pages = JSON.parse(transport([
      'api', `repos/${repository}/releases?per_page=100`, '--paginate', '--slurp',
    ]));
    if (!Array.isArray(pages) || pages.some(page => !Array.isArray(page))) {
      throw new Error('GitHub release listing is incomplete');
    }
    const matches = pages.flat().filter(release => release.tag_name === tag);
    if (!matches.length && optional) return null;
    if (matches.length !== 1 || !Number.isSafeInteger(matches[0].id) || matches[0].id <= 0) {
      throw new Error(`Missing or ambiguous GitHub release: ${tag}`);
    }
    const release = api(`releases/${matches[0].id}`);
    if (release.id !== matches[0].id || release.tag_name !== tag) {
      throw new Error('GitHub release identity changed during lookup');
    }
    return release;
  }
  function downloadAsset(asset) {
    return transport(['api', `repos/${repository}/releases/assets/${asset.id}`, '-H', 'Accept: application/octet-stream'], true);
  }
  function moveAsset(asset, name) {
    return JSON.parse(transport(['api', `repos/${repository}/releases/assets/${asset.id}`,
      '-X', 'PATCH', '-f', `name=${name}`]));
  }
  function releaseCommit(tag) {
    let object = api(`git/ref/tags/${encodeURIComponent(tag)}`).object;
    for (let depth = 0; object.type === 'tag' && depth < 8; depth++) {
      object = api(`git/tags/${object.sha}`).object;
    }
    if (object.type !== 'commit' || !/^[a-f0-9]{40}$/.test(object.sha)) {
      throw new Error('Release tag does not resolve to a Git commit');
    }
    return object.sha;
  }
  return { releaseByTag, downloadAsset, moveAsset, releaseCommit };
}

const publisherJobs = [
  'Verify signed artifacts and publish before public acceptance',
  'Verify signed artifacts, publish and synchronize update channel',
];
const releaseGateJobs = [
  'Collect actual final package identities / Bind final source and all fourteen assets',
  'Verify actual signed Windows upgrades / Bind all actual final installer upgrades',
];
const mygoJob = 'Windows x64 MyGo test download';
const requiresMyGo = version => compareVersions(version, '0.2.0-alpha.9') >= 0;
const requiresFullSourceAcceptance = version => compareVersions(version, '0.2.0-alpha.10') >= 0;
const fullSourceJobs = [
  'Full source acceptance / Frontend',
  ...['windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64'].map(platform => `Full source acceptance / Tauri backend (${platform})`),
  ...['LegacyUninstall', 'OldToNew', 'PublishedAlphaToAlpha', 'OwnedUpgrade', 'OwnedExternalEdit', 'DeniedGuiUpgrade'].map(scenario => `Full source acceptance / Native NSIS (${scenario})`),
  ...['linux-x86_64', 'linux-aarch64'].map(platform => `Full source acceptance / Verified Flatpak / Flatpak (${platform})`),
];
const nativeReleaseJobs = [
  'Create GitHub release', 'Publish verified standalone archives',
  ...Object.keys(expectedPlatforms).map(platform => `Tauri release (${platform})`),
  ...Object.keys(expectedPlatforms).map(platform => `Build compatible standalone CLI / Standalone (${platform})`),
  ...['linux-x86_64', 'linux-aarch64'].map(platform => `Verified Flatpak release / Flatpak (${platform})`),
  'Verify actual signed Windows upgrades / Verify final signed upgrade inputs',
  ...Object.keys(expectedPlatforms).map(platform => `Collect actual final package identities / Release identities (${platform})`),
  ...['LegacyToAlpha2', 'PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease']
    .map(scenario => `Verify actual signed Windows upgrades / Final published upgrade (${scenario})`),
];

function verifiedReleaseRun(transport, tag, commit, runId, runAttempt, repair = false) {
  if (!/^[1-9]\d*$/.test(String(runId)) || !Number.isSafeInteger(Number(runId))
      || !/^[1-9]\d*$/.test(String(runAttempt)) || !Number.isSafeInteger(Number(runAttempt))) {
    throw new Error('Synchronization requires an exact Release run and attempt');
  }
  const api = route => JSON.parse(transport(['api', `repos/${repository}/${route}`]));
  const workflow = api('actions/workflows/release.yml');
  const run = api(`actions/runs/${runId}/attempts/${runAttempt}`);
  const latest = api(`actions/runs/${runId}`);
  const validRun = value => value?.id === Number(runId) && value.run_attempt === Number(runAttempt)
    && value.workflow_id === workflow.id && value.path === '.github/workflows/release.yml'
    && value.event === 'push' && value.head_branch === tag && value.head_sha === commit
    && value.repository?.full_name === repository && value.head_repository?.full_name === repository
    && ((value.status === 'in_progress' && value.conclusion === null)
      || (value.status === 'completed' && (value.conclusion === 'success' || (repair && value.conclusion === 'failure'))));
  if (!Number.isSafeInteger(workflow?.id) || workflow.id <= 0 || workflow.path !== '.github/workflows/release.yml'
      || workflow.state !== 'active' || !validRun(run) || !validRun(latest)
      || latest.status !== run.status || latest.conclusion !== run.conclusion) {
    throw new Error('Release workflow run/source/attempt is not eligible for synchronization');
  }
  const pages = JSON.parse(transport(['api',
    `repos/${repository}/actions/runs/${runId}/attempts/${runAttempt}/jobs?per_page=100`, '--paginate', '--slurp']));
  const count = pages?.[0]?.total_count;
  if (!Array.isArray(pages) || !pages.length || !Number.isSafeInteger(count) || count <= 0
      || pages.some(page => page.total_count !== count || !Array.isArray(page.jobs))) {
    throw new Error('Release attempt job listing is incomplete');
  }
  const jobs = pages.flatMap(page => page.jobs);
  if (jobs.length !== count || new Set(jobs.map(job => job.id)).size !== count
      || new Set(jobs.map(job => job.name)).size !== count
      || jobs.some(job => !Number.isSafeInteger(job.id) || job.id <= 0 || typeof job.name !== 'string'
        || !job.name.trim() || job.run_id !== Number(runId) || job.run_attempt !== Number(runAttempt)
        || job.head_sha !== commit)) {
    throw new Error('Release attempt jobs have missing or conflicting identities');
  }
  const publishers = jobs.filter(job => publisherJobs.includes(job.name));
  if (publishers.length !== 1 || (requiresMyGo(tag) && jobs.filter(job => job.name === mygoJob).length !== 1)
      || (requiresFullSourceAcceptance(tag) && fullSourceJobs.some(name => !jobs.some(job => job.name === name && job.conclusion === 'success')))
      || jobs.some(job => !(job.status === 'completed' && job.conclusion === 'success')
      && !(['Synchronize an already published update channel', 'Supplement published MyGo installer',
        'Resume verified publication or synchronize update channel',
        'Full source acceptance / Published Windows memory comparison',
        'Full source acceptance / native-file-drag',
        'Full source acceptance / Optional Windows Surface comparison or crash diagnostics'].includes(job.name)
        && job.status === 'completed' && job.conclusion === 'skipped')
      && !(repair && job === publishers[0] && job.status === 'completed' && job.conclusion === 'failure')
      && !(job === publishers[0] && job.status === 'in_progress' && job.conclusion === null))
      || (run.status === 'completed' && publishers[0].status !== 'completed')
      || (run.conclusion === 'failure' && publishers[0].conclusion !== 'failure')
      || (run.conclusion === 'success' && publishers[0].conclusion !== 'success')
      || [...releaseGateJobs, ...nativeReleaseJobs].some(name => !jobs.some(job => job.name === name))) {
    throw new Error('Release native identity/upgrade gates or another job have not succeeded');
  }
  const identities = Object.keys(expectedPlatforms).map(platform => jobs.find(job =>
    job.name === `Collect actual final package identities / Release identities (${platform})`));
  const times = identities.map(job => Date.parse(job.started_at));
  if (identities.some(job => typeof job.started_at !== 'string') || times.some(time => !Number.isFinite(time))) {
    throw new Error('Native release identity start times are missing');
  }
  return { run_id: Number(runId), run_attempt: Number(runAttempt), workflow_id: workflow.id,
    status: run.status, conclusion: run.conclusion, asset_cutoff: Math.min(...times),
    jobs: jobs.map(job => ({ id: job.id, name: job.name, status: job.status, conclusion: job.conclusion,
      started_at: job.started_at })).sort((a, b) => a.id - b.id) };
}

function verifiedReleaseAssets(release, version, cutoff) {
  const parsed = parseVersion(version), value = parsed.value;
  if (parsed.pre.length && !/^(alpha|beta|rc)\.(0|[1-9]\d*)$/.test(parsed.pre.join('.'))) {
    throw new Error('Unsupported RPM release version');
  }
  const rpmVersion = parsed.numbers.join('.'), rpmRelease = parsed.pre.length ? `0.${parsed.pre.join('.')}` : '1';
  const updater = Object.values(expectedPlatforms).map(suffix => `DezirClip_${value}_${suffix}`);
  const cli = Object.keys(expectedPlatforms).map(platform => `dzc-${value}-${platform}.${platform.startsWith('windows-') ? 'zip' : 'tar.gz'}`);
  const deb = ['amd64', 'arm64'].map(arch => `DezirClip_${value}_${arch}.deb`);
  const names = [...updater.flatMap(name => [name, `${name}.sig`]), ...cli.flatMap(name => [name, `${name}.sha256`]),
    ...deb.flatMap(name => [name, `${name}.sig`]),
    ...['x86_64', 'aarch64'].map(arch => `DezirClip-${rpmVersion}-${rpmRelease}.${arch}.rpm`),
    ...['x86_64', 'aarch64'].map(arch => `DezirClip-${value}-linux-${arch}.flatpak`)];
  let assets = release.assets;
  if (requiresMyGo(value)) names.push(`DezirClip-v${value}-windows-x64-MyGo${value === '0.2.0-alpha.9' ? '.zip' : '-setup.exe'}`);
  // alpha.9's original gate predates its separately tested installer supplement.
  // That attachment is outside the signed updater set and original time cutoff.
  if (value === '0.2.0-alpha.9' && !release.draft && Array.isArray(assets)) {
    const supplements = assets.filter(asset => asset.name === `DezirClip-v${value}-windows-x64-MyGo-setup.exe`);
    if (supplements.length > 1 || supplements.some(asset => !Number.isSafeInteger(asset.id) || asset.id <= 0
        || !Number.isSafeInteger(asset.size) || asset.size <= 0 || asset.state !== 'uploaded'
        || !/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? ''))) {
      throw new Error('Invalid alpha.9 MyGo installer supplement');
    }
    assets = assets.filter(asset => !supplements.includes(asset));
  }
  if (!Array.isArray(assets) || assets.length !== names.length || new Set(assets.map(asset => asset.id)).size !== names.length
      || names.some(name => assets.filter(asset => asset.name === name).length !== 1)
      || assets.some(asset => !Number.isSafeInteger(asset.id) || asset.id <= 0
        || !Number.isSafeInteger(asset.size) || asset.size <= 0 || asset.state !== 'uploaded'
        || !/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? '') || typeof asset.updated_at !== 'string'
        || !Number.isFinite(Date.parse(asset.updated_at))
        || Date.parse(asset.updated_at) > cutoff)) {
    throw new Error(`All ${names.length} release assets must retain their pre-gate identities`);
  }
  return JSON.stringify(assets.map(({ id, name, size, state, digest, updated_at }) =>
    ({ id, name, size, state, digest, updated_at })).sort((a, b) => a.name.localeCompare(b.name)));
}

function sameManifest(value, expected) {
  return value && JSON.stringify(Object.keys(value).sort()) === JSON.stringify(Object.keys(expected).sort())
    && value.version === expected.version && value.notes === expected.notes && value.platforms
    && JSON.stringify(Object.keys(value.platforms).sort()) === JSON.stringify(Object.keys(expected.platforms).sort())
    && Object.entries(expected.platforms).every(([platform, item]) => {
      const actual = value.platforms[platform];
      return actual && Object.keys(actual).sort().join(',') === 'signature,url'
        && actual.url === item.url && actual.signature === item.signature;
    });
}

function channelWriteIdentity(release) {
  if (release === null) return null;
  return { id: release.id, tag_name: release.tag_name, draft: release.draft, prerelease: release.prerelease,
    assets: release.assets.map(({ id, name, size, state, digest, updated_at }) =>
      ({ id, name, size, state, digest, updated_at })).sort((a, b) => a.name.localeCompare(b.name)) };
}

function verifiedUpgradeGate(evidence, receipt) {
  const gate = evidence.value;
  const names = ['LegacyToAlpha2', 'PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease'];
  if (gate?.schema_version !== 1 || gate.kind !== 'windows_upgrade' || gate.status !== 'passed'
      || gate.version !== receipt.version || gate.source_revision !== receipt.source_revision
      || gate.release_id !== receipt.release_id || !/^[a-f0-9]{64}$/.test(gate.policy_sha256 ?? '')
      || assetIdentity(gate.signed_assets) !== assetIdentity(receipt.signed_assets)
      || !Array.isArray(gate.checks) || gate.checks.length !== names.length
      || names.some(name => gate.checks.filter(check => check?.name === name).length !== 1)
      || gate.checks.some(check => check.status !== 'passed' || !/^[a-f0-9]{64}$/.test(check.report_sha256 ?? '')
        || !/^[a-f0-9]{64}$/.test(check.raw_native_sha256 ?? ''))) {
    throw new Error('Windows upgrade gate does not bind the exact source, eight signed assets and three native upgrades');
  }
}

function publication(mode, version, config, log, transport, bundle) {
  const client = githubClient(transport);
  const parsed = parseVersion(version);
  const tag = `v${parsed.value}`;
  const channel = promotionChannel(parsed.value);
  const release = client.releaseByTag(tag);
  if (release.tag_name !== tag || release.prerelease !== Boolean(parsed.pre.length)) {
    throw new Error('Release tag or prerelease flag differs from the signed version');
  }
  const commit = client.releaseCommit(tag);
  const resumeDraft = mode === 'resume-verified-draft';
  const synchronize = mode === 'publish-and-sync' || mode === 'sync-published' || resumeDraft;
  const repair = mode === 'sync-published' || resumeDraft;
  const run = synchronize ? verifiedReleaseRun(transport, tag, commit, bundle.runId, bundle.runAttempt, repair) : null;
  const releaseAssets = synchronize ? verifiedReleaseAssets(release, parsed.value, run.asset_cutoff) : null;
  if (synchronize && typeof release.draft !== 'boolean') throw new Error('Release draft state is missing');
  if (mode === 'sync-published' && (release.draft !== false || run.status !== 'completed')) {
    throw new Error('Published-channel synchronization requires a public release and completed eligible original run');
  }
  if (resumeDraft && (release.draft !== true || run.status !== 'completed' || run.conclusion !== 'failure')) {
    throw new Error('Draft recovery requires an unpublished release and completed publisher-only failure');
  }
  if (mode === 'promote' && release.draft) throw new Error('Promotion requires an already published release');
  const signedAssets = [];
  const manifest = makeManifest(parsed.value, release.assets, name => {
    const asset = release.assets.find(asset => asset.name === name);
    const bytes = client.downloadAsset(asset);
    if (synchronize && (bytes.length !== asset.size
        || `sha256:${createHash('sha256').update(bytes).digest('hex')}` !== asset.digest)) {
      throw new Error('Downloaded signed asset differs from GitHub digest');
    }
    signedAssets.push({ name, id: asset.id, size: asset.size, updated_at: asset.updated_at,
      sha256: createHash('sha256').update(bytes).digest('hex') });
    return bytes;
  }, config.plugins.updater.pubkey, release.body ?? '');
  const receipt = { schema_version: 1, version: parsed.value, source_revision: commit,
    release_id: release.id, signed_assets: signedAssets };
  if (synchronize) verifiedUpgradeGate(bundle.upgradeEvidence, receipt);
  const syncReceipt = synchronize ? { ...receipt, workflow: run,
    windows_upgrade_gate_sha256: bundle.upgradeEvidence.sha256 } : null;
  if (mode === 'promote') {
    const acceptance = bundle?.acceptance;
    if (acceptance?.status !== 'passed' || acceptance.schema_version !== 1
        || acceptance.version !== receipt.version || acceptance.source_revision !== commit
        || acceptance.release_id !== release.id
        || assetIdentity(acceptance.signed_assets) !== assetIdentity(signedAssets)
        || acceptanceGates.some(gate => bundle.reports[gate] !== acceptance.gates?.[gate]?.sha256)) {
      throw new Error('Promotion requires matching completed public acceptance evidence');
    }
  }
  let channels = mode !== 'publish-only' ? client.releaseByTag(channelTag, true) : null;
  if (channels?.draft) throw new Error('Existing channel release is still a draft; inspect it before retrying');
  if (synchronize && channels && channels.assets.filter(asset => asset.name === `${channel}.json`).length > 1) {
    throw new Error('Updater channel contains duplicate active manifests');
  }
  const old = channels?.assets.find(asset => asset.name === `${channel}.json`);
  const currentBytes = old ? client.downloadAsset(old) : null;
  const current = currentBytes ? JSON.parse(currentBytes.toString('utf8')) : null;
  if (current) {
    parseVersion(current.version);
    if (channel === 'stable' && parseVersion(current.version).pre.length) {
      throw new Error('Stable channel contains a prerelease');
    }
    if (synchronize && compareVersions(current.version, parsed.value) === 0 && !sameManifest(current, manifest)) {
      throw new Error('Equal-version channel manifest does not exactly match the verified release');
    }
  }
  // Check asset identity again before making the verified release public.
  const currentRun = synchronize
    ? verifiedReleaseRun(transport, tag, commit, bundle.runId, bundle.runAttempt, repair) : null;
  const latest = client.releaseByTag(tag);
  if (latest.id !== release.id || client.releaseCommit(tag) !== commit) throw new Error('Release source changed during verification');
  for (const asset of release.assets) {
    if (!latest.assets.some(value => value.id === asset.id && value.name === asset.name && value.size === asset.size
        && value.updated_at === asset.updated_at)) {
      throw new Error('Release assets changed during verification');
    }
  }
  if (synchronize) {
    if (latest.tag_name !== tag || latest.prerelease !== release.prerelease || latest.body !== release.body
        || verifiedReleaseAssets(latest, parsed.value, run.asset_cutoff) !== releaseAssets
        || JSON.stringify(currentRun) !== JSON.stringify(run)) {
      throw new Error('Release workflow gates or asset identities changed before publication');
    }
    if (readEvidenceJson(bundle.upgradePath).sha256 !== bundle.upgradeEvidence.sha256) {
      throw new Error('Windows upgrade gate changed before publication');
    }
    const freshChannels = client.releaseByTag(channelTag, true);
    if (JSON.stringify(channelWriteIdentity(freshChannels)) !== JSON.stringify(channelWriteIdentity(channels))) {
      throw new Error('Updater channel changed before synchronization');
    }
  }
  if (release.draft) {
    transport(['release', 'edit', tag, '--repo', repository, '--draft=false',
      parsed.pre.length ? '--latest=false' : '--latest']);
  }
  const published = client.releaseByTag(tag);
  if (published.draft) throw new Error('Release has not been published');
  if (synchronize && (published.draft !== false || published.id !== release.id || published.tag_name !== tag
      || published.prerelease !== release.prerelease || published.body !== release.body
      || client.releaseCommit(tag) !== commit || verifiedReleaseAssets(published, parsed.value, run.asset_cutoff) !== releaseAssets)) {
    throw new Error('Published release readback differs from the verified release');
  }
  if (mode === 'publish-only') {
    log(`Published ${tag}; updater channels await separate public acceptance.`);
    return { ...receipt, status: 'awaiting_acceptance' };
  }
  if (!shouldPromote(current?.version, parsed.value)) {
    log(`Published ${tag}; ${channel} already points to an equal or newer version.`);
    if (synchronize) {
      const active = client.releaseByTag(channelTag).assets.filter(asset => asset.name === `${channel}.json`);
      if (active.length !== 1 || active[0].id !== old.id || !client.downloadAsset(active[0]).equals(currentBytes)) {
        throw new Error('Existing updater channel readback differs');
      }
      return { ...syncReceipt, status: compareVersions(current.version, parsed.value) === 0
        ? 'already_synchronized' : 'newer_channel_preserved', channel, channel_version: current.version };
    }
    return;
  }
  const staging = mkdtempSync(join(tmpdir(), 'dezirclip-channel-'));
  try {
    const stagedName = `${channel}-${parsed.value}.json`;
    const path = join(staging, stagedName);
    const contents = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
    writeFileSync(path, contents);
    const fresh = !channels;
    if (fresh) {
      transport(['release', 'create', channelTag, '--repo', repository, '--draft', '--prerelease',
        '--latest=false', '--target', commit, '--title', 'DezirClip updater channels',
        '--notes', 'Signed updater manifests. Stable accepts official releases; Preview checks both channels.']);
      channels = client.releaseByTag(channelTag);
    }
    transport(['release', 'upload', channelTag, path, '--repo', repository, '--clobber']);
    channels = client.releaseByTag(channelTag);
    const staged = channels.assets.find(asset => asset.name === stagedName);
    if (!staged || !client.downloadAsset(staged).equals(contents)) throw new Error('Uploaded channel verification failed');
    if (synchronize) {
      const active = channels.assets.filter(asset => asset.name === `${channel}.json`);
      if (active.length !== Number(Boolean(old)) || (old && (active[0].id !== old.id
          || active[0].size !== old.size || active[0].updated_at !== old.updated_at
          || !client.downloadAsset(active[0]).equals(currentBytes)))) {
        throw new Error('Current updater channel changed during staging');
      }
    }
    try {
      if (old) client.moveAsset(old, `${channel}-previous-${old.id}.json`);
      if (synchronize && client.releaseByTag(channelTag).assets.some(asset => asset.name === `${channel}.json`)) {
        throw new Error('Another active updater channel appeared before activation');
      }
      client.moveAsset(staged, `${channel}.json`);
    }
    catch (error) {
      const active = client.releaseByTag(channelTag).assets.find(asset => asset.name === `${channel}.json`);
      // A lost API response can follow a successful rename; resolve that outcome first.
      if (!active || !client.downloadAsset(active).equals(contents)) {
        if (old && !active) client.moveAsset(old, `${channel}.json`);
        throw error;
      }
    }
    if (fresh) transport(['release', 'edit', channelTag, '--repo', repository, '--draft=false', '--latest=false']);
    if (synchronize) {
      const readback = client.releaseByTag(channelTag);
      const active = readback.assets.filter(asset => asset.name === `${channel}.json`);
      if (readback.draft || active.length !== 1
          || !client.downloadAsset(active[0]).equals(contents)) throw new Error('Active updater channel readback differs');
    }
    log(`Published ${tag}; advanced ${channel}.json after verifying all four signed updater artifacts.`);
    if (synchronize) return { ...syncReceipt, status: 'synchronized', channel, channel_version: parsed.value };
  } finally {
    if (resolve(staging).startsWith(resolve(tmpdir()) + (process.platform === 'win32' ? '\\' : '/'))
        && basename(staging).startsWith('dezirclip-channel-')) rmSync(staging, { recursive: true, force: true });
  }
}

export function publishOnly(version, config, log = console.log, transport = gh) {
  return publication('publish-only', version, config, log, transport);
}

export function promote(version, config, acceptancePath, log = console.log, transport = gh) {
  return publication('promote', version, config, log, transport, acceptanceBundle(acceptancePath));
}

function synchronizationBundle(runId, runAttempt, upgradePath) {
  if (typeof upgradePath !== 'string' || !upgradePath) throw new Error('An original Windows upgrade gate file is required');
  return { runId, runAttempt, upgradePath, upgradeEvidence: readEvidenceJson(upgradePath) };
}

export function publishAndSync(version, config, runId, runAttempt, upgradePath, log = console.log, transport = gh) {
  return publication('publish-and-sync', version, config, log, transport, synchronizationBundle(runId, runAttempt, upgradePath));
}

export function syncPublished(version, config, runId, runAttempt, upgradePath, log = console.log, transport = gh) {
  return publication('sync-published', version, config, log, transport, synchronizationBundle(runId, runAttempt, upgradePath));
}

export function resumeVerifiedDraft(version, config, runId, runAttempt, upgradePath, log = console.log, transport = gh) {
  return publication('resume-verified-draft', version, config, log, transport, synchronizationBundle(runId, runAttempt, upgradePath));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  preflight(config, pkg);
  const [mode, version, acceptancePath, runAttempt, upgradePath] = process.argv.slice(2);
  if (mode === 'preflight') {
    parseVersion(version);
    if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Release signing secret is not configured');
    console.log('Updater signing configuration is ready.');
  } else if (mode === 'publish-only') publishOnly(version, config);
  else if (['publish-and-sync', 'sync-published', 'resume-verified-draft'].includes(mode) && acceptancePath && runAttempt && upgradePath) {
    const synchronize = mode === 'resume-verified-draft' ? resumeVerifiedDraft
      : mode === 'sync-published' ? syncPublished : publishAndSync;
    console.log(JSON.stringify(synchronize(version, config, acceptancePath, runAttempt, upgradePath, console.error)));
  }
  else if (mode === 'promote' && acceptancePath) {
    promote(version, config, acceptancePath);
  } else throw new Error('Usage: node scripts/updater-release.mjs preflight|publish-only vVERSION; promote vVERSION ACCEPTANCE.json; publish-and-sync|sync-published|resume-verified-draft vVERSION RUN_ID ATTEMPT WINDOWS_UPGRADE.json');
}
