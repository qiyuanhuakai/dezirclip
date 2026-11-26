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

function replacePatch(source, before, after) {
  if (source.split(before).length !== 2) throw new Error('Expected one reviewed MyGo patch site');
  return source.replace(before, after);
}

function windowsPatches(source) {
  // Clicks on a non-activating native flyout leave focus with its owner.
  const file = 'internal/windows/surface.go';
  let surface = replacePatch(source(file, '6b9cdb85684b730a94081b632ad82d0affab38310aa2da91e2ca0d9b3d14cf95'),
    '\t\tprocSetFocus.Call(hwnd)\n\t\tif s.buttons == 0 {',
    '\t\tif f := s.w.opts.Flyout; f == nil || f.Focusable {\n\t\t\tprocSetFocus.Call(hwnd)\n\t\t}\n\t\tif s.buttons == 0 {');
  surface = replacePatch(surface,
    '\tcase wmSetCursor:\n\t\tif loword(lp) == htClient {',
    `\tcase wmSetCursor:
		if s.w.dragCursor != 0 {
			procSetCursor.Call(s.w.dragCursor)
			return 1, true
		}
		if loword(lp) == htClient {`);
  surface = replacePatch(surface, '\ts.cursor = c\n\tvar pt point', `\ts.cursor = c
	if s.w.dragCursor != 0 {
		return
	}
	var pt point`);

  const windowFile = 'internal/windows/window.go';
  let window = source(windowFile, '3684e7e8cab7878d4bf24c88c7b5534ee7a3a3c082ca2503fce1608ec19a6452');
  window = replacePatch(window, '\tparent *window\n', '\tparent *window\n\tdragCursor uintptr\n');
  window = replacePatch(window, '\tcase wmMove:\n', `\tcase 0x0216: // WM_MOVING
		if w.dragCursor != 0 {
			procSetCursor.Call(w.dragCursor)
		}
	case wmSetCursor:
		if w.dragCursor != 0 {
			procSetCursor.Call(w.dragCursor)
			return 1, true
		}
	case wmMove:
`);
  window = replacePatch(window, `func (w *window) StartDrag() {
	procReleaseCapture.Call()
	procSendMessageW.Call(w.hwnd, wmNCLButtonDown, htCaption, 0)
}`, `func (w *window) StartDrag() {
	procReleaseCapture.Call()
	w.dragCursor = cursorHandle(platform.CursorMove)
	if w.dragCursor != 0 {
		previous, _, _ := procSetCursor.Call(w.dragCursor)
		defer func() {
			w.dragCursor = 0
			procSetCursor.Call(previous)
		}()
	}
	// Keep the native move loop for snapping, release and Escape.
	var pos point
	procGetCursorPos.Call(uintptr(unsafe.Pointer(&pos)))
	screenPoint := uintptr(uint16(pos.X)) | uintptr(uint16(pos.Y))<<16
	procSendMessageW.Call(w.hwnd, wmNCLButtonDown, htCaption, screenPoint)
}`);
  return [[file, Buffer.from(surface)], [windowFile, Buffer.from(window)]];
}

function linuxPatches(source) {
  const surfaceFile = 'internal/linux/surface.go';
  const surface = source(surfaceFile, 'c237ca2fa47ae19e19d77b69ad8f5ccdbfa2d3202b9f94e7cef874cc3c244481')
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
  const windowFile = 'internal/linux/window.go';
  let window = replacePatch(source(windowFile, '6daca09642185068201a1220b1efe08a93a39fd337fa1cc88cb8ced5afacd1fc'),
    `func (w *window) StartDrag() {
	if !w.press.hasPressed {
		return
	}
	if !w.undecorated() {`, `func (w *window) StartDrag() {
	if !w.press.hasPressed {
		return
	}
	// A native X11 title bar with maximization disabled can hand off
	// the press right away, without waiting for client-side motion.
	direct := w.b.onX11 && w.surface != nil && !w.opts.Maximizable && !w.IsMaximized()
	if direct {
		// A user move takes precedence over any pending placement request.
		w.requested, w.placing = nil, false
		w.dragPending = false
	}
	if !w.undecorated() || direct {`);
  window = replacePatch(window, `func (w *window) TitleBarDoubleClicked() {
	if w.IsMaximized() {
		w.Unmaximize()
	} else {
		w.Maximize()
	}
}`, `func (w *window) TitleBarDoubleClicked() {
	if w.IsMaximized() {
		w.Unmaximize()
	} else if !w.b.onX11 || w.surface == nil || w.opts.Maximizable {
		w.Maximize()
	}
}`);
  return [[surfaceFile, Buffer.from(surface)], [windowFile, Buffer.from(window)]];
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
  const changes = new Map([['app.go', patchedApp], ...windowsPatches(verifiedSource), ...linuxPatches(verifiedSource)]);
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
    `-replace=github.com/qiyuanhuakai/dezirclip/clients/go=${resolve(moduleRoot, '../../clients/go')}`]);
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
