import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MYGO_PATCH_ID, prepareMyGoPatch } from './mygo-surface-patch.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const identityPackage = 'github.com/qiyuanhuakai/dezirclip/frontends/dzc-surface/internal/buildinfo';
const targets = {
  'x86_64-pc-windows-msvc': { GOOS: 'windows', GOARCH: 'amd64' },
  'aarch64-pc-windows-msvc': { GOOS: 'windows', GOARCH: 'arm64' },
  'x86_64-unknown-linux-gnu': { GOOS: 'linux', GOARCH: 'amd64' },
  'aarch64-unknown-linux-gnu': { GOOS: 'linux', GOARCH: 'arm64' },
};
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function verifySurfaceIdentity(info, expected) {
  for (const field of ['component_version', 'source_revision', 'source_state', 'build_id', 'target', 'toolkit_patch_id']) {
    if (info[field] !== expected[field]) throw new Error(`Surface identity differs: ${field}`);
  }
  if (info.component !== 'surface' || info.mygo_version !== 'v0.6.4' || info.toolkit_patch_id !== MYGO_PATCH_ID
    || info.protocol_version?.major !== 1 || info.protocol_version?.minor !== 0) throw new Error('Surface runtime identity is incompatible');
}

export function buildSurface(args = process.argv.slice(2), env = process.env) {
  const run = (command, argv, options = {}) => execFileSync(command, argv, { cwd: root, encoding: 'utf8', ...options });
  const host = run('rustc', ['--print', 'host-tuple']).trim();
  let target = env.TAURI_ENV_TARGET_TRIPLE || host;
  let destination;
  for (let index = 0; index < args.length; index++) {
    if (args[index] === '--target' && args[index + 1]) target = args[++index];
    else if (args[index] === '--out-dir' && args[index + 1]) destination = resolve(root, args[++index]);
    else throw new Error(`Unknown Surface build option: ${args[index]}`);
  }
  if (!Object.hasOwn(targets, target)) throw new Error('Surface supports Windows and Linux x64/ARM64 targets');
  const go = env.DZC_GO || 'go';
  const goVersion = run(go, ['version']).trim();
  if (!/^go version go1\.27\.3 (windows|linux)\/(amd64|arm64)$/.test(goVersion)) throw new Error('Surface requires the pinned Go 1.27.3 toolchain');
  run(process.execPath, ['scripts/set-release-version.mjs', '--check'], { stdio: 'inherit' });
  run(process.execPath, ['scripts/generate-surface-tokens.mjs', '--check']);
  const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
  const revision = run('git', ['rev-parse', 'HEAD']).trim();
  if (!/^[a-f0-9]{40}$/.test(revision) || (env.DZC_BUILD_REVISION && env.DZC_BUILD_REVISION.toLowerCase() !== revision)) throw new Error('Surface build revision must match the checkout');
  const sourceStatus = () => run('git', ['status', '--porcelain=v1', '--untracked-files=normal']).trim();
  const initialStatus = sourceStatus();
  const sourceState = initialStatus ? 'modified_checkout' : 'verified_checkout';
  const expected = { component_version: version, source_revision: revision, source_state: sourceState, build_id: `${version}@${revision}`, target, toolkit_patch_id: MYGO_PATCH_ID };
  const moduleRoot = join(root, 'frontends/dzc-surface');
  const module = readFileSync(join(moduleRoot, 'go.mod'), 'utf8');
  if (!/^go 1\.27\.3\s*$/m.test(module) || !/github\.com\/egoist\/mygo v0\.6\.4(?:\s|$)/m.test(module)
    || /replace\s+(?:\([^)]*)?github\.com\/egoist\/mygo/m.test(module)) throw new Error('Surface dependency pin differs');
  const fingerprint = () => {
    const paths = run('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--',
      'frontends/dzc-surface', 'clients/go', 'design', 'package.json', 'Cargo.toml', 'Cargo.lock', 'src-tauri/tauri.conf.json',
      'scripts/set-release-version.mjs', 'scripts/updater-release.mjs', 'scripts/build-surface.mjs', 'scripts/mygo-surface-patch.mjs', 'scripts/generate-surface-tokens.mjs'])
      .split('\0').filter(Boolean).sort();
    return paths.map(path => ({ path, sha256: sha256(readFileSync(join(root, path))) }));
  };
  const inputs = fingerprint();
  const outDir = destination || join(root, 'artifacts/dzc-surface', target);
  mkdirSync(outDir, { recursive: true });
  const executable = join(outDir, targets[target].GOOS === 'windows' ? 'dzc-surface.exe' : 'dzc-surface');
  const temporary = executable + `.building-${process.pid}${targets[target].GOOS === 'windows' ? '.exe' : ''}`;
  const goEnv = { ...env, ...targets[target], CGO_ENABLED: '0', GOTOOLCHAIN: 'local', GOWORK: 'off', GOFLAGS: '' };
  const patch = prepareMyGoPatch({ run, go, moduleRoot, outputRoot: outDir, env: goEnv });
  const values = { Version: version, SourceRevision: revision, SourceState: sourceState, BuildID: expected.build_id, Target: target, ToolkitPatch: MYGO_PATCH_ID };
  const ldflags = ['-s', '-w', ...(targets[target].GOOS === 'windows' ? ['-H=windowsgui'] : []),
    ...Object.entries(values).flatMap(([name, value]) => ['-X', `${identityPackage}.${name}=${value}`])].join(' ');
  const goArgs = ['build', '-mod=readonly', `-modfile=${patch.modfile}`, '-trimpath', '-buildvcs=false', '-ldflags', ldflags, '-o', temporary, './cmd/dzc-surface'];
  run(go, goArgs, { cwd: moduleRoot, env: goEnv, stdio: 'inherit' });
  patch.verifyUnchanged();
  if (JSON.stringify(fingerprint()) !== JSON.stringify(inputs) || sourceStatus() !== initialStatus
    || run('git', ['rev-parse', 'HEAD']).trim() !== revision) throw new Error('Surface inputs changed while building; unpublished output retained for diagnosis');
  let buildInfo = null;
  if (target === host) {
    buildInfo = JSON.parse(run(temporary, ['--build-info'], { timeout: 5000 }));
    verifySurfaceIdentity(buildInfo, expected);
  }
  renameSync(temporary, executable);
  const proof = { schema_version: 1, kind: 'surface-build', created_at: new Date().toISOString(),
    executable, sha256: sha256(readFileSync(executable)), expected, build_info: buildInfo,
    identity_verification: buildInfo ? 'executed_native_build_info' : 'cross_target_not_executed',
    toolchain: goVersion, command: [go, ...goArgs], inputs, toolkit_patch: patch.proof };
  writeFileSync(executable + '.proof.json', JSON.stringify(proof, null, 2) + '\n');
  console.log(`Built experimental Surface: ${executable}`);
  return proof;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) buildSurface();
