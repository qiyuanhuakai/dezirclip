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
function api(path) { return JSON.parse(gh(['api', `repos/${repository}/${path}`])); }
function optionalChannelRelease() {
  try { return api(`releases/tags/${channelTag}`); }
  catch (error) {
    if (error.stderr?.toString().includes('(HTTP 404)')) return null;
    throw error;
  }
}
function downloadAsset(asset) {
  return gh(['api', `repos/${repository}/releases/assets/${asset.id}`, '-H', 'Accept: application/octet-stream'], true);
}
function moveAsset(asset, name) {
  return apiMutation(`releases/assets/${asset.id}`, ['-X', 'PATCH', '-f', `name=${name}`]);
}
function apiMutation(path, args) {
  return JSON.parse(gh(['api', `repos/${repository}/${path}`, ...args]));
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

export function publish(version, config, log = console.log) {
  const parsed = parseVersion(version);
  const tag = `v${parsed.value}`;
  const channel = promotionChannel(parsed.value);
  const release = api(`releases/tags/${tag}`);
  if (release.tag_name !== tag || release.prerelease !== Boolean(parsed.pre.length)) {
    throw new Error('Release tag or prerelease flag differs from the signed version');
  }
  const commit = releaseCommit(tag);
  const manifest = makeManifest(parsed.value, release.assets, name => {
    return downloadAsset(release.assets.find(asset => asset.name === name));
  }, config.plugins.updater.pubkey, release.body ?? '');
  let channels = optionalChannelRelease();
  if (channels?.draft) throw new Error('Existing channel release is still a draft; inspect it before retrying');
  const old = channels?.assets.find(asset => asset.name === `${channel}.json`);
  const current = old ? JSON.parse(downloadAsset(old).toString('utf8')) : null;
  if (current) {
    parseVersion(current.version);
    if (channel === 'stable' && parseVersion(current.version).pre.length) {
      throw new Error('Stable channel contains a prerelease');
    }
  }
  // Check asset identity again before making the verified release public.
  const latest = api(`releases/tags/${tag}`);
  for (const asset of release.assets) {
    if (!latest.assets.some(value => value.id === asset.id && value.size === asset.size
        && value.updated_at === asset.updated_at)) {
      throw new Error('Release assets changed during verification');
    }
  }
  if (release.draft) {
    gh(['release', 'edit', tag, '--repo', repository, '--draft=false',
      parsed.pre.length ? '--latest=false' : '--latest']);
  }
  if (api(`releases/tags/${tag}`).draft) throw new Error('Release has not been published');
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
      gh(['release', 'create', channelTag, '--repo', repository, '--draft', '--prerelease',
        '--latest=false', '--target', commit, '--title', 'DezirClip updater channels',
        '--notes', 'Signed updater manifests. Stable accepts official releases; Preview checks both channels.']);
      channels = api(`releases/tags/${channelTag}`);
    }
    gh(['release', 'upload', channelTag, path, '--repo', repository, '--clobber']);
    channels = api(`releases/tags/${channelTag}`);
    const staged = channels.assets.find(asset => asset.name === stagedName);
    if (!staged || !downloadAsset(staged).equals(contents)) throw new Error('Uploaded channel verification failed');
    if (old) moveAsset(old, `${channel}-previous-${old.id}.json`);
    try { moveAsset(staged, `${channel}.json`); }
    catch (error) {
      const active = api(`releases/tags/${channelTag}`).assets.find(asset => asset.name === `${channel}.json`);
      // A lost API response can follow a successful rename; resolve that outcome first.
      if (!active || !downloadAsset(active).equals(contents)) {
        if (old && !active) moveAsset(old, `${channel}.json`);
        throw error;
      }
    }
    if (fresh) gh(['release', 'edit', channelTag, '--repo', repository, '--draft=false', '--latest=false']);
    log(`Published ${tag}; advanced ${channel}.json after verifying all four signed updater artifacts.`);
  } finally {
    if (resolve(staging).startsWith(resolve(tmpdir()) + (process.platform === 'win32' ? '\\' : '/'))
        && basename(staging).startsWith('dezirclip-channel-')) rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  preflight(config, pkg);
  const [mode, version] = process.argv.slice(2);
  if (mode === 'preflight') {
    parseVersion(version);
    if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('Release signing secret is not configured');
    console.log('Updater signing configuration is ready.');
  } else if (mode === 'publish') publish(version, config);
  else throw new Error('Usage: node scripts/updater-release.mjs preflight|publish vVERSION');
}
