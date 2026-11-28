import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { compareVersions, parseVersion } from './release-semver.mjs';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const noPrevious = '0'.repeat(64);

export function dualChannelUpdate({ channel, manifest, identity, sameManifest }) {
  const contents = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  const contentHash = sha256(contents);
  const operation = sha256(JSON.stringify([channel, identity, contentHash]));
  const activeName = `${channel}.json`;
  const pending = `${channel}-pending-`;
  const previous = `${channel}-previous-`;
  const prefix = `${pending}${operation}-`;
  const notes = `Signed updater manifests. Stable accepts official releases; Preview checks both channels.\n\nDezirClip channel bootstrap v1: ${channel} ${operation}`;

  function asset(release, name) {
    const matches = release.assets.filter(value => value.name === name);
    if (matches.length > 1) throw new Error(`Duplicate channel asset: ${name}`);
    return matches[0] ?? null;
  }

  function read(client, value, expectedHash) {
    if (!Number.isSafeInteger(value?.id) || value.id <= 0 || value.state !== 'uploaded'
        || !Number.isSafeInteger(value.size) || value.size <= 0 || value.size > 1024 * 1024) {
      throw new Error('Invalid updater channel asset identity');
    }
    const bytes = client.downloadAsset(value);
    const hash = sha256(bytes);
    if (bytes.length !== value.size || (value.digest && value.digest !== `sha256:${hash}`)
        || (expectedHash !== undefined && hash !== expectedHash)) {
      throw new Error('Updater channel asset differs from its verified content');
    }
    return { value, bytes, hash };
  }

  function readIndex(client, value, expectedHash) {
    const observed = read(client, value, expectedHash);
    const index = JSON.parse(observed.bytes.toString('utf8'));
    const version = parseVersion(index.version);
    if (channel === 'stable' && version.pre.length) throw new Error('Stable channel contains a prerelease');
    return { ...observed, index };
  }

  function inspect(client, release) {
    if (!release) return { old: null, active: null, staged: null, stagedName: null };
    if (typeof release.draft !== 'boolean' || release.prerelease !== true || !Array.isArray(release.assets)) {
      throw new Error('Invalid updater channel release identity');
    }
    if (release.draft && release.body?.trimEnd() !== notes) {
      throw new Error('Existing channel release is still a draft and its ownership is unknown');
    }
    const found = asset(release, activeName);
    const active = found ? readIndex(client, found) : null;
    if (active) {
      const order = compareVersions(active.index.version, manifest.version);
      if (order === 0 && !sameManifest(active.index, manifest)) {
        throw new Error('Equal-version channel manifest does not exactly match the verified release');
      }
      if (order >= 0) {
        if (release.draft && (order !== 0 || !active.bytes.equals(contents))) {
          throw new Error('Owned channel draft does not contain the verified active manifest');
        }
        return { active, order, old: null, staged: null, stagedName: null };
      }
    }
    const stagedAssets = release.assets.filter(value => value.name.startsWith(pending));
    if (stagedAssets.length > 1 || stagedAssets.some(value => !value.name.startsWith(prefix))) {
      throw new Error('Updater channel contains staging owned by another candidate');
    }
    if (!stagedAssets.length) {
      if (!active && release.assets.some(value => value.name.startsWith(previous) || value.name.startsWith(`${channel}-`))) {
        throw new Error('Missing active channel and no matching candidate transition');
      }
      return { active, old: active, staged: null, stagedName: null };
    }
    const staged = stagedAssets[0];
    const binding = /^([1-9]\d*|0)-([a-f0-9]{64})\.json$/.exec(staged.name.slice(prefix.length));
    if (!binding || !Number.isSafeInteger(Number(binding[1]))) throw new Error('Invalid staged channel binding');
    const [, priorId, priorHash] = binding;
    read(client, staged, contentHash);
    let old = active;
    if (!active && priorId !== '0') {
      old = readIndex(client, asset(release, `${previous}${priorId}-${operation}.json`), priorHash);
    }
    if (String(old?.value.id ?? 0) !== priorId || (old?.hash ?? noPrevious) !== priorHash
        || (old && compareVersions(old.index.version, manifest.version) >= 0)) {
      throw new Error('Previous updater channel does not match the staged candidate');
    }
    return { active, old, staged, stagedName: staged.name };
  }

  function synchronize({ client, tag, repository, release, transport, log }) {
    let releaseId = release?.id;
    const load = () => {
      const observed = client.releaseByTag(tag);
      if (releaseId !== undefined && observed.id !== releaseId) throw new Error('Updater channel release ID changed');
      releaseId = observed.id;
      return observed;
    };
    if (!release) {
      try {
        transport(['release', 'create', tag, '--repo', repository, '--draft', '--prerelease',
          '--latest=false', '--target', identity.source, '--title', 'DezirClip updater channels', '--notes', notes]);
      } catch (error) {
        // The write may have succeeded even though its response was lost.
        const observed = client.releaseByTag(tag, true);
        if (!observed?.draft || observed.body?.trimEnd() !== notes) throw error;
      }
      release = load();
      if (!release.draft || release.body?.trimEnd() !== notes) throw new Error('Created channel is not the expected draft');
    }

    let transition = inspect(client, release);
    if (!transition.staged && transition.order === undefined) {
      const expected = transition.active;
      const name = `${prefix}${expected?.value.id ?? 0}-${expected?.hash ?? noPrevious}.json`;
      const directory = mkdtempSync(join(resolve(tmpdir()), 'dezirclip-channel-'));
      try {
        const path = join(directory, name);
        writeFileSync(path, contents);
        try {
          transport(['release', 'upload', tag, path, '--repo', repository]);
        } catch (error) {
          if (!asset(load(), name)) throw error;
        }
        transition = inspect(client, load());
        if (transition.stagedName !== name || transition.active?.value.id !== expected?.value.id
            || transition.active?.hash !== expected?.hash) {
          throw new Error('Current updater channel changed during staging');
        }
      } finally {
        if (dirname(directory) === resolve(tmpdir())) rmSync(directory, { recursive: true, force: true });
      }
    }
    if (transition.order > 0) {
      return { status: 'newer_channel_preserved', channel_version: transition.active.index.version };
    }
    const staged = transition.staged;
    if (staged) {
      const old = transition.old?.value;
      const backupName = old && `${previous}${old.id}-${operation}.json`;
      try {
        if (transition.active) client.moveAsset(old, backupName);
        const ready = inspect(client, load());
        if (ready.active || ready.staged?.id !== staged.id) throw new Error('Updater transition changed before activation');
        client.moveAsset(staged, activeName);
      } catch (error) {
        const observed = inspect(client, load());
        if (observed.active?.value.id !== staged.id || !observed.active.bytes.equals(contents)) {
          if (old && !observed.active && observed.staged?.id === staged.id && observed.old?.value.id === old.id) {
            client.moveAsset(observed.old.value, activeName);
          }
          throw error;
        }
      }
    }
    const activated = load();
    const result = inspect(client, activated);
    const intended = staged?.id ?? transition.active?.value.id;
    if (result.order !== 0 || result.active.value.id !== intended || (staged && !result.active.bytes.equals(contents))) {
      throw new Error('Active updater channel readback differs');
    }
    if (activated.draft) {
      try {
        transport(['release', 'edit', tag, '--repo', repository, '--draft=false', '--latest=false']);
      } catch (error) {
        if (load().draft) throw error;
      }
      const published = load();
      const verified = inspect(client, published);
      if (published.draft || verified.active?.value.id !== intended || !verified.active.bytes.equals(contents)) {
        throw new Error('Published updater channel does not match its verified draft');
      }
    }
    log(`Verified ${activeName} for ${manifest.version}.`);
    return { status: staged || activated.draft ? 'synchronized' : 'already_synchronized', channel_version: manifest.version };
  }

  return { inspect, synchronize };
}
