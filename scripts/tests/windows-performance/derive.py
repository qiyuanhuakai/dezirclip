"""Generate the bounded comparison fixture from reviewed functional sources; no native execution."""
import argparse
import ast
import difflib
import hashlib
import json
from pathlib import Path
import re

REPOSITORY = Path(__file__).resolve().parents[3]
BASE_COMMIT = 'c5083f9849d9543f81d71f0b57ce49522a51e9e4'


def derive_sources(controller, original_target, crash_events):
    # Compare canonical LF text so Git's Windows checkout conversion is harmless.
    controller = controller.replace('\r\n', '\n')
    original_target = original_target.replace('\r\n', '\n')
    crash_events = crash_events.replace('\r\n', '\n')
    assert hashlib.sha256(controller.encode('utf-8')).hexdigest() == 'c1950a6bff4cf51f91e9d5f5cc4d8bf554d6c9c5361c6b733c676f6374e812cf', 'Functional controller changed; review the derivation before use'
    assert hashlib.sha256(crash_events.encode('utf-8')).hexdigest() == 'b8fd4ea818c09963e8e0d9f746ffd2a2e7ca8b2e90993cf931989b7b09d61400', 'Functional crash diagnostics changed; review the derivation before use'
    original_plumbing = controller.split('export async function runSurfaceAcceptance(', 1)[0]
    plumbing = original_plumbing.replace("from '../build-surface.mjs'", "from '../../scripts/build-surface.mjs'")
    plumbing = plumbing.replace("from '../release-windows-upgrade.mjs'", "from '../../scripts/release-windows-upgrade.mjs'")
    plumbing = plumbing.replace("new URL('./native-windows-surface-target.py', import.meta.url)",
                                "new URL('./native-target.py', import.meta.url)")
    plumbing += '\nexport { repository, sha, json, delay, normalized, regular, owned, save, copy, canarySnapshot, environment, LoggedProcess, NativeChannel, Bridge, completedCommand, availablePort, fixedSurfaceInputs, toolkitPatchId };\n'
    assert hashlib.sha256(original_target.encode('utf-8')).hexdigest() == 'cf3ad99a1c21a1267dd3011567a9a9bc434baa8d7214f7e1c8c730bdc30c5780', 'Functional target changed; review the derivation before use'
    target = original_target


    def replace_once(old, new):
        nonlocal target
        assert target.count(old) == 1, old
        target = target.replace(old, new)


    replace_once("sys.path.insert(0, str(Path(__file__).resolve().parents[1]))",
                 "sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))")
    replace_once('deadline = time.monotonic() + 300', 'deadline = time.monotonic() + 1080')
    replace_once("'paste': [0x11, 0x56]}", "'paste': [0x11, 0x56], 'backspace': [0x08]}")
    replace_once("'memory_scope': 'All observed Desktop descendants; working set/private commit are not PSS or USS.'}",
                 "'memory_scope': 'All observed Desktop descendants; working set/private commit are not PSS or USS.',\n                'observed_monotonic_ns': str(time.monotonic_ns())}")
    replace_once("        else:\n            raise RuntimeError('Only explicit target/Surface input is permitted')", """        elif role == 'tauri':
            require(foreground.get('pid') == self.desktop and foreground.get('hwnd') == request.get('panel_hwnd')
                    and foreground.get('title') == '' and foreground.get('class_name') == 'Tauri Window'
                    and foreground.get('visible') and foreground.get('rect', {}).get('width') == 480
                    and foreground.get('rect', {}).get('height') == 360,
                    'Input requires the exact observed foreground 480x360 owned Tauri panel')
            require(self.process(self.desktop)['running'], 'Original Desktop has exited')
        else:
            raise RuntimeError('Only explicit target/Surface/Tauri panel input is permitted')""")
    replace_once('        sent = self.user.SendInput(len(inputs), array, ctypes.sizeof(self.Input))',
                 '        input_monotonic_ns = time.monotonic_ns()\n        sent = self.user.SendInput(len(inputs), array, ctypes.sizeof(self.Input))')
    replace_once("return {'sent': sent, 'foreground_before': foreground, 'native_input': True, 'target_focus': self.focus_snapshot()}",
                 "return {'sent': sent, 'foreground_before': foreground, 'native_input': True, 'target_focus': self.focus_snapshot(),\n                'input_monotonic_ns': str(input_monotonic_ns)}")
    replace_once("elif operation == 'target-text': value = {'text': native.target_text(), 'target_focus': native.focus_snapshot()}",
                 "elif operation == 'target-text': value = {'text': native.target_text(), 'target_focus': native.focus_snapshot(), 'observed_monotonic_ns': str(time.monotonic_ns())}")
    replace_once("elif operation == 'observe': value = native.observe()",
                 "elif operation == 'observe': value = native.observe()\n                elif operation == 'observe-processes': value = native.resource_observation(request)")
    replace_once("elif operation == 'accessibility': value = native.accessibility(request.get('pid'))",
                 "elif operation == 'accessibility': value = native.accessibility(request.get('pid'), request.get('panel_hwnd'))")
    replace_once("""    def accessibility(self, pid):
        observation = self.observe()
        window = next(w for w in observation['windows'] if w['pid'] == pid and w['visible'])
        require(any(p['pid'] == pid and Path(p['image']).resolve() == (self.root / 'candidate/dzc-surface.exe').resolve() for p in observation['processes']), 'UIA is limited to the actual owned Surface')
""", """    def accessibility(self, pid, panel_hwnd=None):
        observation = self.observe()
        if pid == self.desktop:
            windows = [w for w in observation['windows'] if w['pid'] == self.desktop and w['visible'] and w['class_name'] == 'Tauri Window']
            require(isinstance(panel_hwnd, int) and len(windows) == 1, 'Explicit unique original Tauri panel required for UIA')
            window = windows[0]
            require(window['hwnd'] == panel_hwnd and window['title'] == ''
                    and window['rect']['width'] == 480 and window['rect']['height'] == 360
                    and observation['foreground'].get('hwnd') == panel_hwnd
                    and observation['foreground'].get('pid') == self.desktop
                    and self.process(self.desktop)['running'],
                    'UIA requires the exact original foreground 480x360 Tauri panel')
        else:
            window = next(w for w in observation['windows'] if w['pid'] == pid and w['visible'])
            require(any(p['pid'] == pid and Path(p['image']).resolve() == (self.root / 'candidate/dzc-surface.exe').resolve() for p in observation['processes']), 'UIA is limited to the actual owned Surface')
            require(panel_hwnd == window['hwnd'] and observation['foreground'].get('hwnd') == panel_hwnd,
                    'UIA requires the original foreground Surface panel')
""")
    replace_once('    def activate(self, alternate=False):', """    def resource_observation(self, request):
        processes = self.tree()
        hwnd, pid = request.get('panel_hwnd'), request.get('panel_pid')
        require(isinstance(hwnd, int) and hwnd > 0 and isinstance(pid, int) and pid > 0,
                'Explicit owned panel identity required')
        require(any(item['pid'] == pid and item['running'] for item in processes),
                'The original measured panel process has exited')
        owner = W.DWORD()
        require(self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner)) and owner.value == pid,
                'Measured panel no longer belongs to its original process')
        main = request.get('main_hwnd')
        require(isinstance(main, int) and main > 0
                and self.user.GetWindowThreadProcessId(main, ctypes.byref(owner)) and owner.value == self.desktop,
                'The measured retained main window must still belong to the original Desktop')
        foreground = self.user.GetForegroundWindow()
        return {'processes': processes, 'observed_monotonic_ns': str(time.monotonic_ns()),
                'panel': {'hwnd': hwnd, 'pid': pid, 'visible': bool(self.user.IsWindowVisible(hwnd)),
                          'foreground': foreground == hwnd},
                'main': {'hwnd': main, 'visible': bool(self.user.IsWindowVisible(main))},
                'foreground_target': foreground == self.target}

    def activate(self, alternate=False):""")

    files = {'native-windows-crash-events.mjs': crash_events}
    for filename, before, after in [('native-plumbing.mjs', original_plumbing, plumbing),
                                     ('native-target.py', original_target, target)]:
        files[filename] = after
        files[filename + '.patch'] = ''.join(difflib.unified_diff(
            before.splitlines(keepends=True), after.splitlines(keepends=True),
            fromfile='functional-base/' + filename, tofile='comparison/' + filename))
    ast.parse(target, filename='derived/native-target.py')
    return files


def validate_destination(repository, destination):
    repository = repository.resolve(strict=True)
    destination = Path(destination).absolute()
    boundary = repository / '.harness'
    is_link = lambda path: path.is_symlink() or getattr(path, 'is_junction', lambda: False)()
    if (destination.parent != boundary or not re.fullmatch(r'windows-performance-[a-z0-9-]+', destination.name)
            or destination.exists() or is_link(destination) or is_link(boundary) or boundary.resolve() != boundary):
        raise ValueError('A fresh windows-performance-* directory directly inside the repository .harness is required')
    return destination


def generate(repository, destination):
    destination = validate_destination(repository, destination)
    controller = (repository / 'scripts/tests/native-windows-surface.mjs').read_text(encoding='utf-8')
    target = (repository / 'scripts/tests/native-windows-surface-target.py').read_text(encoding='utf-8')
    crash_events = (repository / 'scripts/tests/native-windows-crash-events.mjs').read_text(encoding='utf-8')
    files = derive_sources(controller, target, crash_events)
    source_directory = repository / 'scripts/tests/windows-performance'
    for name in ('controller.mjs', 'process-sampler.mjs', 'pure.node.mjs'):
        files[name] = (source_directory / name).read_text(encoding='utf-8')
    destination.parent.mkdir(exist_ok=True)
    destination.mkdir()
    for name, source in files.items():
        with (destination / name).open('x', encoding='utf-8', newline='\n') as output:
            output.write(source)
    proof = {'kind': 'windows_performance_source_derivation_v1', 'functional_base_commit': BASE_COMMIT,
             'native_runtime_executed_by_generator': False,
             'base_lf_sha256': {name: hashlib.sha256(source.encode('utf-8')).hexdigest() for name, source in
                               [('native-windows-surface.mjs', controller), ('native-windows-surface-target.py', target),
                                ('native-windows-crash-events.mjs', crash_events)]},
             'generated_sha256': {name: hashlib.sha256((destination / name).read_bytes()).hexdigest() for name in files}}
    with (destination / 'derivation.json').open('x', encoding='utf-8', newline='\n') as output:
        output.write(json.dumps(proof, indent=2) + '\n')
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', required=True)
    args = parser.parse_args()
    print(generate(REPOSITORY, Path(args.output_dir)))


if __name__ == '__main__':
    main()
