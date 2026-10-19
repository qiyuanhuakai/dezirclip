import base64
import argparse
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import time
import tempfile
from urllib.request import Request, urlopen
from urllib.error import HTTPError

SCRIPT = Path(__file__).resolve()
MARKER = 'Synthetic clipboard acceptance; no production key or user profile.\n'
OLD_SHA256 = 'da4abe1af820226ccf0cb8c40c8d7512b5ae3d7de89babcd1acecfd1ece9f0f0'
NAMESPACES = ('user', 'pid', 'net')


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def configure_headless_fixture(database):
    if not database.is_file():
        raise RuntimeError('The old CLI must create the synthetic database first.')
    requested = {'app.silent_start': 'false', 'app.disable_webview_gpu': 'true'}
    with closing(sqlite3.connect(database)) as connection, connection:
        before = {}
        for key in requested:
            row = connection.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
            before[key] = row[0] if row else None
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', requested.items())
        after = dict(connection.execute('SELECT key,value FROM settings WHERE key IN (?,?)', tuple(requested)))
    return {'before': before, 'after': after,
            'scope': 'Existing user settings selected only in a synthetic headless Xvfb profile; product defaults are unchanged.'}


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def namespaces():
    return {name: os.readlink(f'/proc/self/ns/{name}') for name in NAMESPACES}


def validate_fixture(root):
    require(root.parent == Path('/tmp').resolve() and root.name.startswith('dezirclip-native-acceptance-'),
            'Only a disposable /tmp/dezirclip-native-acceptance-* fixture is allowed.')
    marker = root / '.native-updater-fixture'
    require(not marker.is_symlink() and marker.read_text() == MARKER, 'Fixture marker is missing or invalid.')
    require(not (root / 'scenario').is_symlink(), 'Fixture scenario must not be a symlink.')


if sys.platform != 'linux':
    raise SystemExit('Native AppImage acceptance requires Linux with bubblewrap and Xvfb.')
INSIDE = os.environ.get('DEZIRCLIP_ACCEPTANCE_INSIDE') == '1'
if INSIDE:
    ROOT = Path(os.environ['DEZIRCLIP_ACCEPTANCE_ROOT']).resolve(strict=True)
    validate_fixture(ROOT)
    HOST_NAMESPACES = json.loads(os.environ['DEZIRCLIP_ACCEPTANCE_HOST_NAMESPACES'])
    require(all(namespaces()[name] != HOST_NAMESPACES[name] for name in NAMESPACES),
            'The fixture requires private user, PID and network namespaces.')
else:
    parser = argparse.ArgumentParser(description='Verify copied AppImages in isolated user, PID, network and desktop sessions.')
    parser.add_argument('--old-appimage', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--public-key', type=Path, required=True)
    parser.add_argument('--helper', type=Path, required=True)
    parser.add_argument('--webkit-driver', type=Path, required=True)
    parser.add_argument('--source-revision', help='Optional full source commit label; actual tool files are always hashed.')
    options = parser.parse_args()
    SOURCE = options.old_appimage.resolve(strict=True)
    CANDIDATE = options.candidate.resolve(strict=True)
    PUBLIC_KEY = options.public_key.resolve(strict=True)
    HELPER = options.helper.resolve(strict=True)
    DRIVER = options.webkit_driver.resolve(strict=True)
    require(digest(SOURCE) == OLD_SHA256, 'The old AppImage must match the pinned official 0.1.20 SHA256.')
    require(CANDIDATE.name == 'DezirClip_0.2.0-alpha.1_amd64.AppImage',
            'Only the explicit unpublished alpha.1 fixture is accepted.')
    if options.source_revision:
        require(re.fullmatch('[0-9a-fA-F]{40}', options.source_revision), 'Use a full source commit label.')
    decoded_key = base64.b64decode(PUBLIC_KEY.read_text().strip(), validate=True).splitlines()
    require(len(decoded_key) == 2 and decoded_key[0].startswith(b'untrusted comment: minisign public key:')
            and len(base64.b64decode(decoded_key[1], validate=True)) == 42,
            'Pass only a Tauri-encoded Minisign public test key.')
    ROOT = Path(tempfile.mkdtemp(prefix='dezirclip-native-acceptance-', dir='/tmp')).resolve(strict=True)
    (ROOT / '.native-updater-fixture').write_text(MARKER)
    HOST_NAMESPACES = namespaces()
    print(f'Acceptance evidence: {ROOT}', flush=True)
SCENARIO = ROOT / 'scenario'

def run(arguments, **kwargs):
    return subprocess.run([str(value) for value in arguments], check=True, **kwargs)

def sandbox(command, readonly=False, readonly_image=False):
    arguments = ['bwrap', '--unshare-user', '--unshare-pid', '--unshare-net', '--unshare-ipc', '--unshare-uts', '--die-with-parent', '--new-session',
        '--clearenv',
        '--ro-bind', '/usr', '/usr', '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/sbin', '/sbin',
        '--ro-bind', '/lib', '/lib', '--ro-bind', '/lib64', '/lib64', '--ro-bind', '/etc', '/etc',
        '--dev', '/dev', '--proc', '/proc', '--tmpfs', '/tmp', '--tmpfs', '/run', '--dir', '/run/fixture', '--chmod', '0700', '/run/fixture', '--dir', '/home/fixture',
        '--dir', '/fixture-bin', '--ro-bind', str(HELPER), '/fixture-bin/updater', '--ro-bind', str(SCRIPT), '/fixture-bin/acceptance.py',
        '--ro-bind', str(DRIVER), '/fixture-bin/WebKitWebDriver',
        '--ro-bind' if readonly else '--bind', str(SCENARIO), str(SCENARIO),
        '--setenv', 'HOME', '/home/fixture', '--setenv', 'XDG_CONFIG_HOME', str(SCENARIO / 'profile/config'),
        '--setenv', 'DEZIRCLIP_ACCEPTANCE_INSIDE', '1', '--setenv', 'DEZIRCLIP_ACCEPTANCE_ROOT', str(ROOT),
        '--setenv', 'DEZIRCLIP_ACCEPTANCE_HOST_NAMESPACES', json.dumps(HOST_NAMESPACES),
        '--setenv', 'XDG_DATA_HOME', str(SCENARIO / 'profile/data'), '--setenv', 'XDG_CACHE_HOME', '/home/fixture/.cache',
        '--setenv', 'XDG_RUNTIME_DIR', '/run/fixture', '--setenv', 'PATH', '/usr/bin:/bin:/fixture-bin',
        '--setenv', 'LANG', 'C.UTF-8', '--setenv', 'TZ', 'UTC', '--chdir', str(SCENARIO),
        '--unsetenv', 'WAYLAND_DISPLAY', '--unsetenv', 'DISPLAY', '--unsetenv', 'DBUS_SESSION_BUS_ADDRESS',
        '--setenv', 'WEBKIT_DISABLE_DMABUF_RENDERER', '1', '--setenv', 'WEBKIT_DISABLE_COMPOSITING_MODE', '1',
        '--setenv', 'APPIMAGE_EXTRACT_AND_RUN', '1', '--setenv', 'TAURI_WEBVIEW_AUTOMATION', 'true',
        '--setenv', 'DEZIRCLIP_DISABLE_LINUX_SERVICES', 'window_tracker,edge_docking,mouse_hotkey',
        '--setenv', 'TIEZ_DISABLE_LINUX_SERVICES', 'window_tracker,edge_docking,mouse_hotkey']
    arguments += ['--ro-bind', str(ROOT / '.native-updater-fixture'), str(ROOT / '.native-updater-fixture')]
    if readonly_image:
        arguments += ['--ro-bind', str(SCENARIO / 'current.AppImage'), str(SCENARIO / 'current.AppImage')]
    return run(arguments + command, capture_output=True, text=True, timeout=180)

def extract(label):
    directory = SCENARIO / (label + '-extract')
    directory.mkdir(exist_ok=True)
    with (SCENARIO / (label + '-extract.log')).open('w') as output:
        run([SCENARIO / 'current.AppImage', '--appimage-extract'], cwd=directory, stdout=output, stderr=subprocess.STDOUT)
    binaries = list(directory.rglob('dzc'))
    require(len(binaries) == 1 and binaries[0].is_file(), f'Expected one bundled dzc binary, found {binaries}.')
    return binaries[0]

def request(port, route, data=None, method=None):
    payload = json.dumps(data).encode() if data is not None else None
    req = Request(f'http://127.0.0.1:{port}/{route}', data=payload, method=method, headers={'Content-Type': 'application/json'})
    try:
        with urlopen(req, timeout=40) as response:
            value = json.load(response)
            if isinstance(value.get('value'), dict) and value['value'].get('error'):
                raise RuntimeError(value)
            return value.get('value', value)
    except HTTPError as error:
        raise RuntimeError(error.read().decode()) from error

def inside(mode):
    os.environ['DEZIRCLIP_DB_PATH'] = str(SCENARIO / 'profile/data/io.github.qiyuanhuakai.dezirclip/clipboard.db')
    if mode == 'old':
        cli = extract('old')
        version = subprocess.check_output([cli, '--version'], text=True)
        require('0.1.20' in version, f'Unexpected old CLI version: {version}')
        run([cli, 'add', 'nativeappimagefixturemarker'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        settings = configure_headless_fixture(Path(os.environ['DEZIRCLIP_DB_PATH']))
        (SCENARIO / 'headless-settings.json').write_text(json.dumps(settings, indent=2))
        print(json.dumps({'old_version': version.strip(), 'history_seeded': True}))
        return
    cli = extract(mode)
    version = subprocess.check_output([cli, '--version'], text=True)
    require('0.2.0-alpha.1' in version, f'Unexpected candidate CLI version: {version}')
    history = subprocess.check_output([cli, 'search', 'nativeappimagefixturemarker', '--metadata', '--json'], text=True)
    require('nativeappimagefixturemarker' in history, 'Candidate CLI did not preserve synthetic old history.')
    (SCENARIO / 'new-history.json').write_text(history)
    with closing(sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH'])) as connection:
        settings = dict(connection.execute('SELECT key,value FROM settings WHERE key IN (?,?)',
                                          ('app.silent_start', 'app.disable_webview_gpu')))
    (SCENARIO / (mode + '-headless-settings.json')).write_text(json.dumps(settings, indent=2))
    require(settings == {'app.silent_start': 'false', 'app.disable_webview_gpu': 'true'},
            'Candidate did not preserve the explicit synthetic headless settings.')
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    with (SCENARIO / 'webdriver.log').open('w') as output:
        driver = subprocess.Popen(['/fixture-bin/WebKitWebDriver', f'--port={port}', '--host=local'], stdout=output, stderr=subprocess.STDOUT)
        route = None
        try:
            driver_ready = False
            for _ in range(100):
                try:
                    request(port, 'status')
                    driver_ready = True
                    break
                except OSError:
                    require(driver.poll() is None, 'WebKitWebDriver exited before becoming ready.')
                    time.sleep(.1)
            require(driver_ready, 'WebKitWebDriver did not become ready.')
            session = request(port, 'session', {'capabilities': {'alwaysMatch': {
                'webkitgtk:browserOptions': {'binary': str(SCENARIO / 'current.AppImage')}
            }}})
            session_id = session['sessionId']
            route = f'session/{session_id}'
            for _ in range(100):
                ready = request(port, route + '/execute/sync', {'script': 'return typeof window.__TAURI_INTERNALS__?.invoke === "function" && document.body.innerText.length > 0', 'args': []})
                if ready:
                    break
                time.sleep(.1)
            diagnostic = request(port, route + '/execute/sync', {'script': 'return {url:location.href,title:document.title,ready:document.readyState,visibility:document.visibilityState,hidden:document.hidden,width:innerWidth,height:innerHeight,textContent:document.body?.textContent,bridge:typeof window.__TAURI_INTERNALS__?.invoke,bodyText:document.body?.innerText,html:document.documentElement?.outerHTML}', 'args': []})
            (SCENARIO / (mode + '-webview-diagnostic.json')).write_text(json.dumps(diagnostic, indent=2))
            if diagnostic['bridge'] == 'function':
                visible = request(port, route + '/execute/async', {'script': 'const done=arguments[arguments.length-1];window.__TAURI_INTERNALS__.invoke("plugin:window|is_visible",{label:"main"}).then(x=>done({visible:x})).catch(e=>done({error:String(e)}))', 'args': []})
                (SCENARIO / (mode + '-native-window.json')).write_text(json.dumps(visible, indent=2))
            if not ready:
                try:
                    picture = request(port, route + '/screenshot')
                    (SCENARIO / (mode + '-diagnostic.png')).write_bytes(base64.b64decode(picture))
                except (OSError, RuntimeError) as error:
                    (SCENARIO / (mode + '-screenshot-error.txt')).write_text(str(error))
            require(ready, 'Actual AppImage WebView did not become visible and ready.')
            support = request(port, route + '/execute/async', {'script': 'const done=arguments[arguments.length-1]; window.__TAURI_INTERNALS__.invoke("get_update_support").then(done).catch(e=>done({error:String(e)}));', 'args': []})
            expected = 'app_image_read_only' if mode == 'new-read-only' else 'app_image'
            require(support['installation'] == expected and support['canInstall'] is (mode != 'new-read-only'),
                    f'Unexpected runtime installation capability: {support}')
            gui_history = request(port, route + '/execute/async', {'script': 'const done=arguments[arguments.length-1]; window.__TAURI_INTERNALS__.invoke("get_clipboard_history", {limit:20, offset:0, contentType:null}).then(done).catch(e=>done({error:String(e)}));', 'args': []})
            require(isinstance(gui_history, list) and any(entry.get('content') == 'nativeappimagefixturemarker' for entry in gui_history),
                    'Actual GUI command did not return the synthetic old history.')
            for _ in range(50):
                rendered = request(port, route + '/execute/sync', {'script': 'return document.body.innerText.includes("nativeappimagefixturemarker")', 'args': []})
                if rendered:
                    break
                time.sleep(.1)
            screenshot = request(port, route + '/screenshot')
            (SCENARIO / (mode + '-appimage.png')).write_bytes(base64.b64decode(screenshot))
            require(rendered, 'Actual WebView did not render the preserved synthetic entry.')
            print(json.dumps({'new_version': version.strip(), 'history_preserved': True, 'gui_history_preserved': True, 'history_rendered': rendered, 'actual_webview_ready': True, 'support': support}))
        finally:
            if route is not None:
                try:
                    request(port, route, method='DELETE')
                except (OSError, RuntimeError):
                    pass
            driver.terminate()
            try:
                driver.wait(timeout=10)
            except subprocess.TimeoutExpired:
                driver.kill()
                driver.wait(timeout=10)

def output_text(value):
    return value.decode('utf-8', errors='replace') if isinstance(value, bytes) else value or ''


def outside():
    SCENARIO.mkdir(mode=0o700)
    signature = CANDIDATE.with_suffix('.AppImage.sig')
    inputs = {'old': SOURCE, 'candidate': CANDIDATE, 'signature': signature,
              'public_key': PUBLIC_KEY, 'helper': HELPER, 'webdriver': DRIVER}
    tool_sources = [SCRIPT, *(SCRIPT.parent / 'native-updater-fixture' / name
                             for name in ('Cargo.toml', 'Cargo.lock', 'src/main.rs'))]
    report = {'schema_version': 1, 'started_at': datetime.now(timezone.utc).isoformat(),
              'source_revision_label': options.source_revision, 'status': 'running',
              'host': platform.platform(), 'python': sys.version,
              'inputs': {}, 'tool_sources': {}, 'host_namespaces': HOST_NAMESPACES,
              'scenarios': [], 'evidence_root': str(ROOT),
              'scope': 'Real copied AppImages and offline official updater install in private Linux namespaces; no production channel or key',
              'cleanup': 'Evidence is retained on success and failure; namespace processes end with the sandbox.'}
    report_path = ROOT / 'acceptance.json'

    def save_report():
        report_path.write_text(json.dumps(report, indent=2) + '\n')

    save_report()
    commands = [
        ('old', ['/usr/bin/python3', '/fixture-bin/acceptance.py', 'old'], False),
        ('read-only', ['/fixture-bin/updater', str(SCENARIO), '--read-only'], True),
        ('replace', ['/fixture-bin/updater', str(SCENARIO)], False),
        ('new', ['/usr/bin/xvfb-run', '-a', '-s', '-screen 0 1280x900x24', '/usr/bin/dbus-run-session', '--', '/usr/bin/python3', '/fixture-bin/acceptance.py', 'new'], False),
        ('new-read-only', ['/usr/bin/xvfb-run', '-a', '-s', '-screen 0 1280x900x24', '/usr/bin/dbus-run-session', '--', '/usr/bin/python3', '/fixture-bin/acceptance.py', 'new-read-only'], False),
    ]
    try:
        report['inputs'] = {name: {'path': str(path), 'bytes': path.stat().st_size, 'sha256': digest(path)}
                            for name, path in inputs.items()}
        report['tool_sources'] = {str(path.relative_to(SCRIPT.parent)): digest(path) for path in tool_sources}
        for source, destination in [(SOURCE, SCENARIO / 'current.AppImage'),
                                    (CANDIDATE, SCENARIO / CANDIDATE.name),
                                    (signature, SCENARIO / (CANDIDATE.name + '.sig')),
                                    (PUBLIC_KEY, SCENARIO / 'test-updater.key.pub')]:
            shutil.copyfile(source, destination)
            require(digest(source) == digest(destination), f'Fixture copy differs: {source.name}')
        (SCENARIO / 'current.AppImage').chmod(0o755)
        save_report()
        for label, command, readonly in commands:
            phase = {'name': label, 'started_at': datetime.now(timezone.utc).isoformat(),
                     'command': command, 'read_only_scenario': readonly,
                     'read_only_image': label == 'new-read-only', 'status': 'running'}
            report['scenarios'].append(phase)
            save_report()
            try:
                result = sandbox(command, readonly, readonly_image=label == 'new-read-only')
                log = SCENARIO / (label + '-result.txt')
                log.write_text(result.stdout + result.stderr)
                phase.update(status='passed', exit_code=result.returncode, log=str(log.relative_to(ROOT)))
                print(label, result.stdout.strip(), flush=True)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
                log = SCENARIO / (label + '-failure.txt')
                log.write_text(output_text(error.stdout) + output_text(error.stderr))
                phase.update(status='failed', exit_code=getattr(error, 'returncode', None),
                             timed_out=isinstance(error, subprocess.TimeoutExpired), log=str(log.relative_to(ROOT)))
                print(log.read_text(), file=sys.stderr)
                raise
            except BaseException as error:
                log = SCENARIO / (label + '-failure.txt')
                log.write_text(f'{type(error).__name__}: {error}\n')
                phase.update(status='failed', exit_code=None, log=str(log.relative_to(ROOT)))
                raise
            finally:
                phase['finished_at'] = datetime.now(timezone.utc).isoformat()
                save_report()
        for name, path in inputs.items():
            require(digest(path) == report['inputs'][name]['sha256'], f'Original input changed during acceptance: {name}')
        for path in tool_sources:
            require(digest(path) == report['tool_sources'][str(path.relative_to(SCRIPT.parent))],
                    f'Tool source changed during acceptance: {path.name}')
        report['status'] = 'passed'
    except BaseException as error:
        report.update(status='failed', failure={'type': type(error).__name__, 'message': str(error)})
        raise
    finally:
        report['finished_at'] = datetime.now(timezone.utc).isoformat()
        report['evidence_files'] = {str(path.relative_to(ROOT)): digest(path) for path in SCENARIO.iterdir()
                                    if path.is_file() and path.suffix in {'.json', '.png', '.log', '.txt'}}
        save_report()
        print(f'Acceptance {report["status"]}: {report_path}', flush=True)


if INSIDE:
    require(len(sys.argv) == 2 and sys.argv[1] in {'old', 'new', 'new-read-only'}, 'Unknown inside scenario.')
    inside(sys.argv[1])
else:
    outside()
