import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

export const MYGO_TOOLKIT_PATH = 'frontends/dzc-surface/internal/buildinfo/mygo-toolkit.json';
export const MYGO_TOOLKIT = JSON.parse(readFileSync(new URL('../' + MYGO_TOOLKIT_PATH, import.meta.url), 'utf8'));
export const MYGO_PATCH_ID = MYGO_TOOLKIT.id;
const { module: modulePath, version, module_sum: moduleSum, module_go_mod_sum: goModSum } = MYGO_TOOLKIT;
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function removeStartupCleanupCall(source) {
  const call = '\tgo cleanUpdateLeftovers()\n';
  if (source.split(call).length !== 2) throw new Error('Expected exactly one MyGo startup cleanup call');
  return source.replace(call, '');
}

export function patchVerifiedMyGoApp(source) {
  if (sha256(source) !== MYGO_TOOLKIT.upstream_app_sha256) throw new Error('MyGo app.go differs from the reviewed source');
  const patched = Buffer.from(removeStartupCleanupCall(source.toString('utf8')), 'utf8');
  if (sha256(patched) !== MYGO_TOOLKIT.patched_app_sha256) throw new Error('Patched MyGo app.go differs from the reviewed toolkit manifest');
  return patched;
}

function treeFiles(directory, prefix = '') {
  return readdirSync(join(directory, prefix), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, 'en')).flatMap(entry => {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) return treeFiles(directory, path);
    if (!entry.isFile()) throw new Error(`MyGo module contains a non-regular entry: ${path}`);
    return [{ path, sha256: sha256(readFileSync(join(directory, path))) }];
  });
}

function windowsPatches(source) {
  const entry = 'internal/windows/webview2.go';
  const options = 'internal/windows/webview2_options.go';
  const start = source(entry, 'f53a2724f24fa48c7bca7c6a0e5f102ef307e592e26a559d67431edbdf8e2919')
    .replace('var options uintptr\n\tif language != "" {\n\t\toptions = newEnvOptions(language)',
      `arguments := ""
	if len(os.Args) == 4 && os.Args[1] == "--main-window" && os.Args[3] == "--software-webview" {
		arguments = "--disable-gpu --disable-gpu-compositing --disable-software-rasterizer --disable-features=VizDisplayCompositor"
	}
	var options uintptr
	if language != "" || arguments != "" {
		options = newEnvOptions(language, arguments)`);
  const getters = source(options, '2c536f8b6ce683e80350091a5fd1568c92aab9e74279e1df45197b40e6927e8b')
    .replace('language []uint16 //', 'arguments []uint16\n\tlanguage []uint16 //')
    .replace('func newEnvOptions(language string)', 'func newEnvOptions(language, arguments string)')
    .replace('str(func(*envOptions) []uint16 { return nil }), // get_AdditionalBrowserArguments',
      'str(func(o *envOptions) []uint16 { return o.arguments }), // get_AdditionalBrowserArguments')
    .replace('if language != "" {', 'if arguments != "" {\n\t\to.arguments = utf16z(arguments)\n\t}\n\tif language != "" {');
  // Backport egoist/mygo#660 for transparent native windows as well as flyouts.
  const windowFile = 'internal/windows/window.go';
  const upstream = source(windowFile, '446aa0b7e52a40557f7f6e5f0899cc1c2f4dbe84f354dab911ab642c39d328d1');
  const marker = 'if o.Flyout != nil {\n\t\t// What the content leaves transparent shows what is behind.';
  if (upstream.split(marker).length !== 2) throw new Error('Expected the reviewed MyGo native composition condition');
  const window = upstream.replace(marker, marker.replace('o.Flyout != nil', 'o.Flyout != nil || o.Transparent && o.Surface'))
    .replace('func (w *window) Show() {', 'func (w *window) Show() {\n\tw.memoryTarget(0)')
    .replace('func (w *window) ShowInactive() {', 'func (w *window) ShowInactive() {\n\tw.memoryTarget(0)')
    .replace('func (w *window) Hide() { procShowWindow.Call(w.hwnd, swHide) }', `func (w *window) Hide() { procShowWindow.Call(w.hwnd, swHide); w.memoryTarget(1) }

// ICoreWebView2_19 extends _18 with get/put MemoryUsageTargetLevel at slots 119/120.
// The controller owns its process group; unrelated WebViews are never enumerated.
func (w *window) memoryTarget(level uintptr) {
    if w.webview == 0 { return }
    iid := guid("6921f954-79b0-437f-a997-c85811897c68")
    core := queryInterface(w.webview, &iid)
    if core != 0 { comCall(core, 120, level); release(core) }
}`);
  // A non-activating flyout must receive pointer events without blurring its parent WebView.
  const surfaceFile = 'internal/windows/surface.go';
  const surface = source(surfaceFile, '41dd1c1eacd1a8f6c812725ec2d4eb96f3bf6634dcc2b743a1459c38b1e55c2f')
    .replace('\t\tprocSetFocus.Call(hwnd)\n\t\tif s.buttons == 0 {',
      '\t\tif f := s.w.opts.Flyout; f == nil || f.Focusable {\n\t\t\tprocSetFocus.Call(hwnd)\n\t\t}\n\t\tif s.buttons == 0 {');
  return [[entry, Buffer.from(start)], [options, Buffer.from(getters)], [windowFile, Buffer.from(window)], [surfaceFile, Buffer.from(surface)]];
}

function linuxPatches(source) {
  const surfaceFile = 'internal/linux/surface.go';
  const surface = source(surfaceFile, '9bcaf2a2c49cd61b0b0da873cbc841428bad4102f98423395d8ac0d072095911')
    .replace('cairoPaint                  func(cr ptr)', `cairoPaint                  func(cr ptr)
	cairoGetGroupTarget        func(cr ptr) ptr
	cairoSurfaceGetType        func(s ptr) int32
	cairoSurfaceFlush          func(s ptr)`)
    .replace('mustBind(c, &cairoPaint, "cairo_paint")', `mustBind(c, &cairoPaint, "cairo_paint")
		mustBind(c, &cairoGetGroupTarget, "cairo_get_group_target")
		mustBind(c, &cairoSurfaceGetType, "cairo_surface_get_type")
		mustBind(c, &cairoSurfaceFlush, "cairo_surface_flush")`)
    .replace('cairoPaint(s.cr)\n\tcairoSurfaceDestroy(img)', `cairoPaint(s.cr)
	// GTK copies this back buffer to X11 after the draw callback returns.
	// Flush any deferred Cairo writes before GTK reads the drawable.
	const cairoSurfaceTypeXlib = 3
	target := cairoGetGroupTarget(s.cr)
	if cairoSurfaceGetType(target) == cairoSurfaceTypeXlib {
		cairoSurfaceFlush(target)
	}
	cairoSurfaceDestroy(img)`);
  return [[surfaceFile, Buffer.from(surface)]];
}

function localDragPatch(source) {
  const file = 'ui/input.go';
  let input = source(file, 'e8cec7096e780d83b34abdddc2ba9e70e0af7ec34f28150b4bfe882bcd36c1aa');
  const replace = (before, after) => {
    if (input.split(before).length !== 2) throw new Error('Expected one MyGo local drag input branch');
    input = input.replace(before, after);
  };
  // A virtual list can discard the pressed widget while retaining the local drag.
  replace('\t\trt.dragMove(x, y)\n\t}\n\tmoved :=',
    '\t\trt.dragMove(x, y)\n\t} else if d := rt.drag; d != nil && !d.native && !d.canceled {\n\t\td.over = rt.valueTarget(x, y, d.value)\n\t\trt.requestFrame()\n\t}\n\tmoved :=');
  replace('\tif s := rt.pressed; s != nil && button == rt.pressButton && rt.dragEnd() {\n\t\t// A drag ended, dropping its value: no click.\n\t\trt.pressed = nil\n\t\ts.pressed = false',
    '\tif button == rt.pressButton && rt.dragEnd() {\n\t\t// A drag ended, dropping its value: no click.\n\t\tif s := rt.pressed; s != nil {\n\t\t\ts.pressed = false\n\t\t}\n\t\trt.pressed = nil');
  return [file, Buffer.from(input)];
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
  function verifiedSource(path, digest) {
    const bytes = readFileSync(join(patchedModule, path));
    if (sha256(bytes) !== digest) throw new Error(`Unreviewed MyGo source: ${path}`);
    return bytes.toString('utf8');
  }
  const changes = new Map([['app.go', patchedApp], ...windowsPatches(verifiedSource), ...linuxPatches(verifiedSource),
    localDragPatch(verifiedSource)]);
  if (JSON.stringify([...changes.keys()]) !== JSON.stringify(MYGO_TOOLKIT.changed_files)) {
    throw new Error('MyGo patch file list differs from the reviewed toolkit manifest');
  }
  for (const [path, bytes] of changes) writeFileSync(join(patchedModule, path), bytes);
  const patchedFiles = treeFiles(patchedModule);
  const expectedFiles = originalFiles.map(file => changes.has(file.path) ? { ...file, sha256: sha256(changes.get(file.path)) } : file);
  if (JSON.stringify(patchedFiles) !== JSON.stringify(expectedFiles)) throw new Error('MyGo patch changed unexpected module files');
  const modfile = join(patchRoot, 'surface.mod');
  const sumfile = join(patchRoot, 'surface.sum');
  copyFileSync(join(moduleRoot, 'go.mod'), modfile);
  copyFileSync(join(moduleRoot, 'go.sum'), sumfile);
  goRun(['mod', 'edit', `-modfile=${modfile}`,
    `-replace=${modulePath}@${version}=${patchedModule}`,
    `-replace=github.com/qiyuanhuakai/dezirclip/clients/go=${resolve(moduleRoot, '../../clients/go')}`,
    `-replace=github.com/qiyuanhuakai/dezirclip/packages/ui-native=${resolve(moduleRoot, '../../packages/ui-native')}`,
    `-replace=github.com/qiyuanhuakai/dezirclip/packages/ui-presets=${resolve(moduleRoot, '../../packages/ui-presets')}`]);
  const modfileHash = sha256(readFileSync(modfile));
  const sumfileHash = sha256(readFileSync(sumfile));
  return {
    modfile,
    proof: {
      ...MYGO_TOOLKIT,
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
