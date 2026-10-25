import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = fileURLToPath(new URL('../set-release-version.mjs', import.meta.url));
const oldVersion = '0.2.0-alpha.2';
const newVersion = '0.2.0-alpha.3';
const members = ['crates/dzc-core', 'crates/dzc-protocol', 'src-tauri'];
const names = ['dzc-core', 'dzc-protocol', 'dezirclip'];
const targets = ['package.json', 'src-tauri/tauri.conf.json', 'Cargo.toml', 'Cargo.lock'];
const files = [...targets, ...members.map(member => member + '/Cargo.toml')];
const registryBlock = `[[package]]
name = "dzc-core"
version = "9.8.7"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "${'c'.repeat(64)}"
dependencies = ["registry-only-dependency"]
`;
const environment = Object.fromEntries(Object.entries(process.env).filter(([name]) =>
  !/^(?:GIT_|GITHUB_|GH_|TAURI_SIGNING_|CARGO_|RUST|DZC_|ACTIONS_)/i.test(name)));

function fixture(t, version = oldVersion) {
  const parent = resolve(tmpdir());
  const directory = resolve(mkdtempSync(join(parent, 'dezirclip-release-version-')));
  assert.equal(dirname(directory), parent);
  assert(basename(directory).startsWith('dezirclip-release-version-'));
  t.after(() => {
    assert.equal(dirname(directory), parent);
    assert(basename(directory).startsWith('dezirclip-release-version-'));
    rmSync(directory, { recursive: true, force: true });
  });
  const write = (relative, content) => {
    const path = join(directory, relative);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
    utimesSync(path, new Date('2000-01-01T00:00:00Z'), new Date('2000-01-01T00:00:00Z'));
  };
  write('package.json', `{ "private": true, "name": "release-fixture", "version": "${version}" }\n\n`);
  write('src-tauri/tauri.conf.json', `{\n\t"productName": "DezirClip",\n\t"version": "${version}"\n}\n`);
  write('Cargo.toml', `[workspace]
members = ["${members.join('", "')}"]
resolver = "2"

[workspace.package]
version = "${version}"
edition = "2021"

[workspace.metadata.release_notes]
version = "keep-this-unrelated-version"
`);
  for (let index = 0; index < members.length; index++) {
    write(members[index] + '/Cargo.toml', `[package]\nname = "${names[index]}"\nversion.workspace = true\n`);
  }
  write('Cargo.lock', '# fixture Cargo lock\nversion = 4\n\n' + names.map(name =>
    `[[package]]\nname = "${name}"\nversion = "${version}"\n`).join('\n') + '\n' + registryBlock);
  return { directory, write };
}

function snapshot(directory) {
  return Object.fromEntries(files.map(relative => {
    const path = join(directory, relative);
    return [relative, existsSync(path) ? { bytes: readFileSync(path), mtime: statSync(path, { bigint: true }).mtimeNs.toString() } : null];
  }));
}

function run(directory, args) {
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd: directory, env: environment, encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.signal, null);
  return result;
}

function unchangedFailure(directory, args, message) {
  const before = snapshot(directory);
  const result = run(directory, args);
  assert.notEqual(result.status, 0, result.stdout);
  if (message) assert.match(result.stderr, message);
  assert.deepEqual(snapshot(directory), before, 'a rejected request must preserve every target/member byte and mtime');
  return result;
}

function localLockVersions(directory) {
  const lock = readFileSync(join(directory, 'Cargo.lock'), 'utf8');
  return lock.split(/(?=^\[\[package\]\])/m).filter(block => /^name\s*=/m.test(block) && !/^source\s*=/m.test(block))
    .map(block => ({ name: block.match(/^name = "([^"]+)"/m)?.[1], version: block.match(/^version = "([^"]+)"/m)?.[1] }));
}

test('one version command synchronizes all four targets and three local packages without changing registry or member declarations', t => {
  const { directory } = fixture(t);
  const before = snapshot(directory);
  const result = run(directory, ['v' + newVersion]);
  assert.equal(result.status, 0, result.stderr);
  for (const name of ['package.json', 'src-tauri/tauri.conf.json']) {
    assert.equal(JSON.parse(readFileSync(join(directory, name), 'utf8')).version, newVersion);
  }
  const workspace = readFileSync(join(directory, 'Cargo.toml'), 'utf8');
  assert.match(workspace, /\[workspace\.package\]\nversion = "0\.2\.0-alpha\.3"/);
  assert.match(workspace, /version = "keep-this-unrelated-version"/);
  assert.deepEqual(localLockVersions(directory), names.map(name => ({ name, version: newVersion })));
  assert(readFileSync(join(directory, 'Cargo.lock'), 'utf8').endsWith(registryBlock));
  for (const member of members) assert.deepEqual(snapshot(directory)[member + '/Cargo.toml'], before[member + '/Cargo.toml']);
});

test('matching implicit, explicit and v-tag checks preserve every original byte and timestamp', t => {
  const { directory } = fixture(t, newVersion);
  const before = snapshot(directory);
  for (const args of [['--check'], ['--check', newVersion], ['--check', 'v' + newVersion]]) {
    const result = run(directory, args);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(snapshot(directory), before);
  }
});

test('a wrong tag is a read-only check failure', t => {
  const { directory } = fixture(t, newVersion);
  unchangedFailure(directory, ['--check', 'v0.2.0-alpha.4'], /Release versions differ/);
});

test('check rejects each independently mismatched target without repairing it', t => {
  for (const target of targets) {
    const { directory, write } = fixture(t, newVersion);
    write(target, readFileSync(join(directory, target), 'utf8').replaceAll(newVersion, oldVersion));
    unchangedFailure(directory, ['--check', newVersion], /Release versions differ/);
  }
});

test('invalid Tauri JSON cannot leave an already-parsed package partially upgraded', t => {
  const { directory, write } = fixture(t);
  write('src-tauri/tauri.conf.json', '{"version":');
  unchangedFailure(directory, [newVersion], /SyntaxError|JSON/);
});

test('missing workspace package version fails before any target write', t => {
  const { directory, write } = fixture(t);
  write('Cargo.toml', readFileSync(join(directory, 'Cargo.toml'), 'utf8').replace(`version = "${oldVersion}"\n`, ''));
  unchangedFailure(directory, [newVersion], /Expected exactly one release version in Cargo.toml workspace.package/);
});

test('workspace version trailing garbage is rejected before any target write', t => {
  const { directory, write } = fixture(t);
  write('Cargo.toml', readFileSync(join(directory, 'Cargo.toml'), 'utf8')
    .replace(`version = "${oldVersion}"`, `version = "${oldVersion}" invalid-trailing-token`));
  unchangedFailure(directory, [newVersion], /release version|version field|Cargo.toml/);
});

test('local lock version trailing garbage is rejected before any target write', t => {
  const { directory, write } = fixture(t);
  write('Cargo.lock', readFileSync(join(directory, 'Cargo.lock'), 'utf8')
    .replace(`name = "dzc-core"\nversion = "${oldVersion}"`, `name = "dzc-core"\nversion = "${oldVersion}" invalid-trailing-token`));
  unchangedFailure(directory, [newVersion], /release version|version field|Cargo.lock/);
});

test('missing local lock version and missing local package fail before writes', t => {
  for (const mutation of ['version', 'package']) {
    const { directory, write } = fixture(t);
    const entry = `[[package]]\nname = "dzc-protocol"\nversion = "${oldVersion}"\n`;
    const lock = readFileSync(join(directory, 'Cargo.lock'), 'utf8');
    write('Cargo.lock', lock.replace(entry, mutation === 'version' ? '[[package]]\nname = "dzc-protocol"\n' : ''));
    unchangedFailure(directory, [newVersion], /release version in Cargo.lock dzc-protocol|Local workspace package missing/);
  }
});

test('missing target file fails without creating or changing any remaining target', t => {
  const { directory } = fixture(t);
  rmSync(join(directory, 'Cargo.lock'));
  unchangedFailure(directory, [newVersion], /Missing release-version target/);
});

test('noncanonical SemVer and malformed prefixes are rejected without writes', t => {
  const { directory } = fixture(t);
  for (const version of ['01.2.3', '1.02.3', '1.2.03', '1.2.3-alpha.01', '1.2.3-', '1.2.3+bad..part',
    '1.2.3-alpha.1\n', '1.2.3-alpha_1', 'vv0.2.0-alpha.3', 'not-a-version']) {
    unchangedFailure(directory, [version], /valid SemVer/);
  }
});

test('valid prerelease metadata is preserved in all version targets', t => {
  const { directory } = fixture(t);
  const version = '1.2.3-rc.1+build.007';
  const result = run(directory, [version]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(readFileSync(join(directory, 'package.json'), 'utf8')).version, version);
  assert.deepEqual(localLockVersions(directory), names.map(name => ({ name, version })));
  assert.equal(run(directory, ['--check', version]).status, 0);
});

test('missing write version and extra arguments fail without changing targets', t => {
  const { directory } = fixture(t);
  for (const args of [[], [newVersion, 'extra'], ['--check', newVersion, 'extra']]) {
    unchangedFailure(directory, args, /Usage:/);
  }
});

test('same-version updates and repeated synchronization are byte- and timestamp-idempotent', t => {
  const current = fixture(t, newVersion);
  const original = snapshot(current.directory);
  assert.equal(run(current.directory, [newVersion]).status, 0);
  assert.deepEqual(snapshot(current.directory), original, 'matching custom JSON/TOML formatting must remain intact');
  const upgrading = fixture(t);
  assert.equal(run(upgrading.directory, [newVersion]).status, 0);
  const upgraded = snapshot(upgrading.directory);
  assert.equal(run(upgrading.directory, ['v' + newVersion]).status, 0);
  assert.deepEqual(snapshot(upgrading.directory), upgraded);
});
