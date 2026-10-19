import assert from 'node:assert/strict';
import test from 'node:test';
import { compareVersions, makeManifest, parseVersion, promotionChannel, shouldPromote, verifyTauriSignature } from '../updater-release.mjs';

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
