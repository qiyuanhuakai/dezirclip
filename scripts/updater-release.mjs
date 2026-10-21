import { createHash, createPublicKey, verify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
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

function publication(mode, version, config, log, transport, acceptance) {
  const client = githubClient(transport);
  const parsed = parseVersion(version);
  const tag = `v${parsed.value}`;
  const channel = promotionChannel(parsed.value);
  const release = client.releaseByTag(tag);
  if (release.tag_name !== tag || release.prerelease !== Boolean(parsed.pre.length)) {
    throw new Error('Release tag or prerelease flag differs from the signed version');
  }
  const commit = client.releaseCommit(tag);
  if (mode === 'promote' && release.draft) throw new Error('Promotion requires an already published release');
  const signedAssets = [];
  const manifest = makeManifest(parsed.value, release.assets, name => {
    const asset = release.assets.find(asset => asset.name === name);
    const bytes = client.downloadAsset(asset);
    signedAssets.push({ name, id: asset.id, size: asset.size, updated_at: asset.updated_at,
      sha256: createHash('sha256').update(bytes).digest('hex') });
    return bytes;
  }, config.plugins.updater.pubkey, release.body ?? '');
  const receipt = { schema_version: 1, version: parsed.value, source_revision: commit,
    release_id: release.id, signed_assets: signedAssets };
  if (mode === 'promote') {
    const assetIdentity = assets => !Array.isArray(assets) || assets.length !== 8 ? null : JSON.stringify(
      assets.map(asset => ({ name: asset.name, id: asset.id, size: asset.size,
        updated_at: asset.updated_at, sha256: asset.sha256 })).sort((a, b) => a.name.localeCompare(b.name)));
    if (acceptance?.status !== 'passed' || acceptance.schema_version !== 1
        || acceptance.version !== receipt.version || acceptance.source_revision !== commit
        || acceptance.release_id !== release.id
        || assetIdentity(acceptance.signed_assets) !== assetIdentity(signedAssets)
        || ['public_assets', 'appimage_updates', 'windows_upgrade', 'performance'].some(
          gate => typeof acceptance.gates?.[gate] !== 'string' || !/^[a-f0-9]{64}$/.test(acceptance.gates[gate]))) {
      throw new Error('Promotion requires matching completed public acceptance evidence');
    }
  }
  let channels = mode === 'promote' ? client.releaseByTag(channelTag, true) : null;
  if (channels?.draft) throw new Error('Existing channel release is still a draft; inspect it before retrying');
  const old = channels?.assets.find(asset => asset.name === `${channel}.json`);
  const current = old ? JSON.parse(client.downloadAsset(old).toString('utf8')) : null;
  if (current) {
    parseVersion(current.version);
    if (channel === 'stable' && parseVersion(current.version).pre.length) {
      throw new Error('Stable channel contains a prerelease');
    }
  }
  // Check asset identity again before making the verified release public.
  const latest = client.releaseByTag(tag);
  if (latest.id !== release.id || client.releaseCommit(tag) !== commit) throw new Error('Release source changed during verification');
  for (const asset of release.assets) {
    if (!latest.assets.some(value => value.id === asset.id && value.name === asset.name && value.size === asset.size
        && value.updated_at === asset.updated_at)) {
      throw new Error('Release assets changed during verification');
    }
  }
  if (release.draft) {
    transport(['release', 'edit', tag, '--repo', repository, '--draft=false',
      parsed.pre.length ? '--latest=false' : '--latest']);
  }
  if (client.releaseByTag(tag).draft) throw new Error('Release has not been published');
  if (mode === 'publish-only') {
    log(`Published ${tag}; updater channels await separate public acceptance.`);
    return { ...receipt, status: 'awaiting_acceptance' };
  }
  if (!shouldPromote(current?.version, parsed.value)) {
    log(`Published ${tag}; ${channel} already points to an equal or newer version.`);
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
    try {
      if (old) client.moveAsset(old, `${channel}-previous-${old.id}.json`);
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
    log(`Published ${tag}; advanced ${channel}.json after verifying all four signed updater artifacts.`);
  } finally {
    if (resolve(staging).startsWith(resolve(tmpdir()) + (process.platform === 'win32' ? '\\' : '/'))
        && basename(staging).startsWith('dezirclip-channel-')) rmSync(staging, { recursive: true, force: true });
  }
}

export function publishOnly(version, config, log = console.log, transport = gh) {
  return publication('publish-only', version, config, log, transport);
}

export function promote(version, config, acceptance, log = console.log, transport = gh) {
  return publication('promote', version, config, log, transport, acceptance);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  preflight(config, pkg);
  const [mode, version, acceptancePath] = process.argv.slice(2);
  if (mode === 'preflight') {
    parseVersion(version);
    if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Release signing secret is not configured');
    console.log('Updater signing configuration is ready.');
  } else if (mode === 'publish-only') publishOnly(version, config);
  else if (mode === 'promote' && acceptancePath) {
    promote(version, config, JSON.parse(readFileSync(acceptancePath, 'utf8')));
  } else throw new Error('Usage: node scripts/updater-release.mjs preflight|publish-only vVERSION; promote vVERSION ACCEPTANCE.json');
}
