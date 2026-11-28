import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { existsSync, readFileSync, mkdtempSync, writeFileSync, rmSync, mkdirSync, symlinkSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';
import { compareVersions, makeManifest, parseVersion, promotionChannel, publishOnly, publishAndSync, publishDual, syncPublished, resumeVerifiedDraft, promote as promoteFile, repository, shouldPromote, verifyTauriSignature } from '../updater-release.mjs';
import { attachDesktopCandidates, desktopArtifactIds, desktopReleaseTargets, expectedDesktopCandidateFiles, readStandaloneCandidates, standaloneArtifactIds, stageDesktopCandidate } from '../updater-release.mjs';

const gateNames = ['public_assets', 'appimage_updates', 'windows_upgrade', 'performance'];
const gateReport = (receipt, kind) => ({ schema_version: 1, kind, status: 'passed',
  version: receipt.version, source_revision: receipt.source_revision, release_id: receipt.release_id,
  signed_assets: receipt.signed_assets, checks: [{ name: `synthetic-${kind}`, status: 'passed' }] });
const reportBytes = (receipt, gate) => Buffer.from(JSON.stringify(gateReport(receipt, gate)));
const accepted = receipt => ({ ...receipt, status: 'passed', gates: Object.fromEntries(gateNames.map(gate => [gate,
  { file: `${gate}.json`, sha256: createHash('sha256').update(reportBytes(receipt, gate)).digest('hex') }])) });
function promote(version, config, receipt, log, transport, options = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'dezirclip-acceptance-test-'));
  try {
    for (const gate of gateNames) {
      if (options.missing === gate) continue;
      const report = gateReport(receipt, gate);
      if (options.failed === gate) report.status = 'failed';
      if (options.wrongSource === gate) report.source_revision = 'd'.repeat(40);
      if (options.changeReport && gate === 'performance') options.changeReport(report);
      const bytes = options.rawReport && gate === 'performance'
        ? options.rawReport : Buffer.from(JSON.stringify(report));
      writeFileSync(join(directory, `${gate}.json`), bytes);
      if (options.rehash && receipt.gates?.[gate]) receipt.gates[gate].sha256 = createHash('sha256').update(bytes).digest('hex');
    }
    const path = join(directory, 'acceptance.json');
    writeFileSync(path, JSON.stringify(receipt));
    options.setup?.(directory, path);
    return promoteFile(version, config, path, log, transport);
  } finally {
    assert.equal(dirname(resolve(directory)), resolve(tmpdir()));
    assert(basename(directory).startsWith('dezirclip-acceptance-test-'));
    rmSync(directory, { recursive: true, force: true });
  }
}
function publish(version, config, log, transport) {
  const receipt = publishOnly(version, config, log, transport);
  return promote(version, config, accepted(receipt), log, transport);
}

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
  const version = options.version ?? '0.2.0-alpha.1';
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
      prerelease: Boolean(parseVersion(version).pre.length), body: 'Synthetic release notes', assets: [] },
    channel: null,
  };
  const asset = (name, data) => {
    const value = { id: nextId++, name, size: data.length, updated_at: '2026-10-20T00:00:00Z',
      state: 'uploaded', digest: `sha256:${createHash('sha256').update(data).digest('hex')}` };
    bytes.set(value.id, Buffer.from(data));
    return value;
  };
  const suffixes = ['x64-setup.exe', 'arm64-setup.exe', 'amd64.AppImage', 'aarch64.AppImage'];
  const signedNames = suffixes.map(suffix => `DezirClip_${version}_${suffix}`);
  if (options.dual) signedNames.push(
    ...['x64', 'arm64'].map(arch => `DezirClip-v${version}-windows-${arch}-MyGo-setup.exe`),
    ...['amd64', 'aarch64'].map(arch => `DezirClip-MyGo_${version}_${arch}.AppImage`));
  for (const name of signedNames) {
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
  for (const arch of options.dual ? [] : ['x86_64', 'aarch64']) {
    state.product.assets.push(asset(`DezirClip-${version}-linux-${arch}.flatpak`, Buffer.from('flatpak')));
  }
  if (options.sync) {
    const hasMyGo = compareVersions(version, '0.2.0-alpha.9') >= 0;
    if (hasMyGo && !options.dual) state.product.assets.push(asset(`DezirClip-v${version}-windows-x64-MyGo${version === '0.2.0-alpha.9' ? '.zip' : '-setup.exe'}`, Buffer.from('standalone MyGo package')));
    for (const arch of ['amd64', 'arm64']) {
      const name = `DezirClip_${version}_${arch}.deb`;
      state.product.assets.push(asset(name, Buffer.from('deb')), asset(`${name}.sig`, Buffer.from('signature')));
      if (options.dual) state.product.assets.push(asset(`DezirClip-MyGo_${version}_${arch}.deb`, Buffer.from('deb')),
        asset(`DezirClip-MyGo_${version}_${arch}.deb.sig`, Buffer.from('signature')));
    }
    const parsed = parseVersion(version);
    for (const arch of ['x86_64', 'aarch64']) {
      const suffix = `${parsed.numbers.join('.')}-${parsed.pre.length ? `0.${parsed.pre.join('.')}` : '1'}.${arch}.rpm`;
      state.product.assets.push(asset(`DezirClip-${suffix}`, Buffer.from('rpm')));
      if (options.dual) state.product.assets.push(asset(`DezirClip-MyGo-${suffix}`, Buffer.from('rpm')));
    }
    state.workflow = { id: 303481695, path: '.github/workflows/release.yml', state: 'active' };
    state.run = { id: 7001, run_attempt: 1, workflow_id: state.workflow.id, path: state.workflow.path,
      event: 'push', head_branch: tag, head_sha: 'a'.repeat(40),
      run_started_at: '2026-10-21T00:00:00Z',
      repository: { full_name: repository }, head_repository: { full_name: repository },
      status: options.running ? 'in_progress' : 'completed', conclusion: options.running ? null : 'success' };
    const names = [
      'Create GitHub release', 'Publish verified standalone archives',
      ...(compareVersions(version, '0.2.0-alpha.10') >= 0 ? [
        'Full source acceptance / Frontend',
        ...['windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64'].map(platform => `Full source acceptance / Tauri backend (${platform})`),
        ...['LegacyUninstall', 'OldToNew', 'PublishedAlphaToAlpha', 'OwnedUpgrade', 'OwnedExternalEdit', 'DeniedGuiUpgrade'].map(scenario => `Full source acceptance / Native NSIS (${scenario})`),
        ...['linux-x86_64', 'linux-aarch64'].map(platform => `Full source acceptance / Verified Flatpak / Flatpak (${platform})`),
      ] : []),
      ...(hasMyGo ? ['Windows x64 MyGo test download'] : []),
      ...['windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64'].map(platform => `Tauri release (${platform})`),
      ...['windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64']
        .map(platform => `Build compatible standalone CLI / Standalone (${platform})`),
      ...['linux-x86_64', 'linux-aarch64'].map(platform => `Verified Flatpak release / Flatpak (${platform})`),
      'Verify actual signed Windows upgrades / Verify final signed upgrade inputs',
      'Collect actual final package identities / Bind final source and all fourteen assets',
      'Verify actual signed Windows upgrades / Bind all actual final installer upgrades',
      ...['windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64']
        .map(platform => `Collect actual final package identities / Release identities (${platform})`),
      ...['LegacyToAlpha2', 'PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease']
        .map(scenario => `Verify actual signed Windows upgrades / Final published upgrade (${scenario})`),
      options.oldPublisher ? 'Verify signed artifacts and publish before public acceptance'
        : 'Verify signed artifacts, publish and synchronize update channel',
    ];
    const jobs = options.dual ? names.filter(name => name === 'Create GitHub release' || name === 'Publish verified standalone archives' ||
      name.startsWith('Tauri release') || name.startsWith('Build compatible standalone CLI') ||
      name.startsWith('Verify signed artifacts,')).map(name => name.replace(/^Tauri release/, 'Dual desktop release')) : names;
    state.jobs = jobs.map((name, index) => ({ id: 8000 + index, name, run_id: 7001, run_attempt: 1,
      head_sha: 'a'.repeat(40), status: options.running && index === jobs.length - 1 ? 'in_progress' : 'completed',
      conclusion: options.running && index === jobs.length - 1 ? null : 'success',
      started_at: '2026-10-21T00:00:00Z', completed_at: '2026-10-21T01:00:00Z' }));
  }
  if (options.tampered) bytes.get(state.product.assets[0].id)[0] ^= 1;
  if (options.arch) {
    const job = state.jobs.find(job => job.name === 'Dual desktop release (linux-x86_64)');
    job.steps = [{ name: 'Build Arch Linux packages and AUR recipes', status: 'completed', conclusion: 'success' }];
    for (const name of [
      'Frontend', 'Select desktop acceptance targets', 'Bind candidate source',
      'Tauri backend (linux-x86_64)', 'Tauri backend (windows-x86_64)',
      'Go development (ubuntu-24.04)', 'Go development (windows-latest)',
      'Rust development (ubuntu-24.04, core-tests)', 'Rust development (ubuntu-24.04, desktop-check)',
      'Rust development (windows-latest, core-tests)', 'Rust development (windows-latest, desktop-check)',
    ]) {
      state.jobs.push({ ...state.jobs[0], id: 8_000 + state.jobs.length, name: `Source acceptance / ${name}` });
    }
    state.jobs.push({ ...state.jobs[0], id: 8_000 + state.jobs.length, name: 'Upload verified desktop assets' });
    const archFiles = expectedDesktopCandidateFiles(tag, 'linux-x86_64').filter(name => name.startsWith('dezirclip-'));
    for (const name of archFiles) state.product.assets.push(asset(name, Buffer.from('arch package')));
  }
  if (options.channel) {
    state.channel = { id: 20, tag_name: 'updater-channels', draft: options.channel === 'draft',
      prerelease: true, assets: [
        asset('stable.json', Buffer.from('{"version":"0.1.20"}')),
        asset('preview.json', Buffer.from('{"version":"0.2.0-alpha.0"}')),
      ] };
  }
  const json = value => JSON.stringify(value);
  function recordMutation(stage) {
    state.mutations.push(stage);
    if (options.stopAfter === stage && !state.stoppedOnce) {
      state.stoppedOnce = state.interrupted = true;
      throw new Error('Synthetic publisher interruption');
    }
    if (options.loseResponseAfter === stage && !state.lostStageResponse) {
      state.lostStageResponse = true;
      throw new Error('Synthetic lost channel response');
    }
  }
  const transport = (args, binary = false) => {
    if (state.interrupted) throw new Error('Synthetic publisher interruption');
    state.calls.push([...args]);
    if (args[0] === 'api') {
      const path = args[1].replace(`repos/${repository}/`, '');
      assert.equal(args[1], `repos/${repository}/${path}`);
      options.onApi?.(path, state);
      if (path === 'releases?per_page=100') {
        assert.deepEqual(args.slice(2), ['--paginate', '--slurp']);
        if (++state.listCalls === options.listErrorAt) throw options.apiError;
        if (state.listCalls === options.renameAssetAt) state.product.assets[0].name += '.renamed';
        const matches = [state.product, ...(state.channel ? [state.channel] : [])];
        if (options.duplicate) matches.push({ ...state.product, id: 11 });
        // Put the target on a later page: checking only the first page fails.
        return json([[{ id: 1, tag_name: 'v0.0.0', draft: false }], matches]);
      }
      if (path.startsWith('releases/tags/')) {
        assert.fail('Publisher must not use the published-only by-tag endpoint for drafts');
      }
      if (path === 'actions/workflows/release.yml') return json(state.workflow);
      if (path === 'actions/runs/7001' || path === 'actions/runs/7001/attempts/1') {
        if (path.endsWith('/attempts/1') && ++state.runReads === options.changeRunAt) state.run.head_sha = 'b'.repeat(40);
        return json(state.run);
      }
      if (path === 'actions/runs/7001/artifacts?per_page=100') {
        assert.deepEqual(args.slice(2), ['--paginate', '--slurp']);
        return json([{ artifacts: state.artifacts }]);
      }
      if (path === 'actions/runs/7001/attempts/1/jobs?per_page=100') {
        assert.deepEqual(args.slice(2), ['--paginate', '--slurp']);
        const jobs = options.incompleteJobs ? state.jobs.slice(1) : state.jobs;
        return json([{ total_count: state.jobs.length, jobs: jobs.slice(0, 4) },
          { total_count: state.jobs.length, jobs: jobs.slice(4) }]);
      }
      if (path === `git/ref/tags/${tag}`) return json({ object: { type: 'commit', sha: 'a'.repeat(40) } });
      if (options.dual && /^releases\/(10|20)\/assets\?per_page=100$/.test(path)) {
        assert.deepEqual(args.slice(2), ['--paginate', '--slurp']);
        const release = path.startsWith('releases/10') ? state.product : state.channel;
        return json([release.assets.slice(0, 30), release.assets.slice(30)]);
      }
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
          const activating = name === 'preview.json' && /^preview-(0\.2\.0|pending-)/.test(found.name);
          if (activating && options.failRename && !state.renameFailure) {
            state.renameFailure = true;
            throw new Error('Synthetic rename rejected');
          }
          found.name = name;
          recordMutation(name.startsWith('preview-previous-') && options.stopAfter === 'backup:preview'
            ? 'backup:preview' : `rename:${name}`);
          if (name.startsWith('preview-previous-') && options.lostOldRenameResponse && !state.lostOldResponse) {
            state.lostOldResponse = true;
            throw new Error('Synthetic lost old rename response');
          }
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
      state.channel = { id: 20, tag_name: target, draft: true, prerelease: true, assets: [],
        body: args[args.indexOf('--notes') + 1] };
      recordMutation('create:channel-draft');
      return '';
    }
    if (action === 'upload' && target === 'updater-channels') {
      if (options.failUpload) throw new Error('Synthetic upload failed');
      const path = args[3];
      state.uploadPaths.push(path);
      state.channel.assets.push(asset(basename(path), readFileSync(path)));
      recordMutation('upload:channel');
      options.afterUpload?.(state, bytes);
      return '';
    }
    if (action === 'edit' && (target === tag || target === 'updater-channels')) {
      assert.ok(args.includes('--draft=false') && args.includes(target === tag && !state.product.prerelease ? '--latest' : '--latest=false'));
      const release = target === tag ? state.product : state.channel;
      release.draft = false;
      recordMutation(target === tag ? 'publish:product' : 'publish:channel');
      return '';
    }
    assert.fail('Unexpected fake release command: ' + args.join(' '));
  };
  state.runReads = 0;
  return { state, transport, config: { plugins: { updater: { pubkey: signingKey } } },
    replaceIndex: (name, value) => {
      const found = state.channel.assets.find(item => item.name === name);
      assert.ok(found);
      const raw = Buffer.from(JSON.stringify(value)); bytes.set(found.id, raw);
      found.size = raw.length; found.digest = `sha256:${createHash('sha256').update(raw).digest('hex')}`;
    },
    active: name => {
      const found = state.channel?.assets.find(value => value.name === name);
      return found ? JSON.parse(bytes.get(found.id)) : null;
    } };
}

test('one atomic channel index carries both verified desktop variants without Flatpak', () => {
  const version = '0.2.0-beta.4';
  const { state, config, transport, active } = publisherFixture({ version, sync: true, dual: true, running: true });
  const receipt = publishDual('publish-dual', version, config, 7001, 1, () => {}, transport);
  const manifest = active('preview.json');
  assert.equal(state.product.assets.length, 36);
  assert.equal(state.product.assets.filter(asset => asset.name.endsWith('.zip')).length, 2);
  assert(state.product.assets.filter(asset => asset.name.endsWith('.zip')).every(asset => asset.name.startsWith('dzc-')));
  assert.equal(receipt.signed_assets.length, 16);
  assert.equal(receipt.status, 'synchronized');
  assert.equal(receipt.workflow.run_started_at, '2026-10-21T00:00:00Z');
  assert.equal(Object.keys(manifest.mygo_platforms).length, 4);
  assert.match(manifest.platforms['linux-aarch64'].url, /DezirClip_.*_aarch64\.AppImage$/);
  assert.match(manifest.mygo_platforms['linux-aarch64'].url, /DezirClip-MyGo_.*_aarch64\.AppImage$/);
  assert.match(manifest.mygo_platforms['windows-aarch64'].url, /windows-arm64-MyGo-setup\.exe$/);
});

test('dual channel publication resumes the same assets after interruption at each remote transition', () => {
  const version = '0.2.0-beta.4';
  for (const [stopAfter, channel] of [
    ['publish:product', undefined], ['create:channel-draft', undefined],
    ['upload:channel', undefined], ['backup:preview', 'published'],
    ['rename:preview.json', undefined], ['publish:channel', undefined],
  ]) {
    const fixture = publisherFixture({ version, sync: true, dual: true, running: true, channel, stopAfter });
    const { state, config, transport, active } = fixture;
    const publish = mode => publishDual(mode, version, config, 7001, 1, () => {}, transport);
    assert.throws(() => publish('publish-dual'), /Synthetic publisher interruption/, stopAfter);
    const previouslyStaged = state.channel?.assets.find(value => value.name.startsWith('preview-pending-'));
    const previouslyActivated = state.channel?.assets.find(value => value.name === 'preview.json');
    state.interrupted = false;
    state.run.status = 'completed';
    state.run.conclusion = 'failure';
    Object.assign(state.jobs.at(-1), { status: 'completed', conclusion: 'failure' });
    const result = publish('sync-dual-channel');
    assert.equal(result.channel_version, version, stopAfter);
    assert.equal(state.product.draft, false);
    assert.equal(state.channel.draft, false);
    assert.equal(active('preview.json').version, version);
    assert.equal(state.mutations.filter(value => value === 'upload:channel').length, 1);
    assert.equal(state.mutations.filter(value => value === 'publish:product').length, 1);
    const activated = state.channel.assets.find(value => value.name === 'preview.json');
    if (previouslyStaged || (previouslyActivated && !channel)) {
      assert.equal(activated.id, (previouslyStaged ?? previouslyActivated).id, stopAfter);
    }
    const before = state.mutations.length;
    assert.equal(publish('sync-dual-channel').status, 'already_synchronized');
    assert.equal(state.mutations.length, before);
  }
});

test('dual channel writes refuse unexplained drafts, missing active manifests and foreign staging', () => {
  const version = '0.2.0-beta.4';
  for (const change of [
    state => { state.channel.draft = true; },
    state => { state.channel.assets.find(value => value.name === 'preview.json').name = 'preview-previous-89.json'; },
    state => { state.channel.assets.push({ ...state.channel.assets[0], name: 'preview-pending-foreign.json' }); },
  ]) {
    const { state, config, transport } = publisherFixture({ version, sync: true, dual: true, running: true, channel: 'published' });
    change(state);
    assert.throws(() => publishDual('publish-dual', version, config, 7001, 1, () => {}, transport),
      /ownership is unknown|no matching candidate|another candidate/);
    assert.deepEqual(state.mutations, []);
  }
});

test('dual channel resolves lost draft, upload and activation responses on the original IDs', () => {
  const version = '0.2.0-beta.4';
  for (const loseResponseAfter of ['create:channel-draft', 'upload:channel', 'rename:preview.json', 'publish:channel']) {
    const { state, config, transport, active } = publisherFixture({ version, sync: true, dual: true, running: true, loseResponseAfter });
    const result = publishDual('publish-dual', version, config, 7001, 1, () => {}, transport);
    assert.equal(result.status, 'synchronized', loseResponseAfter);
    assert.equal(state.channel.id, 20);
    assert.equal(state.mutations.filter(value => value === 'upload:channel').length, 1);
    assert.equal(active('preview.json').version, version);
  }
});

test('dual channel refuses to resume with modified staged bytes', () => {
  const version = '0.2.0-beta.4';
  const { state, config, transport, replaceIndex } = publisherFixture({
    version, sync: true, dual: true, running: true, channel: 'published', stopAfter: 'backup:preview',
  });
  const publish = mode => publishDual(mode, version, config, 7001, 1, () => {}, transport);
  assert.throws(() => publish('publish-dual'), /Synthetic publisher interruption/);
  state.interrupted = false;
  const staged = state.channel.assets.find(value => value.name.startsWith('preview-pending-'));
  replaceIndex(staged.name, { version });
  state.run.status = 'completed';
  state.run.conclusion = 'failure';
  Object.assign(state.jobs.at(-1), { status: 'completed', conclusion: 'failure' });
  const before = state.mutations.length;
  assert.throws(() => publish('sync-dual-channel'), /differs from its verified content/);
  assert.equal(state.mutations.length, before);
});

test('missing MyGo targets, wrong signatures and incomplete desktop jobs block release and channel writes', () => {
  const version = '0.2.0-beta.4';
  for (const modify of [
    state => state.product.assets.pop(),
    state => { state.jobs.find(job => job.name === 'Dual desktop release (linux-aarch64)').conclusion = 'failure'; },
    state => { state.product.assets.find(asset => asset.name.endsWith('windows-arm64-MyGo-setup.exe.sig')).digest = `sha256:${'0'.repeat(64)}`; },
  ]) {
    const { state, config, transport } = publisherFixture({ version, sync: true, dual: true, running: true });
    modify(state);
    assert.throws(() => publishDual('publish-dual', version, config, 7001, 1, () => {}, transport));
    assert.deepEqual(state.mutations, []);
  }
});

test('Arch packages and AUR recipes must be present before a Release with Arch packaging is promoted', () => {
  const version = '0.2.0-beta.4';
  const build = () => publisherFixture({ version, sync: true, dual: true, running: true, arch: true });
  const publish = ({ config, transport }) => publishDual('publish-dual', version, config, 7001, 1, () => {}, transport);
  const released = build();
  const { workflow } = publish(released);
  assert.equal(workflow.arch_packages, true);
  assert.equal(workflow.source_acceptance, true);
  assert.equal(released.state.product.assets.length, 40);

  const missingRecipe = build();
  missingRecipe.state.product.assets.pop();
  assert.throws(() => publish(missingRecipe), /All 40 release assets/);
  assert.deepEqual(missingRecipe.state.mutations, []);

  const failedStep = build();
  failedStep.state.jobs.find(job => job.steps).steps[0].conclusion = 'failure';
  assert.throws(() => publish(failedStep), /Arch Linux package verification/);
  assert.deepEqual(failedStep.state.mutations, []);
  const skippedArch = build();
  skippedArch.state.jobs.find(job => job.steps).steps = [];
  assert.throws(() => publish(skippedArch), /Arch Linux package verification/);
  assert.deepEqual(skippedArch.state.mutations, []);

  for (const conclusion of ['skipped', 'failure', 'cancelled', 'missing']) {
    const candidate = build();
    const jobs = candidate.state.jobs;
    const index = jobs.findIndex(job => job.name === 'Source acceptance / Bind candidate source');
    if (conclusion === 'missing') jobs.splice(index, 1);
    else jobs[index].conclusion = conclusion;
    assert.throws(() => publish(candidate), /gates/);
    assert.deepEqual(candidate.state.mutations, []);
  }
});

test('a current workflow publishes only the asset identities read back from its original draft', () => {
  const version = '0.2.1-beta.1';
  const tag = `v${version}`;
  const make = (native = false, linux = false) => {
    const fixture = publisherFixture({ version, sync: true, dual: true, running: true, arch: true });
    const readbacks = { releaseId: String(fixture.state.product.id) };
    for (const kind of ['desktop', 'standalone']) {
      const names = fixture.state.product.assets.filter(asset => asset.name.startsWith('dzc-') === (kind === 'standalone'));
      readbacks[kind] = { schema_version: 1, kind, tag, source_revision: 'a'.repeat(40), release_id: readbacks.releaseId,
        run_id: 7001, run_attempt: 1, assets: names.map(asset => ({ id: asset.id, size: asset.size,
          name: asset.name, sha256: asset.digest.slice(7) })) };
    }
    readbacks.desktop.archives = desktopReleaseTargets.flatMap(target =>
      expectedDesktopCandidateFiles(tag, target).filter(name => !name.endsWith('.sig')).map(name => ({
        name, target, sha256: readbacks.desktop.assets.find(asset => asset.name === name).sha256,
        members: name.endsWith('.src.tar.gz') ? {} : Object.fromEntries(['desktop', 'cli', 'service'].map(role =>
          [role, { path: role, sha256: 'c'.repeat(64) }])),
      })));
    if (native) {
      const variants = [
        { variant: 'tauri', current: `DezirClip_${version}_x64-setup.exe`,
          previous: 'DezirClip_0.2.0_x64-setup.exe', sha256: 'e9406e6da0ad5cbe64d1f45209ac82e34d8161d5d6ce2da50737f835c83e25dd' },
        { variant: 'mygo', current: `DezirClip-v${version}-windows-x64-MyGo-setup.exe`,
          previous: 'DezirClip-v0.2.0-windows-x64-MyGo-setup.exe', sha256: 'f7d408f94eec8118767457db90c43ef5e232ae0b6cce63d8c80c0704c33fca92' },
      ];
      readbacks.windows = variants.map(({ variant, current, previous, sha256 }) => {
        const jobs = fixture.state.jobs;
        jobs.push({ ...jobs[0], id: 8_000 + jobs.length, name: `Verify final Windows installer (${variant})` });
        const archive = readbacks.desktop.archives.find(item => item.name === current);
        return { schema_version: 1, origin: 'release-producer', status: 'passed', variant, tag, target: 'windows-x86_64',
          source_revision: 'a'.repeat(40), run_id: 7001, run_attempt: 1, producer_artifact_id: '925',
          current: { name: current, tag, source_revision: 'a'.repeat(40), sha256: archive.sha256 },
          previous: { name: previous, tag: 'v0.2.0', source_revision: 'ba0ad71f7b9d8caddac45d0bc5588254a8273482', sha256 },
          members: structuredClone(archive.members) };
      });
    }
    if (linux) {
      const targets = ['linux-x86_64', 'linux-aarch64'];
      fixture.state.artifacts = targets.map((target, index) => ({
        id: 950 + index, name: `release-desktop-${target}-1`, digest: 'sha256:' + 'd'.repeat(64),
        expired: false, workflow_run: { id: 7001, head_sha: 'a'.repeat(40), repository_id: 90, head_repository_id: 90 },
      }));
      readbacks.linux = targets.map((target, index) => {
        const jobs = fixture.state.jobs;
        jobs.push({ ...jobs[0], id: 8_000 + jobs.length, name: `Verify final Linux desktop runtime (${target})` });
        return { schema_version: 1, origin: 'release-producer', status: 'passed', tag, target,
          source_revision: 'a'.repeat(40), run_id: 7001, run_attempt: 1,
          producer_artifact_id: String(fixture.state.artifacts[index].id),
          packages: readbacks.desktop.archives.filter(archive => archive.target === target && !archive.name.endsWith('.src.tar.gz'))
            .map(({ name, sha256, members }) => ({ name, sha256, members: structuredClone(members),
              size: readbacks.desktop.assets.find(asset => asset.name === name).size,
              runtime: name.endsWith('.AppImage') ? 'appimage' : 'extracted-native-launcher',
              history_rendered: true, service_verified: true })),
        };
      });
    }
    return { ...fixture, readbacks };
  };
  const publish = fixture => publishDual('publish-dual', version, fixture.config, 7001, 1, () => {},
    fixture.transport, fixture.readbacks);
  assert.equal(publish(make()).status, 'synchronized');
  assert.equal(publish(make(true)).status, 'synchronized');
  assert.equal(publish(make(true, true)).status, 'synchronized');
  for (const change of [
    fixture => { delete fixture.readbacks.linux; },
    fixture => { fixture.readbacks.linux[1].origin = 'published-replay'; },
    fixture => { fixture.readbacks.linux[1].producer_artifact_id = '950'; },
    fixture => { fixture.readbacks.linux[0].packages.pop(); },
    fixture => { fixture.readbacks.linux[0].packages[0].history_rendered = false; },
    fixture => { fixture.readbacks.linux[1].packages[0].size += 1; },
    fixture => { fixture.readbacks.linux[1].packages[0].members.cli.sha256 = 'f'.repeat(64); },
    fixture => { fixture.state.jobs.at(-1).conclusion = 'failure'; },
  ]) {
    const fixture = make(true, true);
    change(fixture);
    assert.throws(() => publish(fixture), /Linux|gates|inventory differs/);
    assert.deepEqual(fixture.state.mutations, []);
  }
  for (const change of [
    fixture => { delete fixture.readbacks.desktop; },
    fixture => { fixture.readbacks.releaseId = '11'; },
    fixture => { fixture.readbacks.desktop.run_attempt = 2; },
    fixture => { fixture.readbacks.standalone.source_revision = 'b'.repeat(40); },
    fixture => { fixture.readbacks.standalone.assets.pop(); },
    fixture => { fixture.readbacks.desktop.assets[0].id += 900; },
    fixture => { fixture.readbacks.desktop.assets[0].sha256 = 'b'.repeat(64); },
  ]) {
    const fixture = make();
    change(fixture);
    assert.throws(() => publish(fixture), /readback/i);
    assert.deepEqual(fixture.state.mutations, []);
  }
  for (const change of [
    inspection => inspection.pop(),
    inspection => { inspection[0].target = 'windows-aarch64'; },
    inspection => { inspection[0].sha256 = 'd'.repeat(64); },
    inspection => { delete inspection[0].members.service; },
  ]) {
    const fixture = make();
    change(fixture.readbacks.desktop.archives);
    assert.throws(() => publish(fixture), /package inspection|packaged executable|inventory differs/i);
    assert.deepEqual(fixture.state.mutations, []);
  }
  for (const change of [
    fixture => { delete fixture.readbacks.windows; },
    fixture => { fixture.readbacks.windows[0].origin = 'published-replay'; },
    fixture => { fixture.readbacks.windows[0].current.sha256 = 'f'.repeat(64); },
    fixture => { fixture.readbacks.windows[0].previous.sha256 = 'f'.repeat(64); },
    fixture => { fixture.readbacks.windows[0].members.service.sha256 = 'f'.repeat(64); },
    fixture => { fixture.readbacks.windows[0].producer_artifact_id = '926'; },
    fixture => { fixture.state.jobs.pop(); },
  ]) {
    const fixture = make(true);
    change(fixture);
    assert.throws(() => publish(fixture), /Windows|gates/i);
    assert.deepEqual(fixture.state.mutations, []);
  }
});

test('desktop download resolves only unique artifacts from the exact candidate attempt', () => {
  const candidate = { source: 'a'.repeat(40), runId: '7001', attempt: '2' };
  const artifacts = desktopReleaseTargets.map((target, index) => ({
    id: 900 + index, name: `release-desktop-${target}-2`, expired: false,
    workflow_run: { id: 7001, head_sha: candidate.source, repository_id: 9, head_repository_id: 9 },
    digest: 'sha256:' + 'b'.repeat(64),
  }));
  const transport = entries => args => {
    assert.deepEqual(args, ['api', `repos/${repository}/actions/runs/7001/artifacts?per_page=100`, '--paginate', '--slurp']);
    return JSON.stringify([{ artifacts: entries.slice(0, 2) }, { artifacts: entries.slice(2) }]);
  };
  const [first, ...remaining] = artifacts;
  const ids = desktopArtifactIds(candidate, transport([
    { ...first, id: 42, name: first.name.replace(/-2$/, '-1') }, ...artifacts,
  ]));
  assert.deepEqual(ids, artifacts.map(artifact => artifact.id));
  const standalone = artifacts.map((artifact, index) => ({ ...artifact, name: `release-cli-2-${desktopReleaseTargets[index]}` }));
  assert.deepEqual(standaloneArtifactIds(candidate, transport(standalone)), ids);
  assert.throws(() => standaloneArtifactIds(candidate, transport(artifacts)), /missing/);
  assert.throws(() => desktopArtifactIds(candidate, transport(remaining)), /missing/);
  assert.throws(() => desktopArtifactIds(candidate, transport([...artifacts, first])), /does not identify/);
  assert.throws(() => desktopArtifactIds(candidate, transport([{ ...first, id: remaining[0].id }, ...remaining])), /not unique/);
  for (const other of [
    { expired: true }, { digest: null },
    { workflow_run: { ...first.workflow_run, head_sha: 'c'.repeat(40) } },
    { workflow_run: { ...first.workflow_run, id: 7002 } },
    { workflow_run: { ...first.workflow_run, head_repository_id: 11 } },
  ]) {
    assert.throws(() => desktopArtifactIds(candidate, transport([{ ...first, ...other }, ...remaining])), /does not identify/);
  }
});

test('standalone attachment requires the exact archives and matching checksums', t => {
  const directory = mkdtempSync(join(tmpdir(), 'dezirclip-cli-assets-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const tag = 'v0.2.0-beta.4';
  const names = desktopReleaseTargets.map(target =>
    `dzc-0.2.0-beta.4-${target}.${target.startsWith('windows-') ? 'zip' : 'tar.gz'}`);
  for (const name of names) {
    const bytes = Buffer.from(name);
    const sha = createHash('sha256').update(bytes).digest('hex');
    writeFileSync(join(directory, name), bytes);
    const newline = name.endsWith('.zip') ? '\r\n' : '\n';
    writeFileSync(join(directory, `${name}.sha256`), `${sha}  ${name}${newline}`);
  }
  assert.deepEqual(readStandaloneCandidates(tag, directory).map(asset => asset.name),
    names.flatMap(name => [name, `${name}.sha256`]));
  writeFileSync(join(directory, names[0]), 'wrong payload');
  assert.throws(() => readStandaloneCandidates(tag, directory), /archive digest differs/);
  writeFileSync(join(directory, 'extra.txt'), 'untracked');
  assert.throws(() => readStandaloneCandidates(tag, directory), /inventory differs/);
});

test('the desktop consumer attaches verified producer bytes to the fixed draft and resumes matching bytes', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'dezirclip-desktop-assets-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const directory = join(root, 'artifacts');
  mkdirSync(directory);
  const candidate = { tag: 'v0.2.0-beta.4', source: 'a'.repeat(40), runId: '7001', attempt: '1', directory, releaseId: '10' };
  for (const target of desktopReleaseTargets) {
    const files = expectedDesktopCandidateFiles(candidate.tag, target).map(name => {
      const file = join(root, name);
      writeFileSync(file, `synthetic release byte stream for ${name}`);
      return file;
    });
    stageDesktopCandidate({ ...candidate, target, files, destination: join(directory, `release-desktop-${target}-1`) });
  }
  const assets = [];
  const bodies = new Map();
  let uploads = 0;
  const draft = { id: 10, tag_name: candidate.tag, draft: true };
  const fakeTransport = args => {
    const endpoint = args[1].replace(`repos/${repository}/`, '');
    if (args[0] === 'api') {
      if (endpoint === 'releases?per_page=100') return JSON.stringify([[draft]]);
      if (endpoint === 'releases/10') return JSON.stringify(draft);
      if (endpoint === 'releases/10/assets?per_page=100') return JSON.stringify([assets]);
      if (endpoint === `git/ref/tags/${candidate.tag}`) return JSON.stringify({ object: { type: 'commit', sha: candidate.source } });
      const match = /^releases\/assets\/(\d+)$/.exec(endpoint);
      assert(match, `Unexpected fixture endpoint: ${endpoint}`);
      return bodies.get(Number(match[1]));
    }
    assert.deepEqual(args.slice(0, 5), ['release', 'upload', candidate.tag, '--repo', repository]);
    uploads++;
    for (const file of args.slice(5)) {
      const body = readFileSync(file);
      const id = assets.length + 1;
      assets.push({ id, name: basename(file), state: 'uploaded', size: body.length,
        digest: 'sha256:' + createHash('sha256').update(body).digest('hex') });
      bodies.set(id, body);
    }
    return '';
  };
  assert.throws(() => attachDesktopCandidates({ ...candidate, releaseId: '11' }, fakeTransport), /original draft/);
  assert.throws(() => attachDesktopCandidates(candidate,
    () => assert.fail('package inspection must precede remote calls'),
    () => { throw new Error('package inspection failed'); }), /package inspection failed/);
  assert.equal(uploads, 0);
  const uploaded = attachDesktopCandidates(candidate, fakeTransport);
  assert.equal(uploaded.length, 32);
  assert.equal(uploads, 1);
  assert.deepEqual(attachDesktopCandidates(candidate, fakeTransport), uploaded);
  assert.equal(uploads, 1);
  const original = bodies.get(1);
  bodies.set(1, Buffer.from('different bytes despite stale metadata'));
  assert.throws(() => attachDesktopCandidates(candidate, fakeTransport), /Uploaded product bytes differ/);
  bodies.set(1, original);
  const windows = 'windows-x86_64';
  const path = join(directory, `release-desktop-${windows}-1`, expectedDesktopCandidateFiles(candidate.tag, windows)[0]);
  writeFileSync(path, 'altered bytes');
  assert.throws(() => attachDesktopCandidates(candidate, () => assert.fail('local digest mismatch must precede remote calls')), /digest differs/);
  assert.equal(uploads, 1);
});

function withSynchronization(options, body) {
  const fixture = publisherFixture({ sync: true, ...options });
  const version = options.version ?? '0.2.0-alpha.1';
  const directory = mkdtempSync(join(tmpdir(), 'dezirclip-sync-test-'));
  const file = join(directory, 'windows-upgrade.json');
  const gate = { schema_version: 1, kind: 'windows_upgrade', status: 'passed', version,
    source_revision: 'a'.repeat(40), release_id: 10, policy_sha256: 'c'.repeat(64),
    signed_assets: fixture.state.product.assets.slice(0, 8).map(({ name, id, size, updated_at, digest }) =>
      ({ name, id, size, updated_at, sha256: digest.slice(7) })),
    checks: ['LegacyToAlpha2', 'PublishedAlphaToAlpha2', 'PublishedAlpha2ToRelease'].map(name =>
      ({ name, status: 'passed', report_sha256: 'd'.repeat(64), raw_native_sha256: 'e'.repeat(64) })) };
  const invoke = (mode = 'publish-and-sync', runId = '7001', attempt = '1') => {
    writeFileSync(file, JSON.stringify(gate));
    const synchronize = mode === 'resume-verified-draft' ? resumeVerifiedDraft
      : mode === 'sync-published' ? syncPublished : publishAndSync;
    return synchronize(version, fixture.config,
      runId, attempt, file, () => {}, fixture.transport);
  };
  try { return body(fixture, gate, invoke, file); }
  finally { assert.equal(dirname(resolve(directory)), resolve(tmpdir())); rmSync(directory, { recursive: true, force: true }); }
}

test('release synchronization accepts the complete paginated gates and the unique running publisher', () => {
  withSynchronization({ running: true }, (fixture, gate, invoke) => {
    const receipt = invoke();
    assert.equal(receipt.status, 'synchronized');
    assert.equal(receipt.source_revision, 'a'.repeat(40));
    assert.equal(receipt.workflow.run_id, 7001);
    assert.equal(receipt.workflow.run_attempt, 1);
    assert.equal(receipt.signed_assets.length, 8);
    assert.equal(receipt.windows_upgrade_gate_sha256, createHash('sha256').update(JSON.stringify(gate)).digest('hex'));
    assert.equal(fixture.state.product.assets.length, 24);
    assert.equal(fixture.state.product.draft, false);
    assert.equal(fixture.state.channel.draft, false);
    assert.equal(fixture.active('preview.json').version, receipt.version);
    assert.equal(fixture.state.runReads, 2);
  });
});

for (const [label, change] of [
  ['foreign repository', fixture => { fixture.state.run.repository.full_name = 'other/repo'; }],
  ['foreign head repository', fixture => { fixture.state.run.head_repository.full_name = 'other/repo'; }],
  ['wrong workflow', fixture => { fixture.state.run.path = '.github/workflows/ci.yml'; }],
  ['wrong workflow ID', fixture => { fixture.state.run.workflow_id++; }],
  ['dispatch event', fixture => { fixture.state.run.event = 'workflow_dispatch'; }],
  ['wrong tag', fixture => { fixture.state.run.head_branch = 'master'; }],
  ['wrong source', fixture => { fixture.state.run.head_sha = 'b'.repeat(40); }],
  ['wrong attempt', fixture => { fixture.state.run.run_attempt = 2; }],
  ['wrong job source', fixture => { fixture.state.jobs[0].head_sha = 'b'.repeat(40); }],
  ['wrong job run', fixture => { fixture.state.jobs[0].run_id++; }],
  ['wrong job attempt', fixture => { fixture.state.jobs[0].run_attempt++; }],
  ['missing aggregate', fixture => { fixture.state.jobs.splice(13, 1); }],
  ['missing actual upgrade', fixture => { fixture.state.jobs.splice(19, 1); }],
  ['duplicate job ID', fixture => { fixture.state.jobs[1].id = fixture.state.jobs[0].id; }],
  ['duplicate publisher', fixture => { fixture.state.jobs.push({ ...fixture.state.jobs.at(-1), id: 9001, name: 'Verify signed artifacts and publish before public acceptance' }); }],
  ['nonpublisher running', fixture => { Object.assign(fixture.state.jobs[0], { status: 'in_progress', conclusion: null }); }],
  ['failed prerequisite', fixture => { fixture.state.jobs[0].conclusion = 'failure'; }],
  ['cancelled prerequisite', fixture => { fixture.state.jobs[0].conclusion = 'cancelled'; }],
  ['skipped prerequisite', fixture => { fixture.state.jobs[0].conclusion = 'skipped'; }],
  ['missing identity time', fixture => { delete fixture.state.jobs.find(job => job.name.includes('Release identities (')).started_at; }],
]) {
  test(`synchronization rejects ${label} without publication or channel writes`, () => {
    withSynchronization({}, (fixture, gate, invoke) => {
      change(fixture); assert.throws(() => invoke());
      assert.deepEqual(fixture.state.mutations, []);
      assert(fixture.state.calls.every(args => !args.includes('Accept: application/octet-stream')));
    });
  });
}

test('alpha.9 publishes the separate MyGo ZIP and retains the eight signed updater assets', () => {
  withSynchronization({ version: '0.2.0-alpha.9', running: true }, (fixture, gate, invoke) => {
    assert.equal(fixture.state.product.assets.length, 25);
    assert.equal(invoke().signed_assets.length, 8);
    assert.equal(fixture.state.product.draft, false);
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.9');
  });
});

for (const issue of ['missing ZIP', 'duplicate ZIP', 'unknown ZIP', 'late ZIP', 'missing job']) {
  test(`alpha.9 blocks publication with a ${issue}`, () => {
    withSynchronization({ version: '0.2.0-alpha.9', running: true }, (fixture, gate, invoke) => {
      const { product, jobs } = fixture.state;
      const zip = product.assets.find(asset => asset.name === 'DezirClip-v0.2.0-alpha.9-windows-x64-MyGo.zip');
      assert.ok(zip);
      if (issue === 'missing ZIP') product.assets.splice(product.assets.indexOf(zip), 1);
      if (issue === 'duplicate ZIP') product.assets.push({ ...zip, id: 9999 });
      if (issue === 'unknown ZIP') zip.name = 'other.zip';
      if (issue === 'late ZIP') zip.updated_at = '2026-10-22T00:00:00Z';
      if (issue === 'missing job') jobs.splice(jobs.findIndex(job => job.name === 'Windows x64 MyGo test download'), 1);
      assert.throws(() => invoke());
      assert.deepEqual(fixture.state.mutations, []);
    });
  });
}

test('only the exact conditional backfill job may be skipped on a push release', () => {
  withSynchronization({}, (fixture, gate, invoke) => {
    fixture.state.jobs.push({ ...fixture.state.jobs[0], id: 9001,
      name: 'Synchronize an already published update channel', conclusion: 'skipped' });
    assert.equal(invoke().status, 'synchronized');
  });
});

for (const name of ['Full source acceptance / Published Windows memory comparison',
  'Full source acceptance / native-file-drag',
  'Resume verified publication or synchronize update channel']) {
  for (const conclusion of ['skipped', 'failure', 'cancelled', null]) {
    test(`optional publication job ${name} accepts only success or intentional skip: ${conclusion}`, () => {
      withSynchronization({ version: '0.2.0-alpha.10', running: true }, (fixture, gate, invoke) => {
        fixture.state.jobs.push({ ...fixture.state.jobs[0], id: 9003, name,
          status: conclusion === null ? 'in_progress' : 'completed', conclusion });
        if (conclusion === 'skipped') assert.equal(invoke().status, 'synchronized');
        else { assert.throws(() => invoke(), /gates/); assert.deepEqual(fixture.state.mutations, []); }
      });
    });
  }
}

test('an incomplete paginated job listing or drifting gate source blocks every write', () => {
  for (const options of [{ incompleteJobs: true }, { changeRunAt: 2 }]) {
    withSynchronization(options, (fixture, gate, invoke) => {
      assert.throws(() => invoke()); assert.deepEqual(fixture.state.mutations, []);
    });
  }
});

for (const conclusion of ['success', 'skipped', 'failure']) {
  test(`beta.1 recovery with a manual-only native skip still requires the full Windows gate: ${conclusion}`, () => {
    withSynchronization({ version: '0.2.0-beta.1' }, (fixture, gate, invoke) => {
      fixture.state.run.conclusion = 'failure';
      fixture.state.jobs.at(-1).conclusion = 'failure';
      fixture.state.jobs.push({ ...fixture.state.jobs[0], id: 9004,
        name: 'Full source acceptance / native-file-drag', conclusion: 'skipped' });
      fixture.state.jobs.find(job => job.name === 'Full source acceptance / Tauri backend (windows-x86_64)').conclusion = conclusion;
      const assets = structuredClone(fixture.state.product.assets);
      if (conclusion === 'success') {
        assert.equal(invoke('resume-verified-draft').status, 'synchronized');
        assert.equal(fixture.active('preview.json').version, '0.2.0-beta.1');
        assert.deepEqual(fixture.state.product.assets, assets);
      } else {
        assert.throws(() => invoke('resume-verified-draft'), /gates/);
        assert.deepEqual(fixture.state.mutations, []);
      }
    });
  });
}

test('the optional native gate exemption does not accept a skipped matrix child', () => {
  withSynchronization({ version: '0.2.0-beta.1', running: true }, (fixture, gate, invoke) => {
    fixture.state.jobs.push({ ...fixture.state.jobs[0], id: 9004,
      name: 'Full source acceptance / native-file-drag (windows-latest)', conclusion: 'skipped' });
    assert.throws(() => invoke(), /gates/);
    assert.deepEqual(fixture.state.mutations, []);
  });
});

for (const [label, change] of [
  ['missing file', fixture => { fixture.state.product.assets.pop(); }],
  ['duplicate ID', fixture => { fixture.state.product.assets[1].id = fixture.state.product.assets[0].id; }],
  ['missing digest', fixture => { delete fixture.state.product.assets[0].digest; }],
  ['not uploaded', fixture => { fixture.state.product.assets[0].state = 'new'; }],
  ['post-gate replacement', fixture => { fixture.state.product.assets[0].updated_at = '2026-10-22T00:00:00Z'; }],
  ['wrong signed digest', fixture => { fixture.state.product.assets[0].digest = 'sha256:' + '0'.repeat(64); }],
]) {
  test(`all public release files are required: ${label}`, () => {
    withSynchronization({}, (fixture, gate, invoke) => {
      change(fixture); assert.throws(() => invoke()); assert.deepEqual(fixture.state.mutations, []);
    });
  });
}

for (const [label, change] of [
  ['failed gate', gate => { gate.status = 'failed'; }],
  ['other source', gate => { gate.source_revision = 'b'.repeat(40); }],
  ['other release', gate => { gate.release_id++; }],
  ['other signed bytes', gate => { gate.signed_assets[0].sha256 = '0'.repeat(64); }],
  ['other signed ID', gate => { gate.signed_assets[0].id++; }],
  ['missing native scenario', gate => { gate.checks.pop(); }],
  ['failed native scenario', gate => { gate.checks[0].status = 'failed'; }],
  ['missing original binding', gate => { delete gate.checks[0].report_sha256; }],
  ['missing native binding', gate => { delete gate.checks[0].raw_native_sha256; }],
]) {
  test(`the original Windows gate is mandatory: ${label}`, () => {
    withSynchronization({}, (fixture, gate, invoke) => {
      change(gate); assert.throws(() => invoke()); assert.deepEqual(fixture.state.mutations, []);
    });
  });
}

test('sync-published repairs the old successful publisher without republishing the product', () => {
  withSynchronization({ published: true, channel: 'published', oldPublisher: true }, (fixture, gate, invoke) => {
    const stable = fixture.active('stable.json');
    assert.equal(invoke('sync-published').status, 'synchronized');
    assert.deepEqual(fixture.active('stable.json'), stable);
    assert(fixture.state.mutations.every(value => !value.startsWith('publish:product')));
  });
});

test('sync-published permits only the unique publisher failure in an otherwise successful completed run', () => {
  withSynchronization({ published: true, channel: 'published' }, (fixture, gate, invoke) => {
    fixture.state.run.conclusion = 'failure'; fixture.state.jobs.at(-1).conclusion = 'failure';
    assert.throws(() => invoke(), /eligible/);
    assert.deepEqual(fixture.state.mutations, []);
    assert.equal(invoke('sync-published').status, 'synchronized');
  });
  for (const options of [{}, { published: true, running: true }]) {
    withSynchronization(options, (fixture, gate, invoke) => {
      assert.throws(() => invoke('sync-published'), /public release and completed/);
      assert.deepEqual(fixture.state.mutations, []);
    });
  }
  withSynchronization({ published: true }, (fixture, gate, invoke) => {
    fixture.state.run.conclusion = 'failure'; fixture.state.jobs[0].conclusion = 'failure';
    assert.throws(() => invoke('sync-published')); assert.deepEqual(fixture.state.mutations, []);
  });
});

test('verified draft recovery retains original signed bytes and accepts only a failed publisher', () => {
  withSynchronization({ version: '0.2.0-alpha.10' }, (fixture, gate, invoke) => {
    fixture.state.run.conclusion = 'failure'; fixture.state.jobs.at(-1).conclusion = 'failure';
    const assets = structuredClone(fixture.state.product.assets);
    const result = invoke('resume-verified-draft');
    assert.equal(result.status, 'synchronized');
    assert.equal(result.workflow.conclusion, 'failure');
    assert.equal(fixture.state.product.draft, false);
    assert.deepEqual(fixture.state.product.assets, assets);
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.10');
  });
});

for (const [label, change] of [
  ['public release', fixture => { fixture.state.product.draft = false; }],
  ['successful run', fixture => { fixture.state.run.conclusion = 'success'; fixture.state.jobs.at(-1).conclusion = 'success'; }],
  ['running publisher', fixture => {
    Object.assign(fixture.state.run, { status: 'in_progress', conclusion: null });
    Object.assign(fixture.state.jobs.at(-1), { status: 'in_progress', conclusion: null });
  }],
  ['failed source gate', fixture => { fixture.state.jobs.find(job => job.name === 'Full source acceptance / Frontend').conclusion = 'failure'; }],
  ['skipped source gate', fixture => { fixture.state.jobs.find(job => job.name === 'Full source acceptance / Frontend').conclusion = 'skipped'; }],
  ['changed source', fixture => { fixture.state.run.head_sha = 'b'.repeat(40); }],
  ['replaced asset', fixture => { fixture.state.product.assets[0].updated_at = '2026-10-22T00:00:00Z'; }],
  ['changed asset bytes', fixture => { fixture.state.product.assets[0].digest = 'sha256:' + '0'.repeat(64); }],
  ['failed upgrade evidence', (fixture, gate) => { gate.status = 'failed'; }],
  ['changed upgrade source', (fixture, gate) => { gate.source_revision = 'b'.repeat(40); }],
]) {
  test(`verified draft recovery refuses ${label} before any writes`, () => {
    withSynchronization({ version: '0.2.0-alpha.10' }, (fixture, gate, invoke) => {
      fixture.state.run.conclusion = 'failure'; fixture.state.jobs.at(-1).conclusion = 'failure';
      change(fixture, gate);
      assert.throws(() => invoke('resume-verified-draft'));
      assert.deepEqual(fixture.state.mutations, []);
    });
  });
}

test('the same channel version is idempotent only for the complete verified manifest', () => {
  withSynchronization({ published: true, channel: 'published' }, (fixture, gate, invoke) => {
    assert.equal(invoke('sync-published').status, 'synchronized');
    fixture.state.mutations = [];
    assert.equal(invoke('sync-published').status, 'already_synchronized');
    assert.deepEqual(fixture.state.mutations, []);
    const corrupt = fixture.active('preview.json'); corrupt.platforms['windows-x86_64'].signature = 'wrong';
    fixture.replaceIndex('preview.json', corrupt);
    assert.throws(() => invoke('sync-published'), /Equal-version/);
    assert.deepEqual(fixture.state.mutations, []);
  });
});

test('newer Preview is retained and a formal release advances only Stable', () => {
  withSynchronization({ published: true, channel: 'published' }, (fixture, gate, invoke) => {
    fixture.replaceIndex('preview.json', { version: '0.2.0-alpha.2' });
    assert.equal(invoke().status, 'newer_channel_preserved');
    assert.deepEqual(fixture.state.mutations, []);
  });
  withSynchronization({ version: '0.2.0', channel: 'published' }, (fixture, gate, invoke) => {
    const preview = fixture.active('preview.json');
    assert.equal(invoke().channel, 'stable');
    assert.equal(fixture.active('stable.json').version, '0.2.0');
    assert.deepEqual(fixture.active('preview.json'), preview);
  });
});

test('channel failures retain the old index and a lost activation response resolves by exact readback', () => {
  for (const options of [{ failRename: true }, { lostOldRenameResponse: true }, { failUpload: true }]) {
    withSynchronization({ published: true, channel: 'published', ...options }, (fixture, gate, invoke) => {
      const preview = fixture.active('preview.json');
      assert.throws(() => invoke()); assert.deepEqual(fixture.active('preview.json'), preview);
      assert.equal(fixture.state.product.draft, false);
    });
  }
  withSynchronization({ published: true, channel: 'published', lostRenameResponse: true }, (fixture, gate, invoke) => {
    assert.equal(invoke().status, 'synchronized'); assert.equal(fixture.state.lostResponse, true);
  });
});

test('a newer index arriving during upload is preserved rather than overwritten by a stale old index', () => {
  withSynchronization({ published: true, channel: 'published', afterUpload(state, payloads) {
    const active = state.channel.assets.find(asset => asset.name === 'preview.json');
    const raw = Buffer.from('{"version":"0.2.0-alpha.9"}'); payloads.set(active.id, raw); active.size = raw.length;
  } }, (fixture, gate, invoke) => {
    assert.throws(() => invoke(), /changed during staging/);
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.9');
    assert(fixture.state.mutations.every(value => !value.startsWith('rename:')));
  });
});

test('gate bytes, unsigned asset identities and the tag are rechecked before the first write', () => {
  for (const kind of ['gate', 'unsigned-asset', 'tag']) {
    let gatePath;
    withSynchronization({ onApi(route, state) {
      if (route !== 'actions/runs/7001/attempts/1' || state.runReads !== 1) return;
      if (kind === 'gate') writeFileSync(gatePath, '{}');
      if (kind === 'unsigned-asset') state.product.assets.at(-1).digest = 'sha256:' + '0'.repeat(64);
      if (kind === 'tag') state.run.head_sha = 'b'.repeat(40);
    } }, (fixture, gate, invoke, file) => {
      gatePath = file; assert.throws(() => invoke()); assert.deepEqual(fixture.state.mutations, []);
    });
  }
});

test('a successful run cannot conceal a failed publisher even in repair mode', () => {
  withSynchronization({ published: true }, (fixture, gate, invoke) => {
    fixture.state.jobs.at(-1).conclusion = 'failure';
    assert.throws(() => invoke('sync-published')); assert.deepEqual(fixture.state.mutations, []);
  });
});

test('assets cannot change between native identity collection and the later aggregate', () => {
  withSynchronization({}, (fixture, gate, invoke) => {
    fixture.state.jobs.find(job => job.name.includes('all fourteen')).started_at = '2026-10-22T00:00:00Z';
    fixture.state.jobs.find(job => job.name.includes('Bind all actual')).started_at = '2026-10-22T00:00:00Z';
    fixture.state.product.assets.at(-1).updated_at = '2026-10-21T00:00:01Z';
    assert.throws(() => invoke(), /pre-gate identities/); assert.deepEqual(fixture.state.mutations, []);
  });
});

test('an early Windows aggregate does not reject final uploads preceding all four identity collectors', () => {
  withSynchronization({}, (fixture, gate, invoke) => {
    fixture.state.jobs.find(job => job.name.includes('Bind all actual')).started_at = '2026-10-19T00:00:00Z';
    fixture.state.jobs.find(job => job.name.includes('all fourteen')).started_at = '2026-10-22T00:00:00Z';
    assert.equal(invoke().status, 'synchronized');
  });
});

test('normal channel reads may increase download counts without changing writer identity', () => {
  let reads = 0;
  withSynchronization({ published: true, channel: 'published', onApi(route, state) {
    if (route === 'releases/20' && ++reads === 2) {
      for (const asset of state.channel.assets) asset.download_count = (asset.download_count ?? 0) + 1;
    }
  } }, (fixture, gate, invoke) => {
    assert.equal(invoke().status, 'synchronized');
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.1');
  });
});

for (const field of ['id', 'digest', 'updated_at']) {
  test(`a changed active channel ${field} still blocks the first write`, () => {
    let reads = 0;
    withSynchronization({ published: true, channel: 'published', onApi(route, state) {
      if (route !== 'releases/20' || ++reads !== 2) return;
      const active = state.channel.assets.find(asset => asset.name === 'preview.json');
      if (field === 'id') active.id++;
      if (field === 'digest') active.digest = 'sha256:' + 'f'.repeat(64);
      if (field === 'updated_at') active.updated_at = '2026-10-22T00:00:00Z';
    } }, (fixture, gate, invoke) => {
      assert.throws(() => invoke(), /Updater channel changed before synchronization/);
      assert.deepEqual(fixture.state.mutations, []);
    });
  });
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

test('an existing draft channel blocks promotion after independent product publication', () => {
  const fixture = publisherFixture({ channel: 'draft' });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /channel release is still a draft/);
  assert.equal(fixture.state.product.draft, false);
  assert.equal(fixture.state.channel.draft, true);
  assert.deepEqual(fixture.state.mutations, ['publish:product']);
});

for (const failure of ['HTTP 403', 'HTTP 404', 'network unavailable']) {
  test(`optional channel lookup does not turn ${failure} into an absent channel`, () => {
    const error = new Error(failure);
    error.stderr = Buffer.from(`gh: ${failure}`);
    const fixture = publisherFixture({ listErrorAt: 5, apiError: error });
    assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), value => value === error);
    assert.equal(fixture.state.product.draft, false);
    assert.deepEqual(fixture.state.mutations, ['publish:product']);
  });
}

test('a release-ID lookup failure is not treated as an absent optional channel', () => {
  const error = new Error('HTTP 404 while reading an existing release ID');
  const fixture = publisherFixture({ channel: 'published', channelError: error });
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), value => value === error);
  assert.equal(fixture.state.product.draft, false);
  assert.deepEqual(fixture.state.mutations, ['publish:product']);
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

test('publish-only verifies and publishes without any channel lookup or mutation', () => {
  const fixture = publisherFixture({ channel: 'published', channelError: new Error('channel unavailable') });
  const receipt = publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
  assert.equal(receipt.status, 'awaiting_acceptance');
  assert.equal(receipt.signed_assets.length, 8);
  assert(receipt.signed_assets.every(asset => /^[a-f0-9]{64}$/.test(asset.sha256)));
  assert.deepEqual(fixture.state.mutations, ['publish:product']);
  assert(fixture.state.calls.every(args => !args.includes(`repos/${repository}/releases/20`)));
});

test('promotion refuses a draft product or incomplete public acceptance before channel writes', () => {
  const draft = publisherFixture();
  const draftReceipt = publishOnly('0.2.0-alpha.1', draft.config, () => {}, draft.transport);
  draft.state.product.draft = true;
  draft.state.mutations = [];
  assert.throws(() => promote('0.2.0-alpha.1', draft.config, accepted(draftReceipt), () => {}, draft.transport), /already published/);
  assert.deepEqual(draft.state.mutations, []);
  for (const change of [
    receipt => receipt,
    receipt => ({ ...accepted(receipt), source_revision: 'c'.repeat(40) }),
    receipt => ({ ...accepted(receipt), gates: { public_assets: 'b'.repeat(64) } }),
    receipt => ({ ...accepted(receipt), signed_assets: receipt.signed_assets.slice(1) }),
    receipt => ({ ...accepted(receipt), signed_assets: receipt.signed_assets.map((asset, index) => index ? asset : { ...asset, sha256: '0'.repeat(64) }) }),
  ]) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, change(receipt), () => {}, fixture.transport), /acceptance/);
    assert.deepEqual(fixture.state.mutations, []);
  }
});

test('promotion refuses changed accepted assets and preserves both channels', () => {
  const fixture = publisherFixture({ published: true, channel: 'published' });
  const receipt = publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport);
  const stable = fixture.active('stable.json');
  const preview = fixture.active('preview.json');
  fixture.state.product.assets[0].updated_at = '2026-10-21T00:00:00Z';
  assert.throws(() => promote('0.2.0-alpha.1', fixture.config, accepted(receipt), () => {}, fixture.transport), /completed public acceptance/);
  assert.deepEqual(fixture.state.mutations, []);
  assert.deepEqual(fixture.active('stable.json'), stable);
  assert.deepEqual(fixture.active('preview.json'), preview);
});

test('a lost old-index rename response restores the original active index', () => {
  const fixture = publisherFixture({ published: true, channel: 'published', lostOldRenameResponse: true });
  const preview = fixture.active('preview.json');
  const stable = fixture.active('stable.json');
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /lost old rename response/);
  assert.equal(fixture.state.lostOldResponse, true);
  assert.deepEqual(fixture.active('preview.json'), preview);
  assert.deepEqual(fixture.active('stable.json'), stable);
});

test('asset rename during fresh verification prevents promotion even if other metadata is unchanged', () => {
  const fixture = publisherFixture({ published: true, channel: 'published', renameAssetAt: 6 });
  const preview = fixture.active('preview.json');
  assert.throws(() => publish('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport), /assets changed during verification/);
  assert.deepEqual(fixture.state.mutations, []);
  assert.deepEqual(fixture.active('preview.json'), preview);
});

test('promotion reads gate files and rejects missing, tampered, failed or unbound reports', () => {
  for (const options of [{ missing: 'performance' }, { failed: 'appimage_updates' },
    { failed: 'windows_upgrade', rehash: true }, { wrongSource: 'public_assets', rehash: true }]) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = accepted(publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport));
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, receipt, () => {}, fixture.transport, options));
    assert.deepEqual(fixture.state.mutations, []);
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.0');
  }
});

test('promotion refuses hash-only declarations and paths outside the evidence bundle', () => {
  for (const reference of ['b'.repeat(64), { file: '../outside.json', sha256: 'b'.repeat(64) },
    ...['/outside.json', 'nested\\outside.json', 'nested/outside.json', 'C:outside.json',
      'performance.json:outside.json', '\\\\server\\outside.json', './performance.json', 'performance.json\u0000']
      .map(file => ({ file, sha256: 'b'.repeat(64) }))]) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = accepted(publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport));
    receipt.gates.performance = reference;
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, receipt, () => {}, fixture.transport));
    assert.deepEqual(fixture.state.mutations, []);
  }
});

test('matching hashes cannot approve failed checks or a report for another release identity', () => {
  for (const changeReport of [
    report => { report.schema_version = 2; },
    report => { report.kind = 'public_assets'; },
    report => { report.version = '0.2.0-alpha.0'; },
    report => { report.release_id++; },
    report => { report.signed_assets = report.signed_assets.slice(1); },
    report => { report.signed_assets = report.signed_assets.map((asset, index) => index ? asset : { ...asset, sha256: '0'.repeat(64) }); },
    report => { report.checks = []; },
    report => { report.checks.push({ name: 'failed-native-check', status: 'failed' }); },
    report => { report.checks[0].status = 'running'; },
    report => { report.checks[0].name = ''; },
    report => { report.checks[0].name = '   '; },
  ]) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = accepted(publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport));
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, receipt, () => {}, fixture.transport,
      { changeReport, rehash: true }), /matching passed public acceptance/);
    assert.deepEqual(fixture.state.mutations, []);
    assert.equal(fixture.active('preview.json').version, '0.2.0-alpha.0');
    assert.equal(fixture.active('stable.json').version, '0.1.20');
  }
});

test('promotion rejects malformed, oversized or non-file bundle evidence before remote calls', () => {
  const cases = [
    { rawReport: Buffer.from('{'), rehash: true },
    { rawReport: Buffer.from('null'), rehash: true },
    { rawReport: Buffer.alloc(8 * 1024 * 1024 + 1, 32), rehash: true },
    { setup: (directory, path) => writeFileSync(path, '{') },
    { setup: (directory, path) => rmSync(path) },
    { setup: directory => {
      const path = join(directory, 'performance.json');
      rmSync(path);
      mkdirSync(path);
    } },
  ];
  for (const options of cases) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = accepted(publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport));
    fixture.state.calls = [];
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, receipt, () => {}, fixture.transport, options));
    assert.deepEqual(fixture.state.calls, []);
    assert.deepEqual(fixture.state.mutations, []);
  }
});

test('promotion rejects symbolic links for a gate report or the acceptance file', t => {
  for (const linkAcceptance of [false, true]) {
    const fixture = publisherFixture({ published: true, channel: 'published' });
    const receipt = accepted(publishOnly('0.2.0-alpha.1', fixture.config, () => {}, fixture.transport));
    fixture.state.calls = [];
    let unsupported = false;
    assert.throws(() => promote('0.2.0-alpha.1', fixture.config, receipt, () => {}, fixture.transport, {
      setup: (directory, acceptancePath) => {
        const path = linkAcceptance ? acceptancePath : join(directory, 'performance.json');
        const target = join(directory, 'linked-target.json');
        writeFileSync(target, readFileSync(path));
        rmSync(path);
        try { symlinkSync(target, path, 'file'); }
        catch (error) {
          if (process.platform === 'win32' && error.code === 'EPERM') unsupported = true;
          throw error;
        }
      },
    }));
    assert.deepEqual(fixture.state.calls, []);
    if (unsupported) { t.skip('Windows account cannot create file symbolic links; Linux runs this rejection check.'); return; }
  }
});

test('later MyGo releases require an installer rather than a ZIP', () => {
  withSynchronization({ version: '0.2.0-alpha.10', running: true }, (fixture, gate, invoke) => {
    assert.ok(fixture.state.product.assets.some(asset => asset.name.endsWith('MyGo-setup.exe')));
    assert.equal(invoke().status, 'synchronized');
  });
  withSynchronization({ version: '0.2.0-alpha.10', running: true }, (fixture, gate, invoke) => {
    fixture.state.product.assets.find(asset => asset.name.endsWith('MyGo-setup.exe')).name = 'DezirClip-v0.2.0-alpha.10-windows-x64-MyGo.zip';
    assert.throws(() => invoke());
  });
});

test('public alpha.9 can retain a later installer supplement without weakening original asset gates', () => {
  withSynchronization({ version: '0.2.0-alpha.9', published: true }, (fixture, gate, invoke) => {
    const original = fixture.state.product.assets.find(asset => asset.name.endsWith('MyGo.zip'));
    fixture.state.product.assets.push({ ...original, id: 9999,
      name: 'DezirClip-v0.2.0-alpha.9-windows-x64-MyGo-setup.exe', updated_at: '2026-11-09T12:00:00Z' });
    assert.equal(invoke('sync-published').status, 'synchronized');
  });
});

test('only the supplemental MyGo packaging job may additionally skip tag publication', () => {
  withSynchronization({}, (fixture, gate, invoke) => {
    fixture.state.jobs.push({ ...fixture.state.jobs[0], id: 9002,
      name: 'Supplement published MyGo installer', conclusion: 'skipped' });
    assert.equal(invoke().status, 'synchronized');
  });
});

for (const missing of ['Full source acceptance / Tauri backend (windows-x86_64)', 'Full source acceptance / Native NSIS (OwnedUpgrade)', 'Full source acceptance / Verified Flatpak / Flatpak (linux-aarch64)']) {
  test(`new releases refuse a missing full-source gate: ${missing}`, () => {
    withSynchronization({ version: '0.2.0-alpha.10', running: true }, (fixture, gate, invoke) => {
      fixture.state.jobs = fixture.state.jobs.filter(job => job.name !== missing);
      assert.throws(() => invoke(), /gates/);
      assert.deepEqual(fixture.state.mutations, []);
    });
  });
}
