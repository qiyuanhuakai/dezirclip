"""Measure installed Desktop capture transport in disposable Linux namespaces.

No product instrumentation is inserted. Functional mode is never timing eligible.
The primary clock starts in the real WebView before the external writer is launched
and ends at its native capture event, not at a headless engine method boundary.
"""
import argparse
import base64
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import re
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen
import zlib

SCRIPT = Path(__file__).resolve()
FIXTURE_HELPER = SCRIPT.parent.parent / 'sqlite_history_fixture.py'
MARKER = b'Synthetic native Desktop capture performance fixture; no host clipboard or profile.\n'
NAMESPACES = ('user', 'pid', 'net', 'ipc', 'uts', 'mnt')
CASES = ('text-200b', 'text-1mib', 'html-3kib', 'rgba-4k')
IDENTIFIER = 'io.github.qiyuanhuakai.dezirclip'
INTERVAL_SECONDS = .7
MINIMUM_SQLITE = (3, 50, 4)
SYSTEM_PYTHON = Path('/usr/bin/python3')
BASELINES = {
    '802808b06e6b795064c5065d1d880bae9b193b9a': 'clipboard-updated',
    '90ba9960744b6cd57caaaf597f68533a7e41be45': 'clipboard-captured',
}
CAPTURE_LISTENER = '''
const done=arguments[arguments.length-1], contract=arguments[0];
window.__nativeCapturePerf={events:[],arm:null,error:null};
const handler=window.__TAURI_INTERNALS__.transformCallback(event=>{
  const arrival=performance.now(), state=window.__nativeCapturePerf, payload=event.payload;
  const keys=Object.keys(payload??{}).sort();
  if(contract.keys && (JSON.stringify(keys)!==JSON.stringify(contract.keys) || !Number.isSafeInteger(payload?.id) || payload.id<=0)) {
    state.error="Capture event violated its metadata contract"; return;
  }
  if(contract.host_instance_id && payload.host_instance_id!==contract.host_instance_id) {
    state.error="Capture event came from a different Host instance"; return;
  }
  const observed={id:payload?.id,keys,arrival_ms:arrival};
  if(contract.host_instance_id) observed.host_instance_id=payload.host_instance_id;
  state.events.push(observed);
  if(state.arm && observed.id===state.arm.expectedId && !state.arm.hit) state.arm.hit={...observed,elapsed_ms:arrival-state.arm.start};
});
window.__TAURI_INTERNALS__.invoke("plugin:event|listen",{event:contract.event,target:{kind:"Any"},handler}).then(id=>done({ok:true,id})).catch(error=>done({ok:false,error:String(error)}));
'''


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def capture_event_contract(event, build_info=None, source_revision=None, host_status=None):
    if event == 'clipboard-updated':
        return {'event': event, 'keys': None, 'host_instance_id': None}
    require(event == 'clipboard-captured', 'Unknown capture event contract')
    require(isinstance(build_info, dict) and build_info.get('source_revision') == source_revision and
            isinstance(source_revision, str) and re.fullmatch(r'[0-9a-f]{40}', source_revision), 'Actual GUI build revision differs from its proof')
    inventory = build_info.get('linked_components')
    require(isinstance(inventory, list) and all(isinstance(item, dict) and isinstance(item.get('component'), str) for item in inventory), 'Actual GUI component inventory is missing')
    components = [item['component'] for item in inventory]
    expected = {'dzc-core', 'dzc-platform', 'dzc-protocol'}
    require(len(components) == len(set(components)) and set(components) in (expected, expected | {'dzc-host'}), 'Unknown actual GUI component inventory')
    host = None
    if 'dzc-host' in components:
        state = host_status.get('state') if isinstance(host_status, dict) else None
        host = state.get('service_instance_id') if isinstance(state, dict) else None
        require(isinstance(host, str) and re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', host), 'Actual GUI Host identity is missing')
    return {'event': event, 'keys': ['host_instance_id', 'id'] if host else ['id'], 'host_instance_id': host}


def validated_listener_events(snapshot):
    require(isinstance(snapshot, dict) and isinstance(snapshot.get('events'), list), 'Native event listener state is missing')
    require(not snapshot.get('error'), snapshot.get('error'))
    return snapshot['events']


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def save(path, value):
    temporary = path.with_name(path.name + '.pending')
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')
    temporary.replace(path)


def timestamp():
    return datetime.now(timezone.utc).isoformat()


def namespaces():
    return {name: os.readlink(f'/proc/self/ns/{name}') for name in NAMESPACES}


def regular(path):
    require(not path.is_symlink() and stat.S_ISREG(path.stat().st_mode), f'Regular input required: {path}')
    return path.resolve(strict=True)


def python_environment(prefix):
    environment = {'PYTHONNOUSERSITE': '1', 'PYTHONDONTWRITEBYTECODE': '1', 'PYTHONSAFEPATH': '1'}
    if prefix:
        environment['PYTHONHOME'] = str(prefix)
    return environment


def module_location(module):
    path = getattr(module, '__file__', None)
    if path:
        return str(regular(Path(path)))
    origin = getattr(getattr(module, '__spec__', None), 'origin', None)
    require(origin in ('built-in', 'frozen'), 'Runtime module has no verifiable file or built-in origin')
    return origin


def runtime_details():
    import _sqlite3
    import encodings
    import sqlite3
    modules = {name: module_location(module) for name, module in (('_sqlite3', _sqlite3), ('sqlite3', sqlite3), ('encodings', encodings))}
    paths = {Path(sys.executable).resolve(strict=True), *(Path(path) for path in modules.values() if path not in ('built-in', 'frozen'))}
    for line in Path('/proc/self/maps').read_text().splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) == 6 and fields[5].startswith('/'):
            mapped = Path(fields[5])
            if mapped.name.startswith(('libpython', 'libsqlite3')):
                paths.add(regular(mapped))
    with closing(sqlite3.connect(':memory:')) as connection:
        source_id = connection.execute('SELECT sqlite_source_id()').fetchone()[0]
    return {'python': sys.version, 'python_version_info': list(sys.version_info[:3]),
            'sqlite': sqlite3.sqlite_version, 'sqlite_source_id': source_id,
            'executable': str(Path(sys.executable).resolve(strict=True)),
            'prefix': str(Path(sys.prefix).resolve(strict=True)), 'base_prefix': str(Path(sys.base_prefix).resolve(strict=True)),
            'module_origins': modules,
            'key_files': {str(path): {'sha256': digest(path), 'bytes': path.stat().st_size} for path in sorted(paths)}}


def validate_runtime(details, expected=None):
    require(tuple(details['python_version_info'][:2]) >= (3, 13), 'Native capture requires Python 3.13 or newer')
    require(re.fullmatch(r'\d+\.\d+\.\d+', details['sqlite']), 'Unrecognized runtime SQLite version')
    require(tuple(int(part) for part in details['sqlite'].split('.')) >= MINIMUM_SQLITE,
            'Native capture requires SQLite >= 3.50.4; older external-content FTS writers are unsupported')
    require(details.get('sqlite_source_id') and details.get('key_files'), 'Runtime provenance is incomplete')
    if expected is not None:
        require(details == expected, 'Python/SQLite runtime differs from the host preflight proof')


def inspect_python_runtime(prefix, script, evidence_directory=None):
    if prefix:
        prefix = Path(prefix).resolve(strict=True)
        require(prefix.is_dir() and prefix not in (Path.home().resolve(), *Path.home().resolve().parents)
                and str(prefix) not in ('/usr', '/usr/local', '/etc', '/lib', '/lib64', '/tmp', '/run'),
                'Python runtime must be a standalone prefix, not a host directory')
        executable = regular(prefix / 'bin/python3.13')
        require(executable.is_relative_to(prefix) and (prefix / 'lib/python3.13').is_dir(), 'Expected a standalone Python 3.13 layout')
    else:
        executable = regular(SYSTEM_PYTHON.resolve(strict=True))
    environment = {'HOME': '/nonexistent', 'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', **python_environment(prefix)}
    probe = subprocess.run([str(executable), '-s', str(script), '--runtime-probe'], cwd='/', env=environment,
                           capture_output=True, text=True, timeout=30)
    if evidence_directory:
        evidence_directory.mkdir()
        (evidence_directory / 'stdout.log').write_text(probe.stdout)
        (evidence_directory / 'stderr.log').write_text(probe.stderr)
    require(probe.returncode == 0, f'Python runtime probe failed ({probe.returncode}): {probe.stderr.strip()}')
    details = json.loads(probe.stdout)
    require(details['executable'] == str(executable), 'Runtime probe executed a different interpreter')
    if prefix:
        require(details['prefix'] == str(prefix) and details['base_prefix'] == str(prefix), 'Runtime prefix escaped its standalone installation')
        require(all(Path(path).resolve(strict=True).is_relative_to(prefix) for path in details['key_files']), 'Standalone runtime loads Python or SQLite outside its prefix')
    return {'prefix': str(prefix) if prefix else None, 'executable': str(executable), 'details': details}


def validate_root(root):
    require(root.parent == Path('/tmp').resolve() and re.fullmatch(r'dezirclip-native-capture-[a-zA-Z0-9_-]+', root.name), 'Expected a disposable /tmp/dezirclip-native-capture-* root')
    marker = regular(root / '.native-capture-fixture')
    require(marker.read_bytes() == MARKER, 'Fixture marker mismatch')


def beneath(path, root):
    result = path.resolve(strict=True)
    require(result != root and root in result.parents, 'Path escapes the disposable fixture')
    return result


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def png_chunk(kind, payload):
    return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind + payload) & 0xffffffff)


def image_payload(path, ordinal):
    width, height = 3840, 2160
    color = ((ordinal >> 16) & 255, (ordinal >> 8) & 255, ordinal & 255, 255)
    row = bytes(color) * width
    noise = bytearray(random.Random(20261019).randbytes(512 * 512 * 4))
    noise[3::4] = b'\xff' * (512 * 512)
    compressor = zlib.compressobj(3)
    chunks = []
    for y in range(height):
        pixels = row if y >= 512 else row[:-2048] + noise[y * 2048:(y + 1) * 2048]
        chunks.append(compressor.compress(b'\0' + pixels))
    chunks.append(compressor.flush())
    encoded = b''.join(chunks)
    header = struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', header) + png_chunk(b'IDAT', encoded) + png_chunk(b'IEND', b''))
    return {'width': width, 'height': height, 'channels': 4, 'decoded_rgba_bytes': width * height * 4, 'corner_rgba': list(color)}


def prepare_payloads(root, cases, count):
    directory = root / 'payloads'
    directory.mkdir()
    result = {}
    nonce = root.name.rsplit('-', 1)[-1]
    for case in cases:
        result[case] = []
        for index in range(count):
            path = directory / f'{case}-{index:03}.bin'
            item = {'case': case, 'index': index, 'path': str(path)}
            if case == 'rgba-4k':
                item.update(mime='image/png', image=image_payload(path, index + 1))
            else:
                token = f'nativecapture{nonce}_{case}_{index:03} '
                size = {'text-200b': 200, 'text-1mib': 1024 * 1024, 'html-3kib': 3 * 1024}[case]
                if case == 'html-3kib':
                    value = '<p>' + token + 'x' * (size - len(token) - 7) + '</p>'
                    item['mime'] = 'text/html'
                else:
                    line = 'Synthetic clipboard text with preserved CRLF.\r\n'
                    value = token + (line * (size // len(line) + 1))[:size - len(token)]
                    item['mime'] = 'text/plain'
                require(len(value.encode()) == size, 'Fixture payload size changed')
                path.write_bytes(value.encode())
            item.update(bytes=path.stat().st_size, sha256=digest(path))
            result[case].append(item)
    return result


def load_proof(path, gui, cli):
    value = json.loads(path.read_text())
    require(re.fullmatch('[0-9a-fA-F]{40}', value.get('source_revision', '')), 'Build proof requires a full source revision')
    require(value.get('profile') == 'release', 'Both Desktop inputs must use release builds')
    for name, binary in [('gui', gui), ('cli', cli)]:
        require(value.get('binaries', {}).get(name, {}).get('sha256', '').lower() == digest(binary), f'{name} binary differs from build proof')
    return value


def baseline_contract(proof, revision):
    require(revision in BASELINES, 'Select an explicitly supported full baseline revision')
    require(proof.get('source_revision') == revision, 'Baseline proof differs from the explicitly selected source')
    return {'source_revision': revision, 'event': BASELINES[revision]}


def request(port, route, payload=None, method=None):
    data = json.dumps(payload).encode() if payload is not None else None
    query = Request(f'http://127.0.0.1:{port}/{route}', data=data, method=method, headers={'Content-Type': 'application/json'})
    try:
        with urlopen(query, timeout=35) as response:
            value = json.load(response).get('value')
            require(not (isinstance(value, dict) and value.get('error')), f'WebDriver failed: {value}')
            return value
    except HTTPError as error:
        raise RuntimeError(error.read().decode()) from error


class Driver:
    def __init__(self, directory, executable):
        self.directory, self.executable, self.route, self.process = directory, executable, None, None

    def __enter__(self):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            self.port = listener.getsockname()[1]
        self.log = (self.directory / 'webdriver.log').open('w')
        self.process = subprocess.Popen(['/fixture-bin/WebKitWebDriver', f'--port={self.port}', '--host=local'], stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(200):
            try:
                request(self.port, 'status')
                break
            except OSError:
                require(self.process.poll() is None, 'WebKitWebDriver exited during startup')
                time.sleep(.1)
        else:
            raise RuntimeError('WebKitWebDriver did not become ready')
        session = request(self.port, 'session', {'capabilities': {'alwaysMatch': {'webkitgtk:browserOptions': {'binary': self.executable}}}})
        self.route = f'session/{session["sessionId"]}'
        request(self.port, self.route + '/timeouts', {'script': 30000, 'pageLoad': 30000, 'implicit': 0})
        for _ in range(300):
            if self.execute('return typeof window.__TAURI_INTERNALS__?.invoke === "function" && document.body.innerText.length > 0;'):
                return self
            time.sleep(.1)
        raise RuntimeError('Actual Desktop WebView did not initialize')

    def execute(self, script, args=None):
        return request(self.port, self.route + '/execute/sync', {'script': script, 'args': args or []})

    def invoke(self, command, args=None):
        value = request(self.port, self.route + '/execute/async', {'script': '''
const done=arguments[arguments.length-1];
window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1]).then(value=>done({ok:true,value})).catch(error=>done({ok:false,error:String(error)}));
''', 'args': [command, args or {}]})
        require(value.get('ok'), f'Actual Desktop invoke failed: {value}')
        return value.get('value')

    def close(self):
        if self.route:
            try:
                request(self.port, self.route, method='DELETE')
            except (OSError, RuntimeError):
                pass
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if hasattr(self, 'log'):
            self.log.close()

    def __exit__(self, *_):
        self.close()


def seed_database(directory, rows, variant):
    sys.path.insert(0, '/fixture-bin')
    from sqlite_history_fixture import connect_fixture_database
    data = directory / 'profile/data' / IDENTIFIER
    data.mkdir(parents=True)
    database = data / 'clipboard.db'
    os.environ['DEZIRCLIP_DB_PATH'] = str(database)
    executable = '/fixture-bin/old-cli' if variant == 'old' else '/fixture-bin/writer'
    with (directory / 'bootstrap.log').open('w') as output:
        subprocess.run([executable, 'add', 'nativecapturebootstrap'], check=True, stdout=output, stderr=subprocess.STDOUT, timeout=30)
    with closing(connect_fixture_database(database)) as connection:
        require(connection.execute('SELECT count(*) FROM clipboard_history').fetchone()[0] == 1, 'Bootstrap database is not disposable')
        columns = {row[1] for row in connection.execute('PRAGMA table_info(clipboard_history)')}
        connection.execute('DELETE FROM clipboard_history')
        values = []
        for index in range(1, rows + 1):
            content = f'Neutral synthetic native capture seed {index:05d}. ' + 'x' * 140
            record = {'id': index, 'content_type': 'text', 'content': content, 'preview': content, 'source_app': 'Native capture fixture', 'timestamp': 1600000000000 + index,
                      'html_content': None, 'tags': '[]', 'is_pinned': 0, 'use_count': 0, 'is_external': 0, 'pinned_order': 0,
                      'content_hash': int.from_bytes(hashlib.sha256(content.encode()).digest()[:8], 'little', signed=True), 'content_kinds': '[]', 'ocr_status': 'skipped'}
            selected = [name for name in record if name in columns]
            values.append(tuple(record[name] for name in selected))
        connection.executemany(f'INSERT INTO clipboard_history({",".join(selected)}) VALUES({",".join("?" for _ in selected)})', values)
        settings = {'app.persistent': 'true', 'app.deduplicate': 'true', 'app.persistent_limit_enabled': 'false', 'app.idle_destroy_enabled': 'false',
                    'app.capture_files': 'true', 'app.capture_rich_text': 'true', 'app.privacy_protection': 'false', 'app.ocr_enabled': 'false',
                    'app.sequential_mode': 'false', 'app.sound_enabled': 'false', 'app.silent_start': 'false'}
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', settings.items())
        connection.commit()
        require(connection.execute('PRAGMA quick_check').fetchone()[0] == 'ok', 'Seed SQLite integrity failed')
        connection.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        proof = {'database': str(database), 'rows': rows, 'user_version': connection.execute('PRAGMA user_version').fetchone()[0], 'settings': settings}
    save(directory / 'seed.json', proof)
    return database


def png_info(path):
    with path.open('rb') as source:
        require(source.read(8) == b'\x89PNG\r\n\x1a\n', 'Stored image is not PNG')
        decoder, first, header, ended = zlib.decompressobj(), b'', None, False
        while True:
            length = source.read(4)
            require(len(length) == 4, 'Stored PNG is truncated')
            size = struct.unpack('>I', length)[0]
            require(size <= 64 * 1024 * 1024, 'Stored PNG chunk exceeds fixture limit')
            kind, data, crc = source.read(4), source.read(size), source.read(4)
            require(len(data) == size and len(crc) == 4 and zlib.crc32(kind + data) & 0xffffffff == struct.unpack('>I', crc)[0], 'Stored PNG CRC failed')
            if kind == b'IHDR':
                header = struct.unpack('>IIBBBBB', data)
            if kind == b'IDAT' and len(first) < 5:
                first += decoder.decompress(data, 5 - len(first))
            if kind == b'IEND':
                ended = True
                break
        require(ended and header and header[2:4] == (8, 6) and header[6] == 0 and len(first) == 5 and first[0] in range(5), 'Expected a complete noninterlaced 8-bit RGBA PNG')
        return {'width': header[0], 'height': header[1], 'channels': 4, 'corner_rgba': list(first[1:]), 'sha256': digest(path), 'bytes': path.stat().st_size}


def validate_sample(connection, directory, payload, event, before):
    count, previous_id = before
    row = connection.execute('SELECT id,content_type,content,html_content,is_external FROM clipboard_history WHERE id=?', (event['id'],)).fetchone()
    require(row is not None and row[0] > previous_id, 'Capture event has no newly persisted SQLite row')
    require(connection.execute('SELECT count(*) FROM clipboard_history').fetchone()[0] == count + 1, 'Capture did not persist exactly one unique row')
    result = {'id': row[0], 'content_type': row[1], 'rows': count + 1}
    if payload['case'] == 'rgba-4k':
        require(row[1] == 'image' and row[4] == 1 and not row[2].startswith('data:'), 'Persisted image did not externalize to an actual file')
        path = beneath(Path(row[2]), directory)
        require(directory / 'profile/data' / IDENTIFIER / 'attachments' in path.parents, 'Image is outside the fixture attachment directory')
        image = png_info(regular(path))
        for field in ('width', 'height', 'channels', 'corner_rgba'):
            require(image[field] == payload['image'][field], f'Stored image differs: {field}')
        result['image'] = image
    else:
        expected = Path(payload['path']).read_bytes().decode()
        require(row[2] == expected, 'SQLite did not retain the complete original text')
        require(row[1] == ('rich_text' if payload['case'] == 'html-3kib' else 'text'), 'Capture classification changed')
        require(row[3] == (expected if payload['case'] == 'html-3kib' else None), 'SQLite HTML differs from the written representation')
        result.update(content_bytes=len(row[2].encode()), content_sha256=hashlib.sha256(row[2].encode()).hexdigest(), html_sha256=hashlib.sha256(row[3].encode()).hexdigest() if row[3] else None)
    return result


def inside(root, plan, block):
    validate_root(root)
    require(all(namespaces()[name] != plan['host_namespaces'][name] for name in NAMESPACES), 'A private namespace is missing')
    require(os.environ.get('HOME') == '/home/fixture' and os.environ.get('TAURI_WEBVIEW_AUTOMATION') == 'true', 'Sandbox environment was not rebuilt')
    require(os.environ.get('DISPLAY') and os.environ.get('DBUS_SESSION_BUS_ADDRESS') and not os.environ.get('WAYLAND_DISPLAY'), 'Private Xvfb/DBus session is missing')
    directory = beneath(root / block['directory'], root)
    evidence = {'schema_version': 1, 'status': 'running', 'timing_eligible': plan['mode'] == 'formal', 'mode': plan['mode'], 'block': block,
                'namespaces': namespaces(), 'started_at': timestamp(), 'loadavg_at_start': list(os.getloadavg()),
                'environment': {name: os.environ.get(name) for name in ('HOME', 'DISPLAY', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS', 'TAURI_WEBVIEW_AUTOMATION', 'WEBKIT_DISABLE_DMABUF_RENDERER', 'WEBKIT_DISABLE_COMPOSITING_MODE', 'DEZIRCLIP_DISABLE_LINUX_SERVICES', 'TIEZ_DISABLE_LINUX_SERVICES')}, 'samples': []}
    destination = directory / 'block.json'
    save(destination, evidence)
    driver = None
    try:
        evidence['python_runtime'] = runtime_details()
        validate_runtime(evidence['python_runtime'], plan['python_runtime']['details'])
        if plan['mode'] == 'isolation':
            evidence.update(status='passed', timing_eligible=False, desktop_capture_verified=False)
            return
        database = seed_database(directory, block['rows'], block['variant'])
        evidence['database'] = str(database)
        gui = '/fixture-bin/old-gui' if block['variant'] == 'old' else '/fixture-bin/candidate-gui'
        driver = Driver(directory, gui)
        driver.__enter__()
        data_path = driver.invoke('get_data_path')
        require(Path(data_path).resolve() == database.parent, 'Actual GUI opened a different profile than its CLI bootstrap')
        driver.invoke('focus_clipboard_window')
        evidence['gui'] = {'data_path': data_path, 'version': driver.invoke('plugin:app|version')}
        event_name = plan['baseline']['event'] if block['variant'] == 'old' else 'clipboard-captured'
        build_info = driver.invoke('get_build_info') if event_name == 'clipboard-captured' else None
        inventory = build_info.get('linked_components') if isinstance(build_info, dict) else None
        has_host = isinstance(inventory, list) and any(isinstance(item, dict) and item.get('component') == 'dzc-host' for item in inventory)
        host_status = driver.invoke('get_clipboard_host_status') if has_host else None
        contract = capture_event_contract(event_name, build_info, plan['source_revisions'][block['variant']], host_status)
        evidence['gui'].update(build_info=build_info, initial_host_status=host_status)
        evidence['event_contract'] = contract
        listener = request(driver.port, driver.route + '/execute/async', {'script': CAPTURE_LISTENER, 'args': [contract]})
        require(listener.get('ok'), 'Actual event listener could not register')
        evidence['event'] = event_name
        time.sleep(1)
        sys.path.insert(0, '/fixture-bin')
        from sqlite_history_fixture import connect_fixture_database
        with closing(connect_fixture_database(f'file:{database}?mode=ro', uri=True, timeout=5)) as connection:
            require(connection.execute('SELECT count(*) FROM clipboard_history').fetchone()[0] == block['rows'], 'GUI changed the neutral seed before the first sample')
            previous_arm = None
            for case in plan['cases']:
                for index, payload in enumerate(plan['payloads'][case]):
                    require(digest(regular(Path(payload['path']))) == payload['sha256'], 'Fixture payload changed before the write')
                    before = connection.execute('SELECT count(*),coalesce(max(id),0) FROM clipboard_history').fetchone()
                    sample = {'case': case, 'index': index, 'warmup': index < plan['warmup'], 'status': 'running', 'started_at': timestamp(), 'payload_sha256': payload['sha256'], 'payload_bytes': payload['bytes'], 'expected_id': before[1] + 1, 'cooldown_seconds': INTERVAL_SECONDS}
                    evidence['samples'].append(sample)
                    save(destination, evidence)
                    arm_begin = time.perf_counter()
                    if previous_arm is not None:
                        sample['previous_arm_spacing_ms'] = (arm_begin - previous_arm) * 1000
                        require(sample['previous_arm_spacing_ms'] >= INTERVAL_SECONDS * 1000, 'Sample arms overlapped the notification cooldown')
                    previous_arm = arm_begin
                    arm = driver.execute('const state=window.__nativeCapturePerf; if(state.error) throw Error(state.error); state.arm={token:arguments[0],expectedId:arguments[1],start:performance.now(),hit:null}; return {token:state.arm.token,start:state.arm.start};', [f'{case}-{index}', before[1] + 1])
                    sample['arm_rpc_elapsed_ms'] = (time.perf_counter() - arm_begin) * 1000
                    sample['arm'] = arm
                    writer_start = time.perf_counter()
                    with Path(payload['path']).open('rb') as source:
                        written = subprocess.run(['/fixture-bin/writer', 'clipboard', 'write', '-', '--mime', payload['mime']], stdin=source, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
                    sample.update(writer_elapsed_ms=(time.perf_counter() - writer_start) * 1000, writer_exit_code=written.returncode, writer_stdout=written.stdout.decode(errors='replace'), writer_stderr=written.stderr.decode(errors='replace'))
                    require(written.returncode == 0, 'External clipboard writer failed')
                    deadline = time.monotonic() + 30
                    while True:
                        observed = driver.execute('const state=window.__nativeCapturePerf; return {hit:state.arm.hit,error:state.error};')
                        require(not observed['error'], observed['error'])
                        if observed['hit']:
                            break
                        require(time.monotonic() < deadline, 'Persisted capture event timed out')
                        time.sleep(.01)
                    sample.update(event=observed['hit'], event_elapsed_ms=observed['hit']['elapsed_ms'], host_arm_to_observation_ms=(time.perf_counter() - arm_begin) * 1000)
                    require(math.isfinite(sample['event_elapsed_ms']) and sample['event_elapsed_ms'] > 0, 'Invalid native capture clock')
                    sample['postcondition'] = validate_sample(connection, directory, payload, observed['hit'], before)
                    sample['status'] = 'passed'
                    sample['finished_at'] = timestamp()
                    save(destination, evidence)
                    print(f'{block["name"]} rows={block["rows"]} {case} {index+1}/{len(plan["payloads"][case])} {"warmup" if sample["warmup"] else "sample"} passed', flush=True)
                    time.sleep(INTERVAL_SECONDS)
            evidence['final_rows'] = connection.execute('SELECT count(*) FROM clipboard_history').fetchone()[0]
            require(evidence['final_rows'] == block['rows'] + len(plan['cases']) * (plan['warmup'] + plan['samples']), 'Final history row count differs')
            require(connection.execute('PRAGMA quick_check').fetchone()[0] == 'ok', 'Post-capture SQLite integrity failed')
        screenshot = request(driver.port, driver.route + '/screenshot')
        (directory / 'desktop.png').write_bytes(base64.b64decode(screenshot))
        evidence['events'] = validated_listener_events(driver.execute('return {events:window.__nativeCapturePerf.events,error:window.__nativeCapturePerf.error};'))
        evidence['status'] = 'passed'
    except BaseException as error:
        evidence.update(status='failed', failure={'type': type(error).__name__, 'message': str(error)}, timing_eligible=False)
        if evidence['samples'] and evidence['samples'][-1]['status'] == 'running':
            evidence['samples'][-1]['status'] = 'failed'
        raise
    finally:
        evidence['finished_at'] = timestamp()
        evidence['loadavg_at_end'] = list(os.getloadavg())
        save(destination, evidence)
        if driver:
            driver.close()


def sandbox(root, plan, block, inputs, driver):
    directory = root / block['directory']
    directory.mkdir(parents=True)
    arguments = ['bwrap', '--unshare-user', '--unshare-pid', '--unshare-net', '--unshare-ipc', '--unshare-uts', '--die-with-parent', '--new-session', '--clearenv',
                 '--ro-bind', '/usr', '/usr', '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/sbin', '/sbin', '--ro-bind', '/lib', '/lib', '--ro-bind', '/lib64', '/lib64', '--ro-bind', '/etc', '/etc',
                 '--dev', '/dev', '--proc', '/proc', '--tmpfs', '/tmp', '--tmpfs', '/run', '--dir', '/run/fixture', '--chmod', '0700', '/run/fixture', '--dir', '/home/fixture', '--dir', '/fixture-bin',
                 '--ro-bind', str(root / 'tool-sources' / SCRIPT.name), '/fixture-bin/native_capture.py', '--ro-bind', str(root / 'tool-sources' / FIXTURE_HELPER.name), '/fixture-bin/sqlite_history_fixture.py',
                 '--dir', str(root), '--ro-bind', str(root / '.native-capture-fixture'), str(root / '.native-capture-fixture'), '--ro-bind', str(root / 'plan.json'), str(root / 'plan.json'), '--bind', str(directory), str(directory)]
    if plan['mode'] != 'isolation':
        arguments += ['--ro-bind', str(root / 'payloads'), str(root / 'payloads'), '--ro-bind', str(driver), '/fixture-bin/WebKitWebDriver']
        for name, value in inputs.items():
            arguments += ['--ro-bind', str(value), f'/fixture-bin/{name}']
    runtime = plan['python_runtime']
    if runtime['prefix']:
        arguments += ['--ro-bind', runtime['prefix'], runtime['prefix']]
    environment = {'HOME': '/home/fixture', 'XDG_DATA_HOME': str(directory / 'profile/data'), 'XDG_CONFIG_HOME': str(directory / 'profile/config'), 'XDG_CACHE_HOME': str(directory / 'profile/cache'),
                   'XDG_RUNTIME_DIR': '/run/fixture', 'PATH': '/usr/bin:/bin:/fixture-bin', 'LANG': 'C.UTF-8', 'TZ': 'UTC', 'TAURI_WEBVIEW_AUTOMATION': 'true',
                   'WEBKIT_DISABLE_DMABUF_RENDERER': '1', 'WEBKIT_DISABLE_COMPOSITING_MODE': '1', 'DEZIRCLIP_NATIVE_CAPTURE_INSIDE': '1',
                   'DEZIRCLIP_DISABLE_LINUX_SERVICES': 'window_tracker,edge_docking,mouse_hotkey', 'TIEZ_DISABLE_LINUX_SERVICES': 'window_tracker,edge_docking,mouse_hotkey'}
    environment.update(python_environment(runtime['prefix']))
    for key, value in environment.items():
        arguments += ['--setenv', key, value]
    arguments += ['--chdir', str(directory), '/usr/bin/xvfb-run', '-a', '-s', '-screen 0 1280x900x24 -noreset', '/usr/bin/dbus-run-session', '--', runtime['executable'], '-s', '/fixture-bin/native_capture.py', '--inside', str(root), block['name']]
    return arguments


def outside(options):
    require(sys.platform == 'linux', 'Native Desktop capture requires Linux')
    for tool in ('bwrap', 'xvfb-run', 'dbus-run-session'):
        require(shutil.which(tool), f'Missing isolation tool: {tool}')
    root = Path(tempfile.mkdtemp(prefix='dezirclip-native-capture-', dir='/tmp')).resolve()
    (root / '.native-capture-fixture').write_bytes(MARKER)
    (root / 'tool-sources').mkdir()
    for source in (SCRIPT, FIXTURE_HELPER):
        shutil.copyfile(source, root / 'tool-sources' / source.name)
    print(f'Native capture evidence: {root}', flush=True)
    inputs, proofs = {}, {}
    report = {'schema_version': 1, 'status': 'running', 'mode': options.mode, 'timing_eligible': False, 'evidence_root': str(root), 'started_at': timestamp(),
              'boundary': 'Real WebView performance.now arm to native persisted capture event; includes WebDriver arm return, external CLI startup and native OS write. Not pure engine latency.',
              'host': {'platform': platform.platform(), 'python': sys.version, 'machine': platform.machine(), 'cpu': Path('/proc/cpuinfo').read_text(), 'loadavg_at_start': list(os.getloadavg())},
              'tool_sources': {str(SCRIPT.name): digest(SCRIPT), str(FIXTURE_HELPER.name): digest(FIXTURE_HELPER)}, 'blocks': [], 'cleanup': 'No host profile, display, clipboard, DBus or network is mounted; all evidence and failures are retained.'}
    destination = root / 'report.json'
    save(destination, report)
    try:
        for source in (SCRIPT, FIXTURE_HELPER):
            require(digest(root / 'tool-sources' / source.name) == report['tool_sources'][source.name], 'Measurement tool changed while its snapshot was copied')
        runtime = inspect_python_runtime(options.python_runtime, root / 'tool-sources' / SCRIPT.name, root / 'runtime-host-before')
        report['python_runtime'] = runtime
        save(destination, report)
        validate_runtime(runtime['details'])
        save(root / 'python-runtime.json', runtime)
        report['python_runtime_proof_sha256'] = digest(root / 'python-runtime.json')
        if options.mode != 'isolation':
            require(not options.baseline_only or options.mode == 'functional', 'Baseline-only is a functional tool check, never a formal comparison')
            required = ('old_gui', 'old_cli', 'writer', 'old_proof', 'webkit_driver')
            if not options.baseline_only:
                required += ('candidate_gui', 'candidate_proof')
            for name in required:
                require(getattr(options, name), f'--{name.replace("_", "-")} is required')
            names = ('old-gui', 'old-cli', 'writer') if options.baseline_only else ('old-gui', 'old-cli', 'candidate-gui', 'writer')
            inputs = {name: regular(Path(getattr(options, name.replace('-', '_')))) for name in names}
            driver = regular(Path(options.webkit_driver))
            proof_paths = [('old', options.old_proof)] + ([] if options.baseline_only else [('candidate', options.candidate_proof)])
            proofs = {variant: load_proof(regular(Path(proof)), inputs['old-gui' if variant == 'old' else 'candidate-gui'], inputs['old-cli' if variant == 'old' else 'writer']) for variant, proof in proof_paths}
            report['baseline'] = baseline_contract(proofs['old'], options.baseline_revision)
            if options.baseline_only:
                require(digest(inputs['writer']) == digest(inputs['old-cli']), 'Baseline-only tool checks must explicitly use the proven old CLI writer')
                report['writer_scope'] = 'Old CLI used only to verify baseline tool functionality; not eligible for comparison.'
            else:
                require(proofs['old'].get('rustc_version') == proofs['candidate'].get('rustc_version') and '1.91.1' in proofs['old'].get('rustc_version', ''), 'Both builds must use the same Rust 1.91.1 compiler')
                require(proofs['old'].get('target') == proofs['candidate'].get('target') and proofs['old'].get('target'), 'Both builds must use the same target')
                report['writer_scope'] = 'One proven candidate CLI writer for every old and candidate GUI block.'
            report['inputs'] = {name: {'path': str(value), 'sha256': digest(value), 'bytes': value.stat().st_size} for name, value in {**inputs, 'webkit-driver': driver}.items()}
            report['build_proofs'] = proofs
            for variant, source in proof_paths:
                copied = root / f'{variant}-build.json'
                shutil.copyfile(source, copied)
                report.setdefault('build_proof_sha256', {})[variant] = digest(copied)
        else:
            driver = None
        if options.mode == 'formal':
            require(options.samples >= 30 and options.warmup >= 5 and sorted(options.rows) == [1000, 10000] and tuple(options.cases) == CASES, 'Formal ABBA requires all cases/1k+10k and at least 30 samples + 5 warmups')
            require(options.quiet_window_note, 'Formal ABBA requires the frozen-source/quiet-window record')
            note = regular(Path(options.quiet_window_note))
            shutil.copyfile(note, root / 'quiet-window-note.txt')
            report['quiet_window_note_sha256'] = digest(note)
        else:
            require(1 <= options.samples <= 5 and 0 <= options.warmup <= 2, 'Functional mode only permits low-count verification; it is not timing eligible')
        order = [('A1', 'old'), ('B1', 'candidate'), ('B2', 'candidate'), ('A2', 'old')] if options.mode == 'formal' else [('A-functional', 'old'), ('B-functional', 'candidate')]
        if options.baseline_only:
            order = [('A-functional', 'old')]
        blocks = [{'name': f'{name}-{rows}', 'variant': variant, 'rows': rows, 'directory': f'rows-{rows}/{name}'} for rows in options.rows for name, variant in order]
        if options.mode == 'isolation':
            blocks = [{'name': 'isolation', 'variant': None, 'rows': 0, 'directory': 'isolation'}]
        plan = {'mode': options.mode, 'samples': options.samples, 'warmup': options.warmup, 'cases': options.cases, 'host_namespaces': namespaces(), 'blocks': blocks, 'python_runtime': runtime,
                'baseline': report.get('baseline'),
                'source_revisions': {variant: proof['source_revision'] for variant, proof in proofs.items()},
                'payloads': prepare_payloads(root, options.cases, options.samples + options.warmup) if options.mode != 'isolation' else {}}
        save(root / 'plan.json', plan)
        report['plan_sha256'] = digest(root / 'plan.json')
        for block in blocks:
            phase = {**block, 'status': 'running', 'started_at': timestamp()}
            report['blocks'].append(phase)
            save(destination, report)
            arguments = sandbox(root, plan, block, inputs, driver)
            phase['command'] = arguments
            log = root / block['directory'] / 'sandbox.log'
            with log.open('w') as output:
                process = subprocess.Popen(arguments, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    phase['exit_code'] = process.wait(timeout=options.block_timeout)
                except subprocess.TimeoutExpired:
                    phase.update(timed_out=True, exit_code=None)
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
            result = root / block['directory'] / 'block.json'
            phase['result'] = str(result.relative_to(root))
            phase['log'] = str(log.relative_to(root))
            if result.is_file():
                value = json.loads(result.read_text())
                phase['status'] = 'passed' if phase['exit_code'] == 0 and value['status'] == 'passed' else 'failed'
                phase['result_sha256'] = digest(result)
            else:
                phase['status'] = 'failed'
            phase['finished_at'] = timestamp()
            save(destination, report)
            print(f'{block["name"]}: {phase["status"]} ({log})', flush=True)
        require(all(block['status'] == 'passed' for block in report['blocks']), 'At least one native block failed; all planned rounds were retained')
        for name, binary in {**inputs, **({'webkit-driver': driver} if driver else {})}.items():
            require(digest(binary) == report['inputs'][name]['sha256'], 'Input binary changed during capture')
        runtime_after = inspect_python_runtime(runtime['prefix'], root / 'tool-sources' / SCRIPT.name, root / 'runtime-host-after')
        validate_runtime(runtime_after['details'], runtime['details'])
        require(runtime_after == runtime, 'Runtime selection changed during capture')
        require(digest(root / 'python-runtime.json') == report['python_runtime_proof_sha256'], 'Runtime proof changed during capture')
        report['python_runtime_verified_after'] = runtime_after
        require(digest(root / 'plan.json') == report['plan_sha256'], 'Capture plan changed during capture')
        for values in plan['payloads'].values():
            for payload in values:
                require(digest(Path(payload['path'])) == payload['sha256'], 'Fixture payload changed during capture')
        for variant, source in [('old', options.old_proof), ('candidate', options.candidate_proof)]:
            if source and variant in report.get('build_proof_sha256', {}):
                require(digest(Path(source)) == report['build_proof_sha256'][variant], 'Build proof changed during capture')
        require(digest(SCRIPT) == report['tool_sources'][SCRIPT.name] and digest(FIXTURE_HELPER) == report['tool_sources'][FIXTURE_HELPER.name], 'Measurement tool changed during capture')
        for source in (SCRIPT, FIXTURE_HELPER):
            require(digest(root / 'tool-sources' / source.name) == report['tool_sources'][source.name], 'Measurement tool snapshot changed during capture')
        report.update(status='passed', timing_eligible=options.mode == 'formal', desktop_capture_verified=options.mode != 'isolation')
        if options.mode == 'formal':
            comparisons = []
            for rows in options.rows:
                for case in options.cases:
                    samples = {'old': [], 'candidate': []}
                    writers = {'old': [], 'candidate': []}
                    for block in report['blocks']:
                        if block['rows'] == rows:
                            value = json.loads((root / block['result']).read_text())
                            samples[block['variant']].extend(item['event_elapsed_ms'] for item in value['samples'] if item['case'] == case and not item['warmup'])
                            writers[block['variant']].extend(item['writer_elapsed_ms'] for item in value['samples'] if item['case'] == case and not item['warmup'])
                    old, candidate = percentile(samples['old'], .95), percentile(samples['candidate'], .95)
                    limit = old + max(old * .1, 20)
                    comparisons.append({'rows': rows, 'case': case, 'old_samples': len(samples['old']), 'candidate_samples': len(samples['candidate']), 'old_p50_ms': percentile(samples['old'], .5), 'candidate_p50_ms': percentile(samples['candidate'], .5), 'old_p95_ms': old, 'candidate_p95_ms': candidate,
                                        'old_writer_p95_ms': percentile(writers['old'], .95), 'candidate_writer_p95_ms': percentile(writers['candidate'], .95), 'limit_ms': limit, 'passed': candidate <= limit})
            report['comparisons'] = comparisons
            require(all(value['passed'] for value in comparisons), 'Complete native capture p95 exceeded the agreed threshold')
    except BaseException as error:
        report.update(status='failed', timing_eligible=False, failure={'type': type(error).__name__, 'message': str(error)})
        raise
    finally:
        report['finished_at'] = timestamp()
        report['host']['loadavg_at_end'] = list(os.getloadavg())
        save(destination, report)
        print(f'Native capture {report["status"]}: {destination}', flush=True)


def main():
    if len(sys.argv) == 2 and sys.argv[1] == '--runtime-probe':
        print(json.dumps(runtime_details()))
        return
    if os.environ.get('DEZIRCLIP_NATIVE_CAPTURE_INSIDE') == '1':
        require(len(sys.argv) == 4 and sys.argv[1] == '--inside', 'Use only the outer isolated runner')
        root = Path(sys.argv[2]).resolve(strict=True)
        validate_root(root)
        plan = json.loads(regular(root / 'plan.json').read_text())
        blocks = [block for block in plan['blocks'] if block['name'] == sys.argv[3]]
        require(len(blocks) == 1, 'Unknown native block')
        inside(root, plan, blocks[0])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=('isolation', 'functional', 'formal'), default='functional')
    parser.add_argument('--baseline-only', action='store_true', help='Functional-only old GUI smoke check using its proven old CLI writer')
    parser.add_argument('--baseline-revision', choices=BASELINES, default='802808b06e6b795064c5065d1d880bae9b193b9a',
                        help='Exact supported source revision; event contract is fixed by this selection, not caller-provided')
    parser.add_argument('--python-runtime', type=Path, help='Standalone Python 3.13 prefix, mounted read-only at its real path; actual SQLite must be >= 3.50.4')
    for name in ('old-gui', 'old-cli', 'candidate-gui', 'writer', 'old-proof', 'candidate-proof', 'webkit-driver', 'quiet-window-note'):
        parser.add_argument('--' + name)
    parser.add_argument('--samples', type=int)
    parser.add_argument('--warmup', type=int)
    parser.add_argument('--rows', type=int, nargs='+', choices=(1000, 10000), default=[1000, 10000])
    parser.add_argument('--cases', nargs='+', choices=CASES, default=list(CASES))
    parser.add_argument('--block-timeout', type=int, default=1200)
    options = parser.parse_args()
    options.samples = options.samples if options.samples is not None else (30 if options.mode == 'formal' else 2)
    options.warmup = options.warmup if options.warmup is not None else (5 if options.mode == 'formal' else 1)
    require(options.block_timeout > 0 and len(set(options.rows)) == len(options.rows) and len(set(options.cases)) == len(options.cases), 'Invalid native capture plan')
    outside(options)


if __name__ == '__main__':
    main()
