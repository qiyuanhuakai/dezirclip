import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

export const MYGO_PATCH_ID = 'mygo-v0.6.4-no-startup-cleanup-v1';
export const MYGO_APP_SHA256 = 'af53a7f7c0dbe36d42edc6de34f9f7735569ae53170715264af6618b99e281b9';
const modulePath = 'github.com/egoist/mygo';
const version = 'v0.6.4';
const moduleSum = 'h1:VzCqZKgvXcym5cSYKoC6IkTgcVeGZquPiui/a3oE36o=';
const goModSum = 'h1:dVav53PwF2xEotjf4oA5lTY2Jp2exgzyjLhbSXhbMwE=';
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function removeStartupCleanupCall(source) {
  const call = '\tgo cleanUpdateLeftovers()\n';
  if (source.split(call).length !== 2) throw new Error('Expected exactly one MyGo startup cleanup call');
  return source.replace(call, '');
}

export function patchVerifiedMyGoApp(source) {
  if (sha256(source) !== MYGO_APP_SHA256) throw new Error('MyGo app.go differs from the reviewed source');
  return Buffer.from(removeStartupCleanupCall(source.toString('utf8')), 'utf8');
}

function treeFiles(directory, prefix = '') {
  return readdirSync(join(directory, prefix), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, 'en')).flatMap(entry => {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) return treeFiles(directory, path);
    if (!entry.isFile()) throw new Error(`MyGo module contains a non-regular entry: ${path}`);
    return [{ path, sha256: sha256(readFileSync(join(directory, path))) }];
  });
}

export function prepareMyGoPatch({ run, go, moduleRoot, outputRoot, env }) {
  const goRun = argv => run(go, argv, { cwd: moduleRoot, env });
  const downloaded = JSON.parse(goRun(['mod', 'download', '-json', `${modulePath}@${version}`]));
  if (downloaded.Path !== modulePath || downloaded.Version !== version || downloaded.Sum !== moduleSum
    || downloaded.GoModSum !== goModSum || !downloaded.Dir || downloaded.Error) throw new Error('MyGo module checksums differ from the reviewed dependency');
  goRun(['mod', 'verify']);
  const originalApp = readFileSync(join(downloaded.Dir, 'app.go'));
  const patchedApp = patchVerifiedMyGoApp(originalApp);
  const originalFiles = treeFiles(downloaded.Dir);
  const patchRoot = mkdtempSync(join(outputRoot, '.mygo-build-'));
  const patchedModule = join(patchRoot, 'mygo');
  for (const { path } of originalFiles) {
    const destination = join(patchedModule, path);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(join(downloaded.Dir, path), destination);
    chmodSync(destination, 0o644);
  }
  if (JSON.stringify(treeFiles(patchedModule)) !== JSON.stringify(originalFiles)) throw new Error('MyGo source changed while creating the isolated copy');
  writeFileSync(join(patchedModule, 'app.go'), patchedApp);
  const patchedFiles = treeFiles(patchedModule);
  const expectedFiles = originalFiles.map(file => file.path === 'app.go' ? { ...file, sha256: sha256(patchedApp) } : file);
  if (JSON.stringify(patchedFiles) !== JSON.stringify(expectedFiles)) throw new Error('MyGo patch changed unexpected module files');
  const modfile = join(patchRoot, 'surface.mod');
  const sumfile = join(patchRoot, 'surface.sum');
  copyFileSync(join(moduleRoot, 'go.mod'), modfile);
  copyFileSync(join(moduleRoot, 'go.sum'), sumfile);
  goRun(['mod', 'edit', `-modfile=${modfile}`,
    `-replace=${modulePath}@${version}=${patchedModule}`,
    `-replace=github.com/qiyuanhuakai/dezirclip/clients/go=${resolve(moduleRoot, '../../clients/go')}`]);
  const modfileHash = sha256(readFileSync(modfile));
  const sumfileHash = sha256(readFileSync(sumfile));
  return {
    modfile,
    proof: {
      id: MYGO_PATCH_ID, strategy: 'isolated_module_copy', module: modulePath, version,
      source_revision: '6255ea40139d06e0762a7c4a32626148ba410478',
      module_sum: moduleSum, module_go_mod_sum: goModSum,
      changed_files: ['app.go'], upstream_app_sha256: MYGO_APP_SHA256, patched_app_sha256: sha256(patchedApp),
      upstream_tree_sha256: sha256(JSON.stringify(originalFiles)), patched_tree_sha256: sha256(JSON.stringify(patchedFiles)),
      file_count: patchedFiles.length,
    },
    verifyUnchanged() {
      if (JSON.stringify(treeFiles(patchedModule)) !== JSON.stringify(patchedFiles)
        || JSON.stringify(treeFiles(downloaded.Dir)) !== JSON.stringify(originalFiles)
        || sha256(readFileSync(modfile)) !== modfileHash || sha256(readFileSync(sumfile)) !== sumfileHash) {
        throw new Error('MyGo module inputs changed while building; unpublished output retained for diagnosis');
      }
    },
  };
}
