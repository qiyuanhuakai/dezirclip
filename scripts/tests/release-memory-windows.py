"""Compare published alpha.8/alpha.9 bytes on a disposable Windows runner."""
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import statistics
import subprocess
import sys
import time
from urllib.request import ProxyHandler, build_opener

import psutil
import websocket

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from sqlite_history_fixture import connect_fixture_database
from importlib.util import spec_from_file_location, module_from_spec

ROOT = Path(__file__).resolve().parents[2]
spec = spec_from_file_location('seed', ROOT / 'scripts/seed-performance-history.py')
seed = module_from_spec(spec)
spec.loader.exec_module(seed)
OUT = ROOT / '.harness/release-memory'
HTTP = build_opener(ProxyHandler({}))


def save(path, value):
    path.write_text(json.dumps(value, indent=2), encoding='utf-8')


def native_windows(pid):
    found = []
    user = ctypes.windll.user32
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    @callback
    def visit(hwnd, unused):
        owner = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True
    user.EnumWindows(visit, 0)
    return found


def ready(port):
    with HTTP.open(f'http://127.0.0.1:{port}/json/list', timeout=2) as response:
        targets = json.load(response)
    main = [target for target in targets if target.get('type') == 'page' and 'window=' not in target.get('url', '')]
    if len(main) != 1:
        raise RuntimeError('Expected exactly one main WebView')
    with websocket.create_connection(main[0]['webSocketDebuggerUrl'], timeout=3, suppress_origin=True) as ws:
        ws.send(json.dumps({'id': 1, 'method': 'Runtime.evaluate', 'params': {
            'expression': "({rows:document.querySelectorAll('.clipboard-item').length,images:[...document.images].filter(i=>i.complete&&i.naturalWidth>0).length,visible:document.visibilityState})",
            'returnByValue': True}}))
        while True:
            result = json.loads(ws.recv())
            if result.get('id') == 1:
                state = result['result']['result']['value']
                if state['rows'] < 1 or state['visible'] != 'visible':
                    raise RuntimeError(f'History not rendered: {state}')
                return state


def wait_ready(port, process):
    deadline = time.monotonic() + 40
    last = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError('Desktop exited before rendering')
        try:
            if native_windows(process.pid):
                return ready(port)
        except Exception as error:
            last = str(error)
        time.sleep(.25)
    raise RuntimeError(f'Main window failed readiness: {last}')


def run_case(label, version, binaries):
    case = ROOT / '.harness/perf/cases' / label
    case.mkdir(parents=True, exist_ok=False)
    for source in binaries.glob('*.exe'):
        shutil.copy2(source, case / source.name)
    data = case / 'data'
    data.mkdir()
    environment = dict(os.environ)
    environment.update(DEZIRCLIP_DB_PATH=str(data / 'clipboard.db'), WEBVIEW2_USER_DATA_FOLDER=str(case / 'webview'),
                       APPDATA=str(case / 'roaming'), LOCALAPPDATA=str(case / 'local'))
    desktop, cli = case / 'dezirclip.exe', case / 'dzc.exe'
    subprocess.run([str(cli), 'add', 'synthetic initialization'], env=environment, check=True, capture_output=True, timeout=20)
    with connect_fixture_database(data / 'clipboard.db') as db:
        db.execute('DELETE FROM clipboard_history')
        settings = {'app.hotkey': 'Ctrl+Alt+Shift+F9', 'app.quick_paste_hotkey': '', 'app.screenshot_hotkey': '',
                    'app.screenshot_full_hotkey': '', 'app.auto_start': 'false', 'app.silent_start': 'false',
                    'app.window_width': '400', 'app.window_height': '600', 'app.theme': 'retro', 'app.color_mode': 'light',
                    'app.edge_docking': 'false', 'app.window_pinned': 'false', 'app.idle_destroy_enabled': 'true',
                    'app.idle_destroy_seconds': '60', 'app.ocr_enabled': 'false', 'app.light_mode_enabled': 'false'}
        db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES (?,?)', settings.items())
        db.commit()
    save(case / 'case.json', {'executable': str(desktop)})
    seed.seed(case, 200, 24, 3840, 2160)
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = f'--remote-debugging-port={port} --remote-debugging-address=127.0.0.1'
    log = (OUT / f'{label}.log').open('wb')
    process = subprocess.Popen([str(desktop)], env=environment, cwd=case, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    root = psutil.Process(process.pid)
    tracked = {(root.pid, root.create_time()): root}
    report = {'case': label, 'version': version, 'sha256': hashlib.sha256(desktop.read_bytes()).hexdigest(), 'settings': settings, 'phases': {}}

    def observe():
        for parent in list(tracked.values()):
            if parent.is_running():
                for child in parent.children(recursive=True):
                    tracked[(child.pid, child.create_time())] = child
        rows = []
        for identity, child in list(tracked.items()):
            try:
                memory = child.memory_info()
                rows.append({'pid': child.pid, 'created': identity[1], 'name': child.name(), 'working_set': memory.rss, 'private_commit': memory.private})
            except psutil.NoSuchProcess:
                continue
        return {'time': time.monotonic(), 'processes': rows}

    def sample(phase, seconds=20):
        records = []
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError('Desktop terminated during sampling')
            records.append(observe())
            time.sleep(.5)
        save(OUT / f'{label}-{phase}.json', records)
        summary = {}
        for group in ['application', 'webview']:
            for metric in ['working_set', 'private_commit']:
                values = [sum(row[metric] for row in record['processes'] if group == 'application' or row['name'].lower() == 'msedgewebview2.exe') / 1048576 for record in records]
                summary[f'{group}_{metric}_MiB'] = {'median': statistics.median(values), 'min': min(values), 'max': max(values)}
        report['phases'][phase] = summary
        save(OUT / f'{label}.json', report)

    try:
        report['initial_ready'] = wait_ready(port, process)
        time.sleep(30)
        sample('visible')
        handles = native_windows(process.pid)
        if len(handles) != 1:
            raise RuntimeError(f'Unexpected visible window count: {len(handles)}')
        user = ctypes.windll.user32
        user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
        if not user.PostMessageW(handles[0], 0x10, 0, 0):
            raise ctypes.WinError()
        time.sleep(5)
        if native_windows(process.pid):
            raise RuntimeError('Close did not hide window')
        sample('hidden')
        time.sleep(45)
        sample('idle_destroyed')
        if any(row['name'].lower() == 'msedgewebview2.exe' for row in observe()['processes']):
            report['idle_webview_processes_retained'] = True
        for key in [0x11, 0x12, 0x10, 0x78]:
            user.keybd_event(key, 0, 0, 0)
        for key in [0x78, 0x10, 0x12, 0x11]:
            user.keybd_event(key, 0, 2, 0)
        report['resumed_ready'] = wait_ready(port, process)
        time.sleep(30)
        sample('resumed')
        return report
    finally:
        observe()
        for child in reversed(list(tracked.values())):
            try:
                child.kill()
            except psutil.NoSuchProcess:
                pass
        psutil.wait_procs(list(tracked.values()), timeout=10)
        log.close()


def main():
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted' or sys.platform != 'win32':
        raise RuntimeError('Requires a disposable GitHub-hosted Windows runner')
    OUT.mkdir(parents=True, exist_ok=False)
    inputs = {}
    for version in ['0.2.0-alpha.8', '0.2.0-alpha.9']:
        directory = OUT / version
        directory.mkdir()
        filename = f'DezirClip_{version}_x64-setup.exe'
        subprocess.run(['gh', 'release', 'download', 'v' + version, '--pattern', filename, '--dir', str(directory)], check=True)
        archive = directory / filename
        listing = subprocess.check_output(['7z', 'l', '-slt', str(archive)], text=True)
        members = []
        for name in ['dezirclip.exe', 'dzc.exe', 'dzc-service.exe']:
            matches = [line[7:].replace('\\', '/') for line in listing.splitlines() if line.startswith('Path = ') and line.replace('\\', '/').endswith('/' + name) or line == 'Path = ' + name]
            if len(matches) != 1 or any(part in ('', '..', '.') for part in matches[0].split('/')) or ':' in matches[0]:
                raise RuntimeError('Invalid archive role inventory')
            members.extend(matches)
        extracted = directory / 'extracted'
        subprocess.run(['7z', 'x', '-y', '-o' + str(extracted), str(archive), '--', *members], check=True, capture_output=True)
        binaries = directory / 'binaries'
        binaries.mkdir()
        for member in members:
            shutil.copy2(extracted / member, binaries / Path(member).name)
        inputs[version] = binaries
        save(directory / 'identity.json', {'installer_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'members': members})
    reports = []
    for index, version in enumerate(['0.2.0-alpha.8', '0.2.0-alpha.9', '0.2.0-alpha.9', '0.2.0-alpha.8']):
        print(f'Measuring case {index + 1}: {version}', flush=True)
        reports.append(run_case(f'release-memory-{index + 1}', version, inputs[version]))
        save(OUT / 'comparison.json', reports)


if __name__ == '__main__':
    main()
