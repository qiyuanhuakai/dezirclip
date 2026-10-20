import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { basename } from 'node:path';
import test from 'node:test';
import { compareVersions, makeManifest, parseVersion, promotionChannel, publish, repository, shouldPromote, verifyTauriSignature } from '../updater-release.mjs';

// Public test vector from tauri-plugin-updater 2.14.0, updater.rs offline_tests.
// https://docs.rs/tauri-plugin-updater/2.14.0/src/tauri_plugin_updater/updater.rs.html
const encode = value => Buffer.from(value).toString('base64');
const publicKey = encode('untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe');
const signatureText = 'untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==';
const signature = encode(signatureText);

test('official Tauri fixture verifies artifact and authenticated version', () => {
  verifyTauriSignature(Buffer.from('test'), signature, publicKey, '2.0.0', 'payload');
  assert.throws(() => verifyTauriSignature(Buffer.from('changed'), signature, publicKey, '2.0.0'));
  assert.throws(() => verifyTauriSignature(Buffer.from('test'), signature, publicKey, '9.0.0'));
  assert.throws(() => verifyTauriSignature(Buffer.from('test'), signature, publicKey, '2.0.0', 'other.exe'));
  assert.throws(() => verifyTauriSignature(Buffer.from('test'), encode(signatureText.replace('version:2.0.0', 'version:9.0.0')), publicKey, '9.0.0'));
  assert.throws(() => verifyTauriSignature(Buffer.from('test'), encode(signatureText.replace('\tversion:2.0.0', '')), publicKey, '2.0.0'));
});

test('all four updater artifacts and signatures are required before promotion', () => {
  let reads = 0;
  assert.throws(() => makeManifest('0.2.0-alpha.1', [], () => { reads++; }, publicKey), /Missing/);
  assert.equal(reads, 0);
  const name = 'DezirClip_2.0.0_x64-setup.exe';
  const assets = [{ name, size: 4 }, { name: `${name}.sig`, size: signature.length }];
  assert.throws(() => makeManifest('2.0.0', assets,
    asset => Buffer.from(asset.endsWith('.sig') ? signature : 'test'), publicKey), /filename/);
});

test('channel progression follows SemVer and refuses equal versions and downgrades', () => {
  assert.equal(compareVersions('0.2.0-alpha.10', '0.2.0-alpha.2'), 1);
  assert.equal(compareVersions('0.2.0', '0.2.0-rc.1'), 1);
  assert.equal(compareVersions('0.2.0+new', '0.2.0+old'), 0);
  assert.equal(promotionChannel('0.2.0-alpha.1'), 'preview');
  assert.equal(promotionChannel('0.2.0'), 'stable');
  assert.equal(shouldPromote(undefined, '0.2.0-alpha.1'), true);
  assert.equal(shouldPromote('0.2.0', '0.2.0-alpha.9'), false);
  assert.equal(shouldPromote('0.2.0-alpha.10', '0.2.0-alpha.2'), false);
  // Preview compares both indexes, so a newer formal release wins over an older preview.
  assert.equal(compareVersions('0.2.0', '0.2.0-alpha.9'), 1);
  for (const invalid of ['1.2', '01.2.3', '1.2.3-alpha.01', '1.2.3\n']) {
    assert.throws(() => parseVersion(invalid));
  }
});

// All publisher calls use the injected in-memory transport. Test signing keys
// exist only in memory; no GitHub process, production key or remote write runs.
function publisherFixture(options = {}) {
  const version = '0.2.0-alpha.1';
  const tag = `v${version}`;
  const keyPair = generateKeyPairSync('ed25519');
  const keyId = Buffer.from('0102030405060708', 'hex');
  const rawKey = keyPair.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const signingKey = encode(`untrusted comment: test public key\n${Buffer.concat([Buffer.from('Ed'), keyId, rawKey]).toString('base64')}`);
  const bytes = new Map();
  let nextId = 100;
  const state = {
    calls: [], mutations: [], uploadPaths: [], draftIdsRead: [], listCalls: 0,
    product: { id: 10, tag_name: tag, draft: options.published !== true,
      prerelease: true, body: 'Synthetic release notes', assets: [] },
    channel: null,
  };
  const asset = (name, data) => {
    const value = { id: nextId++, name, size: data.length, updated_at: '2026-10-20T00:00:00Z' };
    bytes.set(value.id, Buffer.from(data));
    return value;
  };
  for (const suffix of ['x64-setup.exe', 'arm64-setup.exe', 'amd64.AppImage', 'aarch64.AppImage']) {
    const name = `DezirClip_${version}_${suffix}`;
    const payload = Buffer.from('Synthetic ' + name);
    const signed = sign(null, createHash('blake2b512').update(payload).digest(), keyPair.privateKey);
    const comment = `timestamp:1\tfile:${name}\tversion:${version}`;
    const global = sign(null, Buffer.concat([signed, Buffer.from(comment)]), keyPair.privateKey);
    const packet = Buffer.concat([Buffer.from('ED'), keyId, signed]);
    const encoded = encode(`untrusted comment: test signature\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}`);
    state.product.assets.push(asset(name, payload), asset(`${name}.sig`, Buffer.from(encoded)));
  }
  for (const platform of ['linux-x86_64', 'linux-aarch64', 'windows-x86_64', 'windows-aarch64']) {
    const name = `dzc-${version}-${platform}.${platform.startsWith('windows') ? 'zip' : 'tar.gz'}`;
    state.product.assets.push(asset(name, Buffer.from('archive')), asset(`${name}.sha256`, Buffer.from('checksum')));
  }
  for (const arch of ['x86_64', 'aarch64']) {
    state.product.assets.push(asset(`DezirClip-${version}-linux-${arch}.flatpak`, Buffer.from('flatpak')));
  }
  if (options.tampered) bytes.get(state.product.assets[0].id)[0] ^= 1;
  if (options.channel) {
    state.channel = { id: 20, tag_name: 'updater-channels', draft: options.channel === 'draft',
      prerelease: true, assets: [
        asset('stable.json', Buffer.from('{"version":"0.1.20"}')),
        asset('preview.json', Buffer.from('{"version":"0.2.0-alpha.0"}')),
      ] };
  }
  const json = value => JSON.stringify(value);
  const transport = (args, binary = false) => {
    state.calls.push([...args]);
    if (args[0] === 'api') {
      const path = args[1].replace(`repos/${repository}/`, '');
      assert.equal(args[1], `repos/${repository}/${path}`);
      if (path === 'releases?per_page=100') {
        assert.deepEqual(args.slice(2), ['--paginate', '--slurp']);
        if (++state.listCalls === options.listErrorAt) throw options.apiError;
        const matches = [state.product, ...(state.channel ? [state.channel] : [])];
        if (options.duplicate) matches.push({ ...state.product, id: 11 });
        // Put the target on a later page: checking only the first page fails.
        return json([[{ id: 1, tag_name: 'v0.0.0', draft: false }], matches]);
      }
      if (path.startsWith('releases/tags/')) {
        assert.fail('Publisher must not use the published-only by-tag endpoint for drafts');
      }
      if (path === `git/ref/tags/${tag}`) return json({ object: { type: 'commit', sha: 'a'.repeat(40) } });
      if (path === 'releases/10' || path === 'releases/20') {
        const release = path === 'releases/10' ? state.product : state.channel;
        assert.ok(release);
        if (path === 'releases/20' && options.channelError) throw options.channelError;
        if (release.draft) state.draftIdsRead.push(release.id);
        return json(release);
      }
      const match = /^releases\/assets\/(\d+)$/.exec(path);
      if (match) {
        const id = Number(match[1]);
        if (args.includes('PATCH')) {
          assert.deepEqual(args.slice(2, 5), ['-X', 'PATCH', '-f']);
          const name = args[5].slice('name='.length);
          assert.equal(args[5], `name=${name}`);
          const found = state.channel.assets.find(value => value.id === id);
          assert.ok(found);
          const activating = name === 'preview.json' && found.name.startsWith('preview-0.2.0');
          if (activating && options.failRename && !state.renameFailure) {
            state.renameFailure = true;
            throw new Error('Synthetic rename rejected');
          }
          found.name = name;
          state.mutations.push(`rename:${name}`);
          if (activating && options.lostRenameResponse && !state.lostResponse) {
            state.lostResponse = true;
            throw new Error('Synthetic lost rename response');
          }
          return json(found);
        }
        assert.equal(binary, true);
        assert.deepEqual(args.slice(2), ['-H', 'Accept: application/octet-stream']);
        assert.ok(bytes.has(id));
        return Buffer.from(bytes.get(id));
      }
      assert.fail('Unexpected fake API path: ' + path);
    }
    assert.equal(args[0], 'release');
    assert.equal(args[args.indexOf('--repo') + 1], repository);
    const action = args[1], target = args[2];
    if (action === 'create' && target === 'updater-channels') {
      assert.equal(state.product.draft, false);
      assert.equal(state.channel, null);
      assert.ok(args.includes('--draft') && args.includes('--prerelease') && args.includes('--latest=false'));
      assert.equal(args[args.indexOf('--target') + 1], 'a'.repeat(40));
      state.channel = { id: 20, tag_name: target, draft: true, prerelease: true, assets: [] };
      state.mutations.push('create:channel-draft');
      return '';
    }
    if (action === 'upload' && target === 'updater-channels') {
      const path = args[3];
      state.uploadPaths.push(path);
      state.channel.assets.push(asset(basename(path), readFileSync(path)));
      state.mutations.push('upload:channel');
      return '';
    }
    if (action === 'edit' && (target === tag || target === 'updater-channels')) {
      assert.ok(args.includes('--draft=false') && args.includes('--latest=false'));
      const release = target === tag ? state.product : state.channel;
      release.draft = false;
      state.mutations.push(target === tag ? 'publish:product' : 'publish:channel');
      return '';
    }
    assert.fail('Unexpected fake release command: ' + args.join(' '));
  };
  return { state, transport, config: { plugins: { updater: { pubkey: signingKey } } },
    active: name => {
      const found = state.channel?.assets.find(value => value.name === name);
      return found ? JSON.parse(bytes.get(found.id)) : null;
    } };
}

test('publish verifies a product draft and a fresh channel draft through paginated lookup and release IDs', () => {
  const fixture = publisherFixture();
  publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
  assert.equal(fixture.state.product.assets.length, 18);
  assert.equal(fixture.state.product.draft, false);
  assert.equal(fixture.state.channel.draft, false);
  assert.deepEqual(fixture.state.mutations, [
    'publish:product', 'create:channel-draft', 'upload:channel', 'rename:preview.json', 'publish:channel',
  ]);
  assert.ok(fixture.state.draftIdsRead.includes(10) && fixture.state.draftIdsRead.includes(20));
  assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.1');
  assert.equal(Object.keys(fixture.active('preview.json').platforms).length, 4);
  assert.equal(fixture.active('stable.json'), null);
  assert.ok(fixture.state.uploadPaths.every(path => !existsSync(path)));
});

test('a published product and channel remain readable and promotion preserves Stable', () => {
  const fixture = publisherFixture({ published: true, channel: 'published' });
  const stable = fixture.active('stable.json');
  publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
  assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.1');
  assert.deepEqual(fixture.active('stable.json'), stable);
  assert.ok(fixture.state.mutations.every(value => !value.startsWith('publish:') && !value.startsWith('create:')));
});

test('a bad artifact signature leaves the product draft and does not create a channel', () => {
  const fixture = publisherFixture({ tampered: true });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /signature verification failed/);
  assert.equal(fixture.state.product.draft, true);
  assert.equal(fixture.state.channel, null);
  assert.deepEqual(fixture.state.mutations, []);
});

test('an existing draft channel is detected and fails closed before product publication', () => {
  const fixture = publisherFixture({ channel: 'draft' });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /channel release is still a draft/);
  assert.equal(fixture.state.product.draft, true);
  assert.equal(fixture.state.channel.draft, true);
  assert.deepEqual(fixture.state.mutations, []);
});

for (const failure of ['HTTP 403', 'HTTP 404', 'network unavailable']) {
  test(`optional channel lookup does not turn ${failure} into an absent channel`, () => {
    const error = new Error(failure);
    error.stderr = Buffer.from(`gh: ${failure}`);
    const fixture = publisherFixture({ listErrorAt: 2, apiError: error });
    assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), value => value === error);
    assert.equal(fixture.state.product.draft, true);
    assert.deepEqual(fixture.state.mutations, []);
  });
}

test('a release-ID lookup failure is not treated as an absent optional channel', () => {
  const error = new Error('HTTP 404 while reading an existing release ID');
  const fixture = publisherFixture({ channel: 'published', channelError: error });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), value => value === error);
  assert.equal(fixture.state.product.draft, true);
  assert.deepEqual(fixture.state.mutations, []);
});

test('duplicate matching releases are rejected before publication', () => {
  const fixture = publisherFixture({ duplicate: true });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /ambiguous GitHub release/);
  assert.deepEqual(fixture.state.mutations, []);
});

test('a lost rename response resolves the fresh channel while it is still a draft', () => {
  const fixture = publisherFixture({ lostRenameResponse: true });
  publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
  assert.equal(fixture.state.lostResponse, true);
  assert.equal(fixture.state.channel.draft, false);
  assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.1');
});

test('a rejected rename restores the previous channel through draft-compatible metadata reads', () => {
  const fixture = publisherFixture({ channel: 'published', failRename: true });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /rename rejected/);
  assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.0');
  assert.equal(fixture.active('stable.json').version, '0.1.20');
  assert.ok(fixture.state.uploadPaths.every(path => !existsSync(path)));
});
