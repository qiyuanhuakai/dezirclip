import { createHash, createPublicKey, verify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, appendFileSync, mkdtempSync, rmSync, lstatSync, realpathSync,
  openSync, closeSync, fstatSync, readSync, constants, copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { rpmBundleConfig } from './bundle-rpm.mjs';
import { compareVersions, parseVersion } from './release-semver.mjs';
import { dualChannelUpdate } from './release-channel.mjs';
import { attachCandidate, candidateBytes, candidateFilename, candidateSealJob, requireCandidateAsset } from './release-candidate.mjs';

export { compareVersions, parseVersion } from './release-semver.mjs';

export const repository = 'qiyuanhuakai/dezirclip';
export const channelTag = 'updater-channels';
export const channelUrl = channel => `https://github.com/${repository}/releases/download/${channelTag}/${channel}.json`;
const expectedPlatforms = {
  'windows-x86_64': 'x64-setup.exe',
  'windows-aarch64': 'arm64-setup.exe',
  'linux-x86_64': 'amd64.AppImage',
  'linux-aarch64': 'aarch64.AppImage',
};
const mygoAsset = (version, platform) => ({
  'windows-x86_64': `DezirClip-v${version}-windows-x64-MyGo-setup.exe`,
  'windows-aarch64': `DezirClip-v${version}-windows-arm64-MyGo-setup.exe`,
  'linux-x86_64': `DezirClip-MyGo_${version}_amd64.AppImage`,
  'linux-aarch64': `DezirClip-MyGo_${version}_aarch64.AppImage`,
})[platform];

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

export function makeManifest(version, assets, readAsset, publicKey, notes = '', dual = false) {
  return assembleManifest(version, notes, dual, (name, normalizedVersion) => {
    for (const required of [name, name + '.sig']) {
      if (assets.filter(asset => asset.name === required && asset.size > 0).length !== 1) {
        throw new Error('Missing or duplicate release asset: ' + required);
      }
    }
    const data = readAsset(name);
    const signature = readAsset(name + '.sig').toString('utf8').trim();
    if (data.length !== assets.find(asset => asset.name === name).size) {
      throw new Error('Release asset size differs: ' + name);
    }
    verifyTauriSignature(data, signature, publicKey, normalizedVersion, name);
    return signature;
  });
}

function assembleManifest(version, notes, dual, readSignature) {
  const parsed = parseVersion(version);
  const root = 'https://github.com/' + repository + '/releases/download/v' + parsed.value + '/';
  function signedAsset(name) {
    return { url: root + encodeURIComponent(name), signature: readSignature(name, parsed.value) };
  }
  const platforms = Object.fromEntries(Object.entries(expectedPlatforms)
    .map(([platform, suffix]) => [platform, signedAsset(`DezirClip_${parsed.value}_${suffix}`)]));
  const manifest = { version: parsed.value, notes, platforms };
  if (dual) manifest.mygo_platforms = Object.fromEntries(Object.keys(expectedPlatforms)
    .map(platform => [platform, signedAsset(mygoAsset(parsed.value, platform))]));
  return manifest;
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
function githubClient(transport, paginateAssets = false) {
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
    if (paginateAssets) {
      const assetPages = JSON.parse(transport(['api',
        `repos/${repository}/releases/${release.id}/assets?per_page=100`, '--paginate', '--slurp']));
      if (!Array.isArray(assetPages) || !assetPages.length || assetPages.some(page => !Array.isArray(page))) {
        throw new Error('GitHub release asset listing is incomplete');
      }
      release.assets = assetPages.flat();
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
const dualDesktopJobs = Object.keys(expectedPlatforms).map(platform => `Dual desktop release (${platform})`);
const dualReleaseJobs = ['Create GitHub release', 'Publish verified standalone archives',
  ...dualDesktopJobs, ...Object.keys(expectedPlatforms).map(platform => `Build compatible standalone CLI / Standalone (${platform})`)];
const archPackagingStep = 'Build Arch Linux packages and AUR recipes';
const candidateSourceJobs = [
  'Frontend', 'Select desktop acceptance targets', 'Bind candidate source',
  ...['linux-x86_64', 'windows-x86_64'].map(name => `Tauri backend (${name})`),
  ...['ubuntu-24.04', 'windows-latest'].flatMap(os => [
    `Go development (${os})`, `Rust development (${os}, core-tests)`, `Rust development (${os}, desktop-check)`,
  ]),
].map(name => `Source acceptance / ${name}`);
const desktopUploadJob = 'Upload verified desktop assets';
const optionalReleaseJobs = [
  'Synchronize an already published update channel', 'Supplement published MyGo installer',
  'Resume verified publication or synchronize update channel', 'Publish the fixed historical recovery',
  'Resume dual-desktop publication or synchronize its channel',
  'Collect actual final package identities', 'Verify actual signed Windows upgrades',
  'Full source acceptance / Published Windows memory comparison', 'Full source acceptance / native-file-drag',
  'Full source acceptance / Optional Windows Surface comparison or crash diagnostics',
  ...['Published Windows memory comparison', 'native-file-drag', 'Compatible standalone CLI', 'Darwin native foundations',
    'Build shared native NSIS fixtures', 'Native NSIS (${{ matrix.scenario }})', 'Verified Flatpak', 'MyGo installer acceptance',
    'Optional Windows Surface comparison or crash diagnostics', 'Windows Surface comparison or crash diagnostics',
    'Diagnose Windows Service startup'].map(name => `Source acceptance / ${name}`),
];

function verifiedReleaseRun(transport, tag, commit, runId, runAttempt, repair = false, dual = false, sealing = false) {
  if (!/^[1-9]\d*$/.test(String(runId)) || !Number.isSafeInteger(Number(runId))
      || !/^[1-9]\d*$/.test(String(runAttempt)) || !Number.isSafeInteger(Number(runAttempt))) {
    throw new Error('Synchronization requires an exact Release run and attempt');
  }
  const api = route => JSON.parse(transport(['api', `repos/${repository}/${route}`]));
  const workflow = api('actions/workflows/release.yml');
  const run = api(`actions/runs/${runId}/attempts/${runAttempt}`);
  const latest = api(`actions/runs/${runId}`);
  const sameRun = value => value?.id === Number(runId)
    && value.workflow_id === workflow.id && value.path === '.github/workflows/release.yml'
    && value.event === 'push' && value.head_branch === tag && value.head_sha === commit
    && value.repository?.full_name === repository && value.head_repository?.full_name === repository;
  const validRun = value => sameRun(value) && value.run_attempt === Number(runAttempt)
    && ((value.status === 'in_progress' && value.conclusion === null)
      || (value.status === 'completed' && (value.conclusion === 'success'
        || repair && (value.conclusion === 'failure' || dual && value.conclusion === 'cancelled'))));
  const historicalAttempt = repair && dual && sameRun(latest)
    && latest.status === 'completed' && Number.isSafeInteger(latest.run_attempt) && latest.run_attempt > Number(runAttempt);
  if (!Number.isSafeInteger(workflow?.id) || workflow.id <= 0 || workflow.path !== '.github/workflows/release.yml'
      || workflow.state !== 'active' || !validRun(run)
      || (!historicalAttempt && (!validRun(latest) || latest.status !== run.status || latest.conclusion !== run.conclusion))) {
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
  const archSteps = dual ? (jobs.find(job => job.name === 'Dual desktop release (linux-x86_64)')?.steps ?? [])
    .filter(step => step.name === archPackagingStep) : [];
  if (archSteps.length > 1 || archSteps.some(step => step.status !== 'completed' || step.conclusion !== 'success')) {
    throw new Error('Arch Linux package verification did not succeed');
  }
  const currentContract = dual && (archSteps.length === 1 || jobs.some(job => candidateSourceJobs.includes(job.name)));
  if (currentContract && archSteps.length === 0) throw new Error('Arch Linux package verification did not execute');
  const windowsJobs = windowsVariants.map(windowsCandidateJob);
  const linuxJobs = linuxTargets.map(linuxRuntimeJob);
  const seal = dual ? jobs.find(job => job.name === candidateSealJob) : undefined;
  const windowsCandidateGate = dual && (seal !== undefined || jobs.some(job => windowsJobs.includes(job.name)));
  const linuxRuntimeGate = dual && (seal !== undefined || jobs.some(job => linuxJobs.includes(job.name)));
  const cancelledPublisher = repair && seal && run.conclusion === 'cancelled';
  if ((seal && !currentContract) || (historicalAttempt && (!seal || run.status !== 'completed')) || (run.conclusion === 'cancelled' && !cancelledPublisher)
      || (sealing && (run.status !== 'in_progress' || seal?.status !== 'in_progress' || seal.conclusion !== null))) {
    throw new Error('The release has no eligible candidate seal');
  }
  const requiredJobs = dual
    ? [...(currentContract ? [...dualReleaseJobs, desktopUploadJob, ...candidateSourceJobs] : dualReleaseJobs),
      ...(windowsCandidateGate ? windowsJobs : []), ...(linuxRuntimeGate ? linuxJobs : [])]
    : [...releaseGateJobs, ...nativeReleaseJobs];
  const publishers = jobs.filter(job => publisherJobs.includes(job.name));
  if ((sealing ? publishers.length > 1 : publishers.length !== 1)
      || (!dual && requiresMyGo(tag) && jobs.filter(job => job.name === mygoJob).length !== 1)
      || (!dual && requiresFullSourceAcceptance(tag) && fullSourceJobs.some(name => !jobs.some(job => job.name === name && job.conclusion === 'success')))
      || jobs.some(job => !(job.status === 'completed' && job.conclusion === 'success')
      && !(optionalReleaseJobs.includes(job.name)
        && job.status === 'completed' && job.conclusion === 'skipped')
      && !(sealing && job === seal)
      && !(sealing && job === publishers[0] && ['queued', 'waiting', 'requested', 'pending'].includes(job.status))
      && !(repair && job === publishers[0] && job.status === 'completed' && job.conclusion === 'failure')
      && !(cancelledPublisher && job === publishers[0] && job.status === 'completed' && ['cancelled', 'skipped'].includes(job.conclusion))
      && !(job === publishers[0] && job.status === 'in_progress' && job.conclusion === null))
      || (run.status === 'completed' && publishers[0].status !== 'completed')
      || (run.conclusion === 'failure' && publishers[0].conclusion !== 'failure')
      || (cancelledPublisher && !['cancelled', 'skipped'].includes(publishers[0].conclusion))
      || (run.conclusion === 'success' && publishers[0].conclusion !== 'success')
      || requiredJobs.some(name => !jobs.some(job => job.name === name && job.status === 'completed' && job.conclusion === 'success'))) {
    throw new Error('Release native identity/upgrade gates or another job have not succeeded');
  }
  const uploaders = [desktopUploadJob, 'Publish verified standalone archives'];
  const identities = seal ? jobs.filter(job => uploaders.includes(job.name))
    : dual ? jobs.filter(job => dualReleaseJobs.includes(job.name) || job.name === desktopUploadJob) : Object.keys(expectedPlatforms)
    .map(platform => jobs.find(job => job.name === `Collect actual final package identities / Release identities (${platform})`));
  const times = identities.map(job => Date.parse(dual ? job.completed_at : job.started_at));
  if (times.some(time => !Number.isFinite(time))) {
    throw new Error('Native release identity times are missing');
  }
  return { run_id: Number(runId), run_attempt: Number(runAttempt), workflow_id: workflow.id,
    status: run.status, conclusion: run.conclusion, asset_cutoff: dual ? Math.max(...times) : Math.min(...times),
    ...(dual ? { run_started_at: run.run_started_at, arch_packages: archSteps.length === 1, source_acceptance: currentContract,
      windows_candidate_gate: windowsCandidateGate, linux_runtime_gate: linuxRuntimeGate,
      ...(seal ? { seal: { id: seal.id, started_at: seal.started_at, completed_at: sealing ? null : seal.completed_at },
        product_cutoffs: Object.fromEntries(uploaders.map(name => [name, Date.parse(jobs.find(job => job.name === name).completed_at)])) } : {}) } : {}),
    jobs: jobs.map(job => ({ id: job.id, name: job.name, status: job.status, conclusion: job.conclusion,
      started_at: job.started_at, ...(seal ? { completed_at: job.completed_at } : {}) })).sort((a, b) => a.id - b.id) };
}

export const desktopReleaseTargets = Object.freeze(Object.keys(expectedPlatforms));

export function expectedDesktopCandidateFiles(version, target, archPackages = true) {
  const { value } = parseVersion(version);
  if (!desktopReleaseTargets.includes(target)) throw new Error("Invalid desktop release target");
  const signed = [`DezirClip_${value}_${expectedPlatforms[target]}`, mygoAsset(value, target)];
  const names = [];
  if (target.startsWith('linux-')) {
    const amd64 = target === 'linux-x86_64';
    const debArch = amd64 ? 'amd64' : 'arm64';
    const rpmArch = amd64 ? 'x86_64' : 'aarch64';
    signed.push(`DezirClip_${value}_${debArch}.deb`, `DezirClip-MyGo_${value}_${debArch}.deb`);
    const rpm = rpmBundleConfig(value);
    const rpmSuffix = `${rpm.version}-${rpm.bundle.linux.rpm.release}.${rpmArch}.rpm`;
    names.push(`DezirClip-${rpmSuffix}`, `DezirClip-MyGo-${rpmSuffix}`);
    if (amd64 && archPackages) {
      const match = /^([0-9]+\.[0-9]+\.[0-9]+)(?:-([a-z]+)\.?([0-9]+))?$/.exec(value);
      if (!match) throw new Error('Invalid Arch Linux package version');
      const pkgver = match.slice(1).join('');
      names.push(...['dezirclip-bin', 'dezirclip-mygo-bin'].flatMap(name =>
        [`${name}-${pkgver}-1-x86_64.pkg.tar.zst`, `${name}-${pkgver}-1.src.tar.gz`]));
    }
  }
  return [...names, ...signed.flatMap(name => [name, `${name}.sig`])];
}

const desktopBuildReceipt = 'desktop-build.json';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');

function localReleaseAsset(path) {
  const stat = lstatSync(path);
  if (!stat.isFile() || !stat.size) throw new Error(`Invalid release asset: ${basename(path)}`);
  const bytes = readFileSync(path);
  if (bytes.length !== stat.size) throw new Error(`Release asset changed while reading: ${basename(path)}`);
  return { name: basename(path), size: bytes.length, sha256: digest(bytes) };
}

function requireExactNames(actual, expected, context) {
  if (actual.length !== expected.length || new Set(actual).size !== expected.length ||
      expected.some(name => !actual.includes(name))) {
    throw new Error(`Release asset inventory differs: ${context}`);
  }
}

export function stageDesktopCandidate({ tag, source, target, runId, attempt, destination, files }) {
  if (!/^v[0-9]/.test(tag) || !/^[a-f0-9]{40}$/.test(source) ||
      !/^[1-9][0-9]*$/.test(runId) || !/^[1-9][0-9]*$/.test(attempt)) {
    throw new Error("Invalid desktop build provenance");
  }
  const expected = expectedDesktopCandidateFiles(tag, target);
  requireExactNames(files.map(file => basename(file)), expected, target);
  const inputs = files.map(file => ({ file, ...localReleaseAsset(file) }));
  mkdirSync(destination);
  const assets = inputs.map(({ file, name, size, sha256 }) => {
    const staged = join(destination, name);
    copyFileSync(file, staged, constants.COPYFILE_EXCL);
    const asset = localReleaseAsset(staged);
    if (asset.size !== size || asset.sha256 !== sha256) throw new Error(`Desktop staging changed ${name}`);
    return asset;
  });
  writeFileSync(join(destination, desktopBuildReceipt), JSON.stringify({
    schema_version: 1, tag, source_revision: source, target, run_id: runId, run_attempt: attempt, assets,
  }, null, 2) + '\n', { flag: 'wx' });
}

export function readDesktopCandidates({ tag, source, runId, attempt, directory }) {
  const names = desktopReleaseTargets.map(target => `release-desktop-${target}-${attempt}`);
  requireExactNames(readdirSync(directory), names, 'producer artifacts');
  return desktopReleaseTargets.flatMap((target, index) => readDesktopCandidate({
    tag, source, runId, attempt, target, directory: join(directory, names[index]),
  }));
}

function readDesktopCandidate({ tag, source, runId, attempt, target, directory }) {
  if (!lstatSync(directory).isDirectory()) throw new Error(`Invalid desktop producer directory: ${target}`);
  const expected = expectedDesktopCandidateFiles(tag, target);
  requireExactNames(readdirSync(directory), [...expected, desktopBuildReceipt], target);
  const { value: receipt } = readEvidenceJson(join(directory, desktopBuildReceipt));
  if (receipt.schema_version !== 1 || receipt.tag !== tag || receipt.source_revision !== source ||
      receipt.target !== target || receipt.run_id !== runId || receipt.run_attempt !== attempt || !Array.isArray(receipt.assets)) {
    throw new Error(`Desktop receipt has the wrong provenance: ${target}`);
  }
  requireExactNames(receipt.assets.map(asset => asset.name), expected, target);
  return receipt.assets.map(asset => {
    const path = join(directory, asset.name);
    const actual = localReleaseAsset(path);
    if (actual.size !== asset.size || actual.sha256 !== asset.sha256) throw new Error(`Desktop asset digest differs: ${asset.name}`);
    return { ...actual, path, target };
  });
}

function candidateArtifactIds({ source, runId, attempt }, nameForTarget, transport, targets = desktopReleaseTargets) {
  if (!/^[1-9][0-9]*$/.test(attempt)) throw new Error("Invalid candidate artifact provenance");
  return candidateArtifacts(source, runId, targets.map(target => nameForTarget(target, attempt)), transport)
    .map(artifact => artifact.id);
}

function candidateArtifacts(source, runId, names, transport) {
  if (!/^[a-f0-9]{40}$/.test(source) || !/^[1-9][0-9]*$/.test(runId) ||
      new Set(names).size !== names.length) throw new Error("Invalid candidate artifact provenance");
  const pages = JSON.parse(transport(['api', `repos/${repository}/actions/runs/${runId}/artifacts?per_page=100`,
    '--paginate', '--slurp']));
  if (!Array.isArray(pages) || !pages.length || pages.some(page => !Array.isArray(page.artifacts))) {
    throw new Error("Incomplete candidate artifact listing");
  }
  const artifacts = pages.flatMap(page => page.artifacts);
  const selected = names.map(requested => {
    const matches = artifacts.filter(artifact => artifact.name === requested);
    const [artifact] = matches;
    if (matches.length !== 1 || !Number.isSafeInteger(artifact.id) || artifact.id <= 0 || artifact.expired !== false ||
        artifact.workflow_run?.id !== Number(runId) || artifact.workflow_run.head_sha !== source ||
        artifact.workflow_run.head_repository_id !== artifact.workflow_run.repository_id ||
        !/^sha256:[a-f0-9]{64}$/.test(artifact.digest ?? '')) {
      throw new Error(`Producer artifact is missing or does not identify the candidate: ${requested}`);
    }
    return artifact;
  });
  if (new Set(selected.map(artifact => artifact.id)).size !== names.length) {
    throw new Error("Candidate artifact IDs are not unique");
  }
  return selected;
}

export function desktopArtifactIds(candidate, transport = gh) {
  return candidateArtifactIds(candidate, desktopProducerArtifact, transport);
}

export function standaloneArtifactIds(candidate, transport = gh) {
  return candidateArtifactIds(candidate, (target, attempt) => `release-cli-${attempt}-${target}`, transport);
}

const windowsX64 = 'windows-x86_64';
const windowsVariants = ['tauri', 'mygo'];
const windowsCandidateJob = variant => `Verify final Windows installer (${variant})`;
const publishedDesktopBaselines = {
  'v0.2.0': { source: 'ba0ad71f7b9d8caddac45d0bc5588254a8273482', releaseId: 460242002,
    tauri: [799308519, 799308514, 'e9406e6da0ad5cbe64d1f45209ac82e34d8161d5d6ce2da50737f835c83e25dd'],
    mygo: [799308522, 799308518, 'f7d408f94eec8118767457db90c43ef5e232ae0b6cce63d8c80c0704c33fca92'] },
  'v0.2.0-rc5': { source: '026c931a06656a4e9a40ef28360acf13564c9f03', releaseId: 460224068,
    tauri: [799245488, 799245496, '2d20646581c453ba86d7954c34f6710d9d416a9c49065a1715e3eb038bc40202'],
    mygo: [799245492, 799245493, '1ca4ee2050edf7afd858d1194d2f30e1e12d657d6faf01b97423e03c04dd93fd'] },
};

function windowsInstaller(tag, variant) {
  const { value } = parseVersion(tag);
  return variant === 'mygo' ? mygoAsset(value, windowsX64) : `DezirClip_${value}_x64-setup.exe`;
}

function publishedWindowsInstaller(tag, variant, publicKey, transport) {
  const pinned = publishedDesktopBaselines[tag];
  const [id, signatureId, sha256] = pinned[variant];
  const name = windowsInstaller(tag, variant);
  const client = githubClient(transport, true);
  const release = client.releaseByTag(tag);
  if (release.draft || release.id !== pinned.releaseId || client.releaseCommit(tag) !== pinned.source) {
    throw new Error(`Historical Windows installer release differs: ${tag}`);
  }
  const find = (id, name) => {
    const asset = release.assets.find(item => item.id === id && item.name === name && item.state === 'uploaded');
    if (!asset) throw new Error(`Historical Windows asset missing: ${name}`);
    return asset;
  };
  const bytes = client.downloadAsset(find(id, name));
  const signature = client.downloadAsset(find(signatureId, `${name}.sig`));
  if (digest(bytes) !== sha256) throw new Error(`Historical Windows installer bytes differ: ${name}`);
  verifyTauriSignature(bytes, signature.toString('utf8'), publicKey, tag, name);
  return { name, tag, source_revision: pinned.source, sha256, bytes };
}

export function stageWindowsCandidate({ tag, source, runId, attempt, directory, artifactId, variant, destination, publicKey, replay },
  transport = gh) {
  if (!windowsVariants.includes(variant)) throw new Error('Invalid Windows desktop variant');
  let current;
  if (replay) {
    tag = 'v0.2.0';
    current = publishedWindowsInstaller(tag, variant, publicKey, transport);
    source = current.source_revision;
    artifactId = null;
  } else {
    const [id] = candidateArtifactIds({ source, runId, attempt },
      (target, attempt) => `release-desktop-${target}-${attempt}`, transport, [windowsX64]);
    if (String(id) !== artifactId || compareVersions(tag, 'v0.2.0') <= 0) throw new Error('Invalid Windows candidate provenance or version');
    const assets = readDesktopCandidate({ tag, source, runId, attempt, target: windowsX64, directory });
    const name = windowsInstaller(tag, variant);
    const installer = assets.find(asset => asset.name === name);
    const bytes = readFileSync(installer.path);
    if (digest(bytes) !== installer.sha256) throw new Error('Candidate installer changed during staging');
    verifyTauriSignature(bytes, readFileSync(assets.find(asset => asset.name === `${name}.sig`).path, 'utf8'), publicKey, tag, name);
    current = { name, tag, source_revision: source, sha256: installer.sha256, bytes };
  }
  const previous = publishedWindowsInstaller(replay ? 'v0.2.0-rc5' : 'v0.2.0', variant, publicKey, transport);
  mkdirSync(destination);
  const write = ({ bytes, ...identity }) => {
    writeFileSync(join(destination, identity.name), bytes, { flag: 'wx' });
    return identity;
  };
  writeFileSync(join(destination, 'windows-candidate-inputs.json'), JSON.stringify({
    schema_version: 1, origin: replay ? 'published-replay' : 'release-producer', target: windowsX64, variant,
    source_revision: source, tag, run_id: Number(runId), run_attempt: Number(attempt), producer_artifact_id: artifactId,
    current: write(current), previous: write(previous),
  }), { flag: 'wx' });
}

function verifyWindowsCandidateReports(reports, desktop, tag, source, run) {
  if (!Array.isArray(reports)) throw new Error('Missing Windows candidate reports');
  requireExactNames(reports.map(report => report?.variant), windowsVariants, 'Windows candidate reports');
  const baselines = publishedDesktopBaselines['v0.2.0'];
  const producerIds = new Set();
  for (const { schema_version, status, origin, target, variant, source_revision, tag: testedTag, run_id, run_attempt,
    producer_artifact_id, current, previous, members } of reports) {
    const name = windowsInstaller(tag, variant);
    const archive = desktop.archives.find(item => item.name === name);
    if (schema_version !== 1 || status !== 'passed' || origin !== 'release-producer' || target !== windowsX64 ||
        testedTag !== tag || source_revision !== source || run_id !== run.run_id || run_attempt !== run.run_attempt ||
        !/^[1-9][0-9]*$/.test(producer_artifact_id ?? '') || current?.name !== name || current.tag !== tag ||
        current.source_revision !== source || current.sha256 !== archive?.sha256 ||
        previous?.tag !== 'v0.2.0' || previous.source_revision !== baselines.source ||
        previous.name !== windowsInstaller(previous.tag, variant) || previous.sha256 !== baselines[variant][2] ||
        !members || Object.keys(members).length !== 3 ||
        ['desktop', 'cli', 'service'].some(role => members[role]?.path !== archive?.members[role]?.path ||
          members[role]?.sha256 !== archive?.members[role]?.sha256)) {
      throw new Error(`Windows candidate report does not match the final signed installer: ${name}`);
    }
    producerIds.add(producer_artifact_id);
  }
  if (producerIds.size !== 1) throw new Error('Windows variants must come from the same producer artifact');
}

const linuxTargets = desktopReleaseTargets.filter(target => target.startsWith('linux-'));
const linuxRuntimeJob = target => `Verify final Linux desktop runtime (${target})`;
const linuxRuntimeArtifact = (target, attempt) => `linux-runtime-${target}-${attempt}`;
const desktopProducerArtifact = (target, attempt) => `release-desktop-${target}-${attempt}`;
const linuxRuntimeFiles = (tag, target, archPackages) => expectedDesktopCandidateFiles(tag, target, archPackages)
  .filter(name => !name.endsWith('.sig') && !name.endsWith('.src.tar.gz'));
const linuxReplayTag = 'v0.2.0';
// The RPMs have no detached updater signatures. Pin the actual released bytes
// for the read-only exercise alongside the signed Debian and AppImage files.
const linuxInstallerHashes = {
  'DezirClip-0.2.0-1.x86_64.rpm': 'd55291474b2cbba4e14a3fd2450b8983adba5209de7848b8fffba47f5c58f03d',
  'DezirClip-MyGo-0.2.0-1.x86_64.rpm': '8eff2b7525e80ab001cf740ce5f9248774542734994f6522356b3d1c74f772d5',
  'DezirClip-0.2.0-1.aarch64.rpm': '82af26d4faf6aeb3cd1034ce3c5397e9b7f0350d58599e2d13d6fd8b241fb3f0',
  'DezirClip-MyGo-0.2.0-1.aarch64.rpm': '82af6ace94986fe674605fd062aec17ac7a3492055147deacd9c057ff16a6586',
  'DezirClip_0.2.0_amd64.deb': 'eca045be38707b4b99a1433f6e4287f6705fba5e75c572f038b1f3bb823d6b9c',
  'DezirClip-MyGo_0.2.0_amd64.deb': '560c6209445cca15ab83ab3d47943e82922398cf47ecac7786dd99b741dde3ba',
  'DezirClip_0.2.0_arm64.deb': 'b1e48decdb152ed024267a064aba079b5b2be086cfc62dd12ac8a693ded4167c',
  'DezirClip-MyGo_0.2.0_arm64.deb': '4442352b30c4e8e9639404e384c2cfd2cfaf1d4427544da0175e65d86dd0207c',
  'DezirClip-0.2.0-0.rc5.x86_64.rpm': '1342b5da0d9d11f1a58fce35cf7daa3c995971d02051f9b971da121875d9e6d9',
  'DezirClip-MyGo-0.2.0-0.rc5.x86_64.rpm': '955e27d9e81f700c2c1cdf444917a073f0bb27c3a610f2854b67067afbf819f3',
  'DezirClip-0.2.0-0.rc5.aarch64.rpm': '5a1ca6237d6ceba69926a1cf4bd21bb82425f0563c1a41146f361bbba45c8dca',
  'DezirClip-MyGo-0.2.0-0.rc5.aarch64.rpm': 'fbd0284b6e930ba0698355d635c77546272d5354e7634a6841da48d334b04658',
  'DezirClip_0.2.0-rc5_amd64.deb': '5bc1b57579404101cf930c188684977c6f4ea34f123a5d0312a7673195d75e6f',
  'DezirClip-MyGo_0.2.0-rc5_amd64.deb': 'a97c39620c59ce332b4b4db0953262ef671619696ca5f859842ab47fb69e054d',
  'DezirClip_0.2.0-rc5_arm64.deb': '0f5020c812bef2a0bb15fd33b62d04c4ddac22367acc41c0f492c4fa52de5984',
  'DezirClip-MyGo_0.2.0-rc5_arm64.deb': '661b5181d20e0f4d583c3067fdbc12ce7b314cc03df80ff20c13ce7aed489d2a',
};

function verifyDesktopSignatures(tag, assets, publicKey) {
  const byName = new Map(assets.map(asset => [asset.name, asset]));
  for (const { name, path } of assets.filter(asset => asset.name.endsWith('.sig'))) {
    const signedName = name.slice(0, -4);
    verifyTauriSignature(readFileSync(byName.get(signedName).path), readFileSync(path, 'utf8'), publicKey, tag, signedName);
  }
}

const linuxManagedPackage = name => name.endsWith('.deb') || name.endsWith('.rpm');
const linuxArchPackage = name => name.endsWith('.pkg.tar.zst');
const linuxInstalledPackage = name => linuxManagedPackage(name) || linuxArchPackage(name);
const linuxManager = name => name.endsWith('.deb') ? 'dpkg' : linuxArchPackage(name) ? 'pacman' : 'rpm';
const linuxDesktopVariant = name => name.startsWith('DezirClip-MyGo') || name.startsWith('dezirclip-mygo-bin-') ? 'mygo' : 'tauri';
const linuxInstallerKind = name => linuxDesktopVariant(name) + '-' + linuxManager(name);

function stagePublishedLinuxAssets(tag, names, destination, publicKey, cache, transport) {
  const pinned = publishedDesktopBaselines[tag];
  const client = githubClient(transport, true);
  const release = client.releaseByTag(tag);
  if (release.draft !== false || release.id !== pinned.releaseId || client.releaseCommit(tag) !== pinned.source) {
    throw new Error('Historical Linux release identity differs');
  }
  const assets = names.map(name => {
    const found = release.assets.filter(item => item.name === name);
    const [item] = found;
    if (found.length !== 1 || item.state !== 'uploaded' || !Number.isSafeInteger(item.id) || item.id <= 0 ||
        !/^sha256:[a-f0-9]{64}$/.test(item.digest ?? '')) {
      throw new Error('Historical Linux release asset is absent or ambiguous: ' + name);
    }
    const bytes = cache ? readFileSync(join(cache, name)) : client.downloadAsset(item);
    const sha256 = digest(bytes);
    if (bytes.length !== item.size || item.digest !== 'sha256:' + sha256 ||
        (linuxManagedPackage(name) && linuxInstallerHashes[name] !== sha256)) {
      throw new Error('Historical Linux release asset has different bytes: ' + name);
    }
    const path = join(destination, name);
    writeFileSync(path, bytes, { flag: 'wx' });
    return { name, path, size: item.size, sha256, asset_id: item.id };
  });
  verifyDesktopSignatures(tag, assets, publicKey);
  return assets;
}

export function stageLinuxRuntime({ tag, source, runId, attempt, target, directory, artifactId, destination, publicKey, cache },
  transport = gh) {
  if (!linuxTargets.includes(target) || !/^[1-9][0-9]*$/.test(runId) || !/^[1-9][0-9]*$/.test(attempt)) {
    throw new Error('Invalid Linux runtime candidate identity');
  }
  const replay = !directory;
  mkdirSync(destination);
  let assets;
  if (replay) {
    tag = linuxReplayTag;
    source = publishedDesktopBaselines[tag].source;
    artifactId = null;
    assets = stagePublishedLinuxAssets(tag, expectedDesktopCandidateFiles(tag, target, false),
      destination, publicKey, cache, transport);
  } else {
    const [id] = candidateArtifactIds({ source, runId, attempt },
      desktopProducerArtifact, transport, [target]);
    if (String(id) !== artifactId || compareVersions(tag, linuxReplayTag) <= 0) {
      throw new Error('Linux runtime verifier received a different producer artifact or an invalid upgrade version');
    }
    assets = readDesktopCandidate({ tag, source, runId, attempt, target, directory }).map(asset => {
      const path = join(destination, asset.name);
      copyFileSync(asset.path, path, constants.COPYFILE_EXCL);
      const copy = localReleaseAsset(path);
      if (copy.sha256 !== asset.sha256 || copy.size !== asset.size) throw new Error('Linux runtime candidate changed during staging');
      return { ...copy, path };
    });
  }
  if (!replay) verifyDesktopSignatures(tag, assets, publicKey);
  const packages = assets.filter(({ name }) => !name.endsWith('.sig') && !name.endsWith('.src.tar.gz'))
    .map(({ path, ...identity }) => identity);
  const receipt = {
    schema_version: 1, origin: replay ? 'published-replay' : 'release-producer', tag, source_revision: source,
    run_id: Number(runId), run_attempt: Number(attempt), producer_artifact_id: artifactId, target, packages,
  };
  const baselineTag = replay ? 'v0.2.0-rc5' : linuxReplayTag;
  const baselineNames = expectedDesktopCandidateFiles(baselineTag, target, false)
    .filter(name => linuxManagedPackage(name) || name.endsWith('.deb.sig'));
  const previous = new Map(stagePublishedLinuxAssets(baselineTag, baselineNames, destination, publicKey, cache, transport)
    .filter(asset => linuxManagedPackage(asset.name))
    .map(({ path, ...asset }) => [linuxInstallerKind(asset.name),
      { ...asset, tag: baselineTag, source_revision: publishedDesktopBaselines[baselineTag].source }]));
  const transactions = packages.filter(asset => linuxManagedPackage(asset.name)).map(asset => ({
    current: { ...asset, tag, source_revision: source },
    previous: previous.get(linuxInstallerKind(asset.name)),
  }));
  writeFileSync(join(destination, 'linux-package-transactions-inputs.json'),
    JSON.stringify({ target, packages: transactions }, null, 2) + '\n', { flag: 'wx' });
  writeFileSync(join(destination, 'linux-runtime-inputs.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
}

function sameLinuxMembers(installed, expected) {
  return installed && expected && Object.keys(installed).length === 3 && Object.keys(expected).length === 3 &&
    ['desktop', 'cli', 'service'].every(role => typeof expected[role]?.path === 'string' &&
      /^[0-9a-f]{64}$/.test(expected[role]?.sha256 ?? '') &&
      installed[role]?.path === expected[role].path && installed[role]?.sha256 === expected[role].sha256);
}

function verifyArchTransactionBaseline(transaction, archBaseline, debTransaction, debBaseline) {
  const previous = transaction.previous;
  const deb = previous?.debian_source;
  const checked = debTransaction?.previous;
  if (!deb || !checked || previous.name !== archBaseline || !/^[0-9a-f]{64}$/.test(previous.sha256 ?? '') ||
      deb.name !== debBaseline || deb.sha256 !== linuxInstallerHashes[debBaseline] ||
      deb.tag !== previous.tag || deb.source_revision !== previous.source_revision ||
      deb.name !== checked.name || deb.sha256 !== checked.sha256 ||
      deb.size !== checked.size || deb.asset_id !== checked.asset_id ||
      !Number.isSafeInteger(deb.size) || deb.size <= 0 || !Number.isSafeInteger(deb.asset_id) || deb.asset_id <= 0 ||
      !sameLinuxMembers(transaction.previous_members, debTransaction.previous_members)) {
    throw new Error('Arch Linux baseline does not match the installed signed Debian package: ' + transaction.current.name);
  }
}

function verifyLinuxRuntimeReports(reports, desktop, tag, source, run, transport) {
  if (!Array.isArray(reports)) throw new Error('Missing Linux runtime reports');
  requireExactNames(reports.map(report => report?.target), linuxTargets, 'Linux runtime reports');
  const producerIds = candidateArtifactIds({ source, runId: String(run.run_id), attempt: String(run.run_attempt) },
    desktopProducerArtifact, transport, linuxTargets);
  const archives = new Map(desktop.archives.map(archive => [archive.name, archive]));
  const assets = new Map(desktop.assets.map(asset => [asset.name, asset]));
  for (const report of reports) {
    const { target, packages } = report;
    if (report.schema_version !== 1 || report.status !== 'passed' || report.origin !== 'release-producer' ||
        report.tag !== tag || report.source_revision !== source || report.run_id !== run.run_id ||
        report.run_attempt !== run.run_attempt || report.producer_artifact_id !== String(producerIds[linuxTargets.indexOf(target)]) ||
        !Array.isArray(packages)) {
      throw new Error(`Linux runtime report does not identify the candidate: ${target}`);
    }
    const names = linuxRuntimeFiles(tag, target, run.arch_packages);
    requireExactNames(packages.map(asset => asset?.name), names, target);
    const transactions = report.package_transactions;
    if (!Array.isArray(transactions)) throw new Error('Missing Linux package-manager transaction results');
    requireExactNames(transactions.map(item => item?.current?.name), names.filter(linuxInstalledPackage), 'Linux package transactions');
    const transactionsByName = new Map(transactions.map(item => [item.current.name, item]));
    const currentByKind = new Map(names.filter(linuxInstalledPackage).map(name => [linuxInstallerKind(name), name]));
    const baselineByKind = new Map(linuxRuntimeFiles(linuxReplayTag, target, run.arch_packages).filter(linuxInstalledPackage)
      .map(name => [linuxInstallerKind(name), name]));
    for (const item of packages) {
      const archive = archives.get(item.name);
      const asset = assets.get(item.name);
      const runtime = item.name.endsWith('.AppImage') ? 'appimage' : 'extracted-native-launcher';
      if (archive?.target !== target || item.size !== asset?.size || item.sha256 !== archive.sha256 ||
          item.runtime !== runtime || item.history_rendered !== true || item.service_verified !== true ||
          !item.members || Object.keys(item.members).length !== 3 ||
          ['desktop', 'cli', 'service'].some(role => item.members[role]?.path !== archive.members[role].path ||
            item.members[role]?.sha256 !== archive.members[role].sha256)) {
        throw new Error(`Linux runtime report does not match the final package: ${item.name}`);
      }
      if (!linuxInstalledPackage(item.name)) continue;
      const transaction = transactionsByName.get(item.name);
      const { current, previous, members, package_manager, installed, upgraded, reinstalled, uninstalled, profile_retained } = transaction;
      const baseline = baselineByKind.get(linuxInstallerKind(item.name));
      const arch = linuxArchPackage(item.name);
      if (package_manager !== linuxManager(item.name) || current.tag !== tag || current.source_revision !== source ||
          current.sha256 !== item.sha256 || current.size !== item.size || Object.hasOwn(current, 'debian_source') ||
          previous?.name !== baseline || previous.tag !== linuxReplayTag ||
          previous.source_revision !== publishedDesktopBaselines[linuxReplayTag].source ||
          (!arch && previous.sha256 !== linuxInstallerHashes[baseline]) ||
          ![installed, upgraded, reinstalled, uninstalled, profile_retained].every(value => value === true) ||
          !sameLinuxMembers(members, item.members)) {
        throw new Error('Linux package transaction did not verify the final bytes and historical upgrade: ' + item.name);
      }
      if (arch) {
        const debKind = linuxDesktopVariant(item.name) + '-dpkg';
        verifyArchTransactionBaseline(transaction, baseline, transactionsByName.get(currentByKind.get(debKind)),
          baselineByKind.get(debKind));
      }
    }
  }
}

function standaloneArchiveName(version, target) {
  return `dzc-${version}-${target}.${target.startsWith('windows-') ? 'zip' : 'tar.gz'}`;
}

export function readStandaloneCandidates(tag, directory) {
  const { value } = parseVersion(tag);
  const archives = desktopReleaseTargets.map(target => standaloneArchiveName(value, target));
  requireExactNames(readdirSync(directory), archives.flatMap(name => [name, `${name}.sha256`]), 'standalone archives');
  return archives.flatMap(name => {
    const path = join(directory, name);
    const checksum = `${path}.sha256`;
    const archive = { ...localReleaseAsset(path), path };
    const companion = { ...localReleaseAsset(checksum), path: checksum };
    const contents = readFileSync(checksum, 'utf8').replace(/\r\n$/, '\n');
    if (contents !== `${archive.sha256}  ${name}\n`) {
      throw new Error(`Standalone archive digest differs: ${name}`);
    }
    return [archive, companion];
  });
}

export function attachDesktopCandidates(candidate, transport = gh, examine) {
  const assets = readDesktopCandidates(candidate);
  examine?.(assets);
  return attachReleaseFiles(candidate, assets, transport);
}

function examineDesktopArchives(tag, assets, publicKey) {
  const { value } = parseVersion(tag);
  const byName = new Map(assets.map(asset => [asset.name, asset]));
  verifyDesktopSignatures(tag, assets, publicKey);
  const archives = assets.filter(asset => !asset.name.endsWith('.sig'));
  const inspection = JSON.parse(execFileSync('python3',
    ['scripts/release_identity_packages.py', '--candidate-version', value],
    { input: JSON.stringify(archives), encoding: 'utf8', timeout: 120_000 }));
  const targets = new Map(archives.map(asset => [asset.name, asset.target]));
  requireArchiveReadback(inspection, targets, byName);
  return inspection;
}

function requireArchiveReadback(inspection, targets, assets) {
  if (!Array.isArray(inspection)) throw new Error('Missing desktop package inspection');
  requireExactNames(inspection.map(archive => archive?.name), [...targets.keys()], 'desktop package inspection');
  for (const archive of inspection) {
    const roles = archive.name.endsWith('.src.tar.gz') ? [] : ['desktop', 'cli', 'service'];
    const asset = assets.get(archive.name);
    if (!asset || archive.target !== targets.get(archive.name) || archive.sha256 !== asset.sha256 ||
        !archive.members || typeof archive.members !== 'object' || Array.isArray(archive.members)) {
      throw new Error(`Package inspection differs: ${archive.name}`);
    }
    requireExactNames(Object.keys(archive.members), roles, archive.name);
    for (const member of Object.values(archive.members)) {
      if (typeof member?.path !== 'string' || !/^[a-f0-9]{64}$/.test(member.sha256 ?? '')) {
        throw new Error(`Invalid packaged executable identity: ${archive.name}`);
      }
    }
  }
}

function attachReleaseFiles({ tag, source, releaseId }, assets, transport) {
  const client = githubClient(transport, true);
  const assertDraft = () => {
    const release = client.releaseByTag(tag);
    if (String(release.id) !== releaseId || release.draft !== true || client.releaseCommit(tag) !== source) {
      throw new Error("The original draft no longer identifies this candidate");
    }
    return release;
  };
  const matching = (release, expected) => {
    const matches = release.assets.filter(asset => asset.name === expected.name);
    if (!matches.length) return null;
    const [asset] = matches;
    if (matches.length !== 1 || asset.state !== 'uploaded' || asset.size !== expected.size ||
        asset.digest !== `sha256:${expected.sha256}` || !Number.isSafeInteger(asset.id) || asset.id <= 0) {
      throw new Error(`Release draft conflicts with the candidate: ${expected.name}`);
    }
    return asset;
  };
  const draft = assertDraft();
  const missing = assets.filter(asset => !matching(draft, asset)).map(asset => asset.path);
  if (missing.length) transport(['release', 'upload', tag, '--repo', repository, ...missing]);
  const uploaded = assertDraft();
  return assets.map(asset => {
    const remote = matching(uploaded, asset);
    if (!remote || digest(client.downloadAsset(remote)) !== asset.sha256) {
      throw new Error(`Uploaded product bytes differ: ${asset.name}`);
    }
    return { id: remote.id, name: asset.name, size: asset.size, sha256: asset.sha256 };
  });
}

function verifiedReleaseAssets(release, version, cutoff, dual = false, archPackages = false) {
  const { value } = parseVersion(version);
  const rpm = rpmBundleConfig(value);
  const rpmVersion = rpm.version, rpmRelease = rpm.bundle.linux.rpm.release;
  const updater = Object.values(expectedPlatforms).map(suffix => `DezirClip_${value}_${suffix}`);
  const cli = desktopReleaseTargets.map(target => standaloneArchiveName(value, target));
  const deb = ['amd64', 'arm64'].map(arch => `DezirClip_${value}_${arch}.deb`);
  const names = cli.flatMap(name => [name, `${name}.sha256`]);
  if (dual) {
    names.push(...desktopReleaseTargets.flatMap(target => expectedDesktopCandidateFiles(value, target, archPackages)));
  } else {
    names.push(...updater.flatMap(name => [name, `${name}.sig`]), ...deb.flatMap(name => [name, `${name}.sig`]),
      ...['x86_64', 'aarch64'].map(arch => `DezirClip-${rpmVersion}-${rpmRelease}.${arch}.rpm`));
    names.push(...['x86_64', 'aarch64'].map(arch => `DezirClip-${value}-linux-${arch}.flatpak`));
    if (requiresMyGo(value)) names.push(`DezirClip-v${value}-windows-x64-MyGo${value === '0.2.0-alpha.9' ? '.zip' : '-setup.exe'}`);
  }
  let assets = release.assets;
  // alpha.9's original gate predates its separately tested installer supplement.
  // That attachment is outside the signed updater set and original time cutoff.
  if (!dual && value === '0.2.0-alpha.9' && !release.draft && Array.isArray(assets)) {
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

function verifiedProducts(release, tag, run, dual) {
  const assets = run.seal ? release.assets.filter(asset => asset.name !== candidateFilename) : release.assets;
  if (run.seal) {
    const cutoffFor = name => run.product_cutoffs[name.startsWith('dzc-') ? 'Publish verified standalone archives' : desktopUploadJob];
    if (assets.some(asset => !(Date.parse(asset.updated_at) <= cutoffFor(asset.name)))) {
      throw new Error('Product assets changed after their original upload job');
    }
  }
  return verifiedReleaseAssets({ ...release, assets }, tag, run.asset_cutoff, dual, run.arch_packages);
}

function sealArtifacts(source, run, transport) {
  const attempt = run.run_attempt;
  const declarations = [
    { name: 'source-acceptance-' + attempt, job: 'Source acceptance / Bind candidate source', report: true },
    { name: 'verified-desktop-assets-' + attempt, job: desktopUploadJob, report: true },
    { name: 'verified-standalone-assets-' + attempt, job: 'Publish verified standalone archives', report: true },
    ...windowsVariants.map(variant => ({
      name: ['windows-candidate', variant, attempt].join('-'), job: windowsCandidateJob(variant), report: true,
    })),
    ...linuxTargets.map(target => ({
      name: linuxRuntimeArtifact(target, attempt), job: linuxRuntimeJob(target), report: true,
    })),
    ...desktopReleaseTargets.flatMap(target => [
      { name: desktopProducerArtifact(target, attempt), job: 'Dual desktop release (' + target + ')', report: false },
      { name: ['release-cli', attempt, target].join('-'),
        job: 'Build compatible standalone CLI / Standalone (' + target + ')', report: false },
    ]),
  ];
  const artifacts = candidateArtifacts(source, String(run.run_id), declarations.map(entry => entry.name), transport);
  return declarations.map(({ job, report }, index) => {
    const producer = run.jobs.find(entry => entry.name === job);
    return { ...originalArtifact(artifacts[index], producer), report };
  });
}

function originalArtifact(artifact, producer) {
  const { id, name, size_in_bytes, digest, created_at, workflow_run } = artifact;
  const created = Date.parse(created_at);
  const start = Date.parse(producer?.started_at), end = Date.parse(producer?.completed_at);
  if (producer?.status !== 'completed' || producer.conclusion !== 'success'
      || !Number.isSafeInteger(workflow_run.repository_id) || workflow_run.repository_id <= 0
      || !Number.isSafeInteger(size_in_bytes) || size_in_bytes <= 0
      || !(created >= start && created <= end)) {
    throw new Error('Artifact lies outside its successful producer job: ' + name);
  }
  return { name, artifact_id: id, producer_job_id: producer.id, digest, size_in_bytes, created_at };
}

export function releaseSealReceiptId(tag, runId, runAttempt, transport = gh) {
  const client = githubClient(transport, true);
  const source = client.releaseCommit(tag);
  const run = verifiedReleaseRun(transport, tag, source, runId, runAttempt, true, true);
  if (run.status !== 'completed') throw new Error('The original Release attempt is still running');
  if (!run.seal) return null;
  const name = 'release-seal-attempt-' + run.run_attempt;
  const [artifact] = candidateArtifacts(source, String(run.run_id), [name], transport);
  const producer = run.jobs.find(job => job.name === candidateSealJob);
  const evidence = originalArtifact(artifact, producer);
  if (evidence.size_in_bytes > 8192) throw new Error('Original release seal receipt exceeds its artifact budget');
  return evidence.artifact_id;
}

function sameManifest(value, expected) {
  const matches = (actualSet, expectedSet) => actualSet
    && JSON.stringify(Object.keys(actualSet).sort()) === JSON.stringify(Object.keys(expectedSet).sort())
    && Object.entries(expectedSet).every(([platform, item]) => {
      const actual = actualSet[platform];
      return actual && Object.keys(actual).sort().join(',') === 'signature,url'
        && actual.url === item.url && actual.signature === item.signature;
    });
  return value && JSON.stringify(Object.keys(value).sort()) === JSON.stringify(Object.keys(expected).sort())
    && value.version === expected.version && value.notes === expected.notes
    && matches(value.platforms, expected.platforms)
    && (!expected.mygo_platforms || matches(value.mygo_platforms, expected.mygo_platforms));
}

function channelWriteIdentity(release) {
  if (release === null) return null;
  return { id: release.id, tag_name: release.tag_name, draft: release.draft, prerelease: release.prerelease,
    ...(release.draft ? { body: release.body } : {}),
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

function verifyUploadReadbacks(release, tag, source, run, readbacks, transport) {
  const { value } = parseVersion(tag);
  const expected = {
    desktop: desktopReleaseTargets.flatMap(target => expectedDesktopCandidateFiles(tag, target, run.arch_packages)),
    standalone: desktopReleaseTargets.flatMap(target => {
      const name = standaloneArchiveName(value, target);
      return [name, `${name}.sha256`];
    }),
  };
  if (String(release.id) !== readbacks.releaseId) {
    throw new Error('Upload readbacks identify a different draft release');
  }
  const archives = new Map(desktopReleaseTargets.flatMap(target =>
    expectedDesktopCandidateFiles(tag, target, run.arch_packages).filter(name => !name.endsWith('.sig')).map(name => [name, target])));
  const remote = new Map(release.assets.map(asset => [asset.name, asset]));
  for (const [kind, names] of Object.entries(expected)) {
    const report = readbacks[kind];
    if (report?.schema_version !== 1 || report.kind !== kind || report.tag !== tag || report.release_id !== readbacks.releaseId ||
        report.source_revision !== source || report.run_id !== run.run_id || report.run_attempt !== run.run_attempt ||
        !Array.isArray(report.assets)) {
      throw new Error(`Invalid ${kind} upload readback provenance`);
    }
    requireExactNames(report.assets.map(asset => asset?.name), names, `${kind} upload readback`);
    for (const entry of report.assets) {
      const asset = remote.get(entry.name);
      if (!asset || asset.id !== entry.id || asset.size !== entry.size || asset.digest !== `sha256:${entry.sha256}`) {
        throw new Error(`Upload readback no longer matches ${entry.name}`);
      }
    }
    if (kind === 'desktop') {
      requireArchiveReadback(report.archives, archives, new Map(report.assets.map(asset => [asset.name, asset])));
      if (run.windows_candidate_gate) verifyWindowsCandidateReports(readbacks.windows, report, tag, source, run);
      if (run.linux_runtime_gate) verifyLinuxRuntimeReports(readbacks.linux, report, tag, source, run, transport);
    }
  }
}

function verifySourceReadback(receipt, source, run) {
  const jobs = ['frontend', 'rust-development', 'go-development', 'ci-desktop-plan', 'tauri'];
  if (receipt?.schema_version !== 1 || receipt.mode !== 'source-acceptance'
      || receipt.source_revision !== source || receipt.run_id !== run.run_id
      || receipt.run_attempt !== run.run_attempt
      || JSON.stringify(receipt.desktop_targets) !== '["linux-x86_64","windows-x86_64"]'
      || !receipt.jobs || Object.keys(receipt.jobs).length !== jobs.length
      || jobs.some(name => receipt.jobs[name] !== 'success')) {
    throw new Error('Source acceptance receipt does not attest the candidate');
  }
}

export function releaseSealInputIds(tag, source, runId, runAttempt, transport = gh) {
  const run = verifiedReleaseRun(transport, tag, source, runId, runAttempt, false, true, true);
  return sealArtifacts(source, run, transport).filter(input => input.report).map(input => input.artifact_id);
}

export function sealReleaseCandidate(tag, runId, runAttempt, readbacks, readbackIds, transport = gh) {
  const version = parseVersion(tag);
  const client = githubClient(transport, true);
  const release = client.releaseByTag(tag);
  const source = client.releaseCommit(tag);
  const run = verifiedReleaseRun(transport, tag, source, runId, runAttempt, false, true, true);
  const products = verifiedProducts(release, tag, run, true);
  const inputs = sealArtifacts(source, run, transport);
  if (readbackIds !== inputs.filter(input => input.report).map(input => input.artifact_id).join(',')) {
    throw new Error('Downloaded reports do not identify the expected producer artifacts');
  }
  verifySourceReadback(readbacks.source, source, run);
  verifyUploadReadbacks(release, tag, source, run, readbacks, transport);
  const windowsProducer = inputs.find(input => input.name === desktopProducerArtifact(windowsX64, run.run_attempt));
  if (readbacks.windows.some(report => report.producer_artifact_id !== String(windowsProducer.artifact_id))) {
    throw new Error('Windows reports do not bind the original desktop producer artifact');
  }
  const manifest = assembleManifest(version.value, release.body ?? '', true, name => {
    const asset = release.assets.find(item => item.name === name + '.sig');
    if (!asset || asset.size > 16 * 1024) throw new Error('Missing or oversized verified signature');
    const bytes = client.downloadAsset(asset);
    if (bytes.length !== asset.size || asset.digest !== 'sha256:' + digest(bytes)) {
      throw new Error('Verified signature differs from the final package readback');
    }
    return bytes.toString('utf8').trim();
  });
  const bytes = candidateBytes({ tag, source, releaseId: release.id, repository, run, products, inputs,
    channel: promotionChannel(version.value), manifest });
  const readDraft = () => {
    const observed = client.releaseByTag(tag);
    if (observed.id !== release.id || observed.draft !== true || observed.prerelease !== Boolean(version.pre.length)
        || observed.body !== release.body || client.releaseCommit(tag) !== source
        || verifiedProducts(observed, tag, run, true) !== products) {
      throw new Error('The original draft changed during candidate sealing');
    }
    return observed;
  };
  const identity = attachCandidate({ tag, repository, bytes, interval: run.seal, readDraft,
    download: client.downloadAsset, transport });
  return { ...identity, release_id: release.id, source_revision: source, run_id: run.run_id, run_attempt: run.run_attempt };
}

function publication(mode, version, config, log, transport, bundle) {
  const dual = bundle?.dual === true;
  const client = githubClient(transport, dual);
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
  const run = synchronize ? verifiedReleaseRun(transport, tag, commit, bundle.runId, bundle.runAttempt, repair, dual) : null;
  const releaseAssets = synchronize ? verifiedProducts(release, parsed.value, run, dual) : null;
  if (bundle?.readbacks) verifyUploadReadbacks(release, tag, commit, run, bundle.readbacks, transport);
  if (synchronize && typeof release.draft !== 'boolean') throw new Error('Release draft state is missing');
  if (mode === 'sync-published' && (release.draft !== false || run.status !== 'completed')) {
    throw new Error('Published-channel synchronization requires a public release and completed eligible original run');
  }
  if (resumeDraft && (release.draft !== true || run.status !== 'completed' || !['failure', 'cancelled'].includes(run.conclusion))) {
    throw new Error('Draft recovery requires an unpublished release and completed publisher-only interruption');
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
  }, config.plugins.updater.pubkey, release.body ?? '', dual);
  const checkSeal = observed => {
    if (!run?.seal) return null;
    if (!Number.isFinite(Date.parse(run.seal.completed_at))) throw new Error('Candidate seal completion time is missing');
    const inputs = sealArtifacts(commit, run, transport);
    const bytes = candidateBytes({ tag, source: commit, releaseId: observed.id, repository, run,
      products: releaseAssets, inputs, channel, manifest });
    return requireCandidateAsset(observed, bytes, run.seal, client.downloadAsset);
  };
  const sealed = checkSeal(release);
  if (sealed && repair && !bundle.sealIdentity) throw new Error('Recovery requires the original frozen candidate receipt');
  if (bundle?.sealIdentity && (sealed?.id !== bundle.sealIdentity.id || sealed?.sha256 !== bundle.sealIdentity.sha256
      || release.id !== bundle.sealIdentity.releaseId)) {
    throw new Error('Frozen candidate differs from the successful seal job outputs');
  }
  bundle?.trace?.('candidate-verified', { release_id: release.id, candidate: sealed });
  const receipt = { schema_version: 1, version: parsed.value, source_revision: commit,
    release_id: release.id, signed_assets: signedAssets };
  if (synchronize && !dual) verifiedUpgradeGate(bundle.upgradeEvidence, receipt);
  const syncReceipt = synchronize ? { ...receipt, workflow: run,
    ...(sealed ? { sealed_candidate: sealed } : {}),
    ...(dual ? {} : { windows_upgrade_gate_sha256: bundle.upgradeEvidence.sha256 }) } : null;
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
  const channelUpdate = dual ? dualChannelUpdate({ channel, manifest, sameManifest,
    identity: { releaseId: release.id, source: commit, runId: run.run_id, attempt: run.run_attempt, assets: releaseAssets } }) : null;
  let old, currentBytes, current;
  if (channelUpdate) {
    channelUpdate.inspect(client, channels);
  } else {
    if (channels?.draft) throw new Error('Existing channel release is still a draft; inspect it before retrying');
    if (synchronize && channels && channels.assets.filter(asset => asset.name === `${channel}.json`).length > 1) {
      throw new Error('Updater channel contains duplicate active manifests');
    }
    old = channels?.assets.find(asset => asset.name === `${channel}.json`);
    currentBytes = old ? client.downloadAsset(old) : null;
    current = currentBytes ? JSON.parse(currentBytes.toString('utf8')) : null;
  }
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
    ? verifiedReleaseRun(transport, tag, commit, bundle.runId, bundle.runAttempt, repair, dual) : null;
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
        || verifiedProducts(latest, parsed.value, run, dual) !== releaseAssets
        || JSON.stringify(currentRun) !== JSON.stringify(run)
        || JSON.stringify(checkSeal(latest)) !== JSON.stringify(sealed)) {
      throw new Error('Release workflow gates or asset identities changed before publication');
    }
    if (!dual && readEvidenceJson(bundle.upgradePath).sha256 !== bundle.upgradeEvidence.sha256) {
      throw new Error('Windows upgrade gate changed before publication');
    }
    const freshChannels = client.releaseByTag(channelTag, true);
    if (JSON.stringify(channelWriteIdentity(freshChannels)) !== JSON.stringify(channelWriteIdentity(channels))) {
      throw new Error('Updater channel changed before synchronization');
    }
  }
  if (release.draft) {
    bundle?.trace?.('publish-product-requested', { release_id: release.id });
    transport(['release', 'edit', tag, '--repo', repository, '--draft=false',
      parsed.pre.length ? '--latest=false' : '--latest']);
  }
  const published = client.releaseByTag(tag);
  if (published.draft) throw new Error('Release has not been published');
  if (synchronize && (published.draft !== false || published.id !== release.id || published.tag_name !== tag
      || published.prerelease !== release.prerelease || published.body !== release.body
      || client.releaseCommit(tag) !== commit || verifiedProducts(published, parsed.value, run, dual) !== releaseAssets
      || JSON.stringify(checkSeal(published)) !== JSON.stringify(sealed))) {
    throw new Error('Published release readback differs from the verified release');
  }
  if (mode === 'publish-only') {
    log(`Published ${tag}; updater channels await separate public acceptance.`);
    return { ...receipt, status: 'awaiting_acceptance' };
  }
  if (channelUpdate) {
    bundle.trace?.('product-public-readback', { release_id: published.id });
    const observed = client.releaseByTag(channelTag, true);
    if (JSON.stringify(channelWriteIdentity(observed)) !== JSON.stringify(channelWriteIdentity(channels))) {
      throw new Error('Updater channel changed before activation');
    }
    const outcome = channelUpdate.synchronize({ client, tag: channelTag, repository, release: observed,
      transport, log, trace: bundle.trace });
    return { ...syncReceipt, ...outcome, channel };
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
    log(`Published ${tag}; advanced ${channel}.json after verifying all ${dual ? 'eight' : 'four'} signed updater artifacts.`);
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

const dualActions = {
  'publish-dual': 'publish-and-sync',
  'sync-dual-channel': 'sync-published',
  'resume-dual-draft': 'resume-verified-draft',
};

export function publishDual(mode, version, config, runId, runAttempt, log = console.log, transport = gh, readbacks, sealIdentity, trace) {
  if (!Object.hasOwn(dualActions, mode)) throw new Error('Unknown dual desktop publication mode');
  return publication(dualActions[mode], version, config, log, transport, { dual: true, runId, runAttempt, readbacks, sealIdentity, trace });
}

function uploadReceipt(kind, tag, source, releaseId, assets) {
  return { schema_version: 1, kind, tag, release_id: releaseId, source_revision: source,
    run_id: Number(process.env.GITHUB_RUN_ID), run_attempt: Number(process.env.GITHUB_RUN_ATTEMPT), assets };
}

function publicationTrace(tag, originalRun, originalAttempt) {
  return (stage, detail = {}) => {
    if (!process.env.DZC_PUBLICATION_TRACE) return;
    appendFileSync(process.env.DZC_PUBLICATION_TRACE, JSON.stringify({
      stage, tag, original_run_id: Number(originalRun), original_attempt: Number(originalAttempt),
      run_id: Number(process.env.GITHUB_RUN_ID), run_attempt: Number(process.env.GITHUB_RUN_ATTEMPT),
      time: new Date().toISOString(), ...detail,
    }) + '\n');
  };
}

function publishDualCommand(mode, tag, config, runId, runAttempt, identity) {
  const trace = publicationTrace(tag, runId, runAttempt);
  trace('started', { candidate: identity });
  try {
    const result = publishDual(mode, tag, config, runId, runAttempt, console.error, gh, undefined, identity, trace);
    const verifiedAt = new Date().toISOString();
    trace('completed', { status: result.status });
    console.log(JSON.stringify({ ...result, channel_verified_at: verifiedAt }));
  } catch (error) {
    trace('failed');
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  preflight(config, pkg);
  const [mode, version, acceptancePath, runAttempt, upgradePath, ...remaining] = process.argv.slice(2);
  if (mode === 'preflight') {
    parseVersion(version);
    if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Release signing secret is not configured');
    console.log('Updater signing configuration is ready.');
  } else if (mode === 'stage-desktop') {
    stageDesktopCandidate({ tag: version, source: acceptancePath, target: runAttempt, destination: upgradePath,
      runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT, files: remaining });
  } else if (mode === 'attach-desktop') {
    let archives;
    const assets = attachDesktopCandidates({ tag: version, source: acceptancePath, directory: runAttempt, releaseId: upgradePath,
      runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT }, gh,
    files => { archives = examineDesktopArchives(version, files, config.plugins.updater.pubkey); });
    console.log(JSON.stringify({ ...uploadReceipt('desktop', version, acceptancePath, upgradePath, assets), archives }));
  } else if (mode === 'attach-standalone') {
    const assets = attachReleaseFiles({ tag: version, source: acceptancePath, releaseId: upgradePath },
      readStandaloneCandidates(version, runAttempt), gh);
    console.log(JSON.stringify(uploadReceipt('standalone', version, acceptancePath, upgradePath, assets)));
  } else if (mode === 'windows-producer-id' || mode === 'windows-report-ids') {
    const producer = mode === 'windows-producer-id';
    const names = producer ? (target, attempt) => `release-desktop-${target}-${attempt}`
      : (variant, attempt) => `windows-candidate-${variant}-${attempt}`;
    console.log(candidateArtifactIds({ source: acceptancePath, runId: process.env.GITHUB_RUN_ID,
      attempt: process.env.GITHUB_RUN_ATTEMPT }, names, gh, producer ? [windowsX64] : windowsVariants).join(','));
  } else if (mode === 'stage-windows-candidate' && remaining.length === 2) {
    stageWindowsCandidate({ tag: version, source: acceptancePath, variant: runAttempt, directory: upgradePath,
      destination: remaining[0], artifactId: remaining[1], publicKey: config.plugins.updater.pubkey,
      runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT });
  } else if (mode === 'replay-windows-candidate' && process.env.GITHUB_EVENT_NAME === 'workflow_dispatch') {
    stageWindowsCandidate({ replay: true, variant: version, destination: acceptancePath,
      publicKey: config.plugins.updater.pubkey, runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT });
  } else if (mode === 'replay-linux-runtime' && process.env.GITHUB_EVENT_NAME === 'workflow_dispatch') {
    stageLinuxRuntime({ target: version, destination: acceptancePath, publicKey: config.plugins.updater.pubkey,
      runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT });
  } else if (mode === 'linux-runtime-report-ids' || (mode === 'linux-runtime-producer-id' && linuxTargets.includes(runAttempt))) {
    const reporting = mode === 'linux-runtime-report-ids';
    console.log(candidateArtifactIds({ source: acceptancePath, runId: process.env.GITHUB_RUN_ID,
      attempt: process.env.GITHUB_RUN_ATTEMPT }, reporting ? linuxRuntimeArtifact : desktopProducerArtifact,
      gh, reporting ? linuxTargets : [runAttempt]).join(','));
  } else if (mode === 'stage-linux-runtime' && remaining.length === 2) {
    stageLinuxRuntime({ tag: version, source: acceptancePath, target: runAttempt, directory: upgradePath,
      destination: remaining[0], artifactId: remaining[1], publicKey: config.plugins.updater.pubkey,
      runId: process.env.GITHUB_RUN_ID, attempt: process.env.GITHUB_RUN_ATTEMPT });
  } else if (mode === 'desktop-artifact-ids' || mode === 'standalone-artifact-ids') {
    const resolveIds = mode === 'desktop-artifact-ids' ? desktopArtifactIds : standaloneArtifactIds;
    console.log(resolveIds({ source: acceptancePath, runId: process.env.GITHUB_RUN_ID,
      attempt: process.env.GITHUB_RUN_ATTEMPT }).join(','));
  } else if (mode === 'seal-input-ids' && process.env.GITHUB_EVENT_NAME === 'push') {
    console.log(releaseSealInputIds(version, acceptancePath, process.env.GITHUB_RUN_ID, process.env.GITHUB_RUN_ATTEMPT).join(','));
  } else if (mode === 'seal-dual' && process.env.GITHUB_EVENT_NAME === 'push' && remaining.length === 2) {
    const read = name => readEvidenceJson(join(upgradePath, name + '.json')).value;
    const readbacks = { releaseId: remaining[0], source: read('source-acceptance'),
      desktop: read('desktop-readback'), standalone: read('standalone-readback'),
      windows: windowsVariants.map(variant => read('windows-candidate-' + variant)),
      linux: linuxTargets.map(target => read('linux-runtime-' + target)) };
    console.log(JSON.stringify(sealReleaseCandidate(version, acceptancePath, runAttempt, readbacks, remaining[1])));
  } else if (mode === 'publish-sealed-dual' && process.env.GITHUB_EVENT_NAME === 'push' && remaining.length === 2) {
    const sealIdentity = { id: Number(remaining[0]), sha256: remaining[1], releaseId: Number(upgradePath) };
    publishDualCommand('publish-dual', version, config, acceptancePath, runAttempt, sealIdentity);
  } else if (mode === 'seal-receipt-id' && process.env.GITHUB_EVENT_NAME === 'workflow_dispatch') {
    console.log(releaseSealReceiptId(version, acceptancePath, runAttempt) ?? '');
  } else if (mode === 'publish-only') publishOnly(version, config);
  else if (mode !== 'publish-dual' && Object.hasOwn(dualActions, mode) && acceptancePath && runAttempt && remaining.length === 0) {
    let identity;
    if (upgradePath) {
      const receipt = readEvidenceJson(upgradePath).value;
      if (receipt?.name !== candidateFilename || receipt.run_id !== Number(acceptancePath)
          || receipt.run_attempt !== Number(runAttempt) || !/^[a-f0-9]{64}$/.test(receipt.sha256 ?? '')
          || !Number.isSafeInteger(receipt.id) || receipt.id <= 0 || !Number.isSafeInteger(receipt.release_id)
          || receipt.release_id <= 0 || githubClient(gh).releaseCommit(version) !== receipt.source_revision) {
        throw new Error('Recovery receipt does not identify the original candidate');
      }
      identity = { id: receipt.id, sha256: receipt.sha256, releaseId: receipt.release_id };
    }
    publishDualCommand(mode, version, config, acceptancePath, runAttempt, identity);
  }
  else if (['publish-and-sync', 'sync-published', 'resume-verified-draft'].includes(mode) && acceptancePath && runAttempt && upgradePath) {
    const synchronize = mode === 'resume-verified-draft' ? resumeVerifiedDraft
      : mode === 'sync-published' ? syncPublished : publishAndSync;
    console.log(JSON.stringify(synchronize(version, config, acceptancePath, runAttempt, upgradePath, console.error)));
  }
  else if (mode === 'promote' && acceptancePath) {
    promote(version, config, acceptancePath);
  } else throw new Error('Unsupported updater release command or missing required candidate arguments');
}
