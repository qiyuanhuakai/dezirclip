"""Temporary per-app WebView2 policy for disposable alpha.2 release QA only."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

from release_recovery import SOURCE, authority

ROOT = Path(__file__).resolve().parents[1]
PREFIX = r'SOFTWARE\Policies\Microsoft\Edge\WebView2'
KEYS = (PREFIX + r'\AdditionalBrowserArguments', PREFIX + r'\UserDataFolder')
PARENTS = (r'SOFTWARE\Policies\Microsoft\Edge', PREFIX, *KEYS)
NAMES = ('dezirclip.exe', 'io.github.qiyuanhuakai.dezirclip')
VIEWS = (64, 32)


def require(value, message):
    if not value:
        raise ValueError(message)


def owned(path, root, *, exists=False):
    path = Path(path).absolute()
    boundary = Path(root).resolve(strict=True)
    require(path != boundary and path.resolve().is_relative_to(boundary), 'Release QA path must stay inside its owned root')
    for item in (path, *path.parents):
        if item.exists() or item.is_symlink():
            require(not item.is_symlink() and not item.is_junction(), 'Release QA path cannot cross a link or junction')
        if item == boundary:
            break
    if exists:
        require(path.exists(), 'Existing owned release QA input required')
    return path


def read_json(path):
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and not path.is_symlink() and 0 < before.st_size <= 8 * 1024 * 1024,
            'Bounded regular release QA state required')
    with path.open('rb') as stream:
        opened = os.fstat(stream.fileno())
        require((opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino), 'Release QA state changed before opening')
        data = stream.read(8 * 1024 * 1024 + 1)
        after = os.fstat(stream.fileno())
    require((opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns) ==
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) and len(data) == before.st_size,
            'Release QA state changed while reading')
    return json.loads(data), hashlib.sha256(data).hexdigest()


def save(path, value):
    with path.open('xb') as stream:
        stream.write((json.dumps(value, indent=2) + '\n').encode('utf-8'))
        stream.flush()
        os.fsync(stream.fileno())


class Events:
    def __init__(self, path, *, append=False):
        if append and path.exists():
            info = path.lstat()
            require(stat.S_ISREG(info.st_mode) and not path.is_symlink() and info.st_size <= 8 * 1024 * 1024,
                    'Original bounded restoration journal required')
            self.stream = path.open('ab')
            opened = os.fstat(self.stream.fileno())
            require((opened.st_dev, opened.st_ino) == (info.st_dev, info.st_ino), 'Restoration journal changed before opening')
        else:
            self.stream = path.open('xb')

    def add(self, **value):
        self.stream.write((json.dumps(value) + '\n').encode('utf-8'))
        self.stream.flush()
        os.fsync(self.stream.fileno())

    def close(self):
        self.stream.close()


def events(path):
    if not path.exists():
        return []
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and not path.is_symlink() and before.st_size <= 8 * 1024 * 1024,
            'Bounded regular configuration journal required')
    result = []
    for index, line in enumerate(path.read_bytes().splitlines(keepends=True)):
        try:
            require(line.endswith(b'\n'), 'Incomplete original journal line')
            value = json.loads(line)
            require(isinstance(value, dict), 'Original journal record must be an object')
            result.append(value)
        except (ValueError, UnicodeDecodeError):
            result.append({'event': 'incomplete_configuration_journal', 'line': index,
                           'original_line_sha256': hashlib.sha256(line).hexdigest()})
    return result


def packed(value):
    if isinstance(value, bytes):
        return {'binary_base64': base64.b64encode(value).decode('ascii')}
    require(value is None or isinstance(value, (str, int)) or
            isinstance(value, list) and all(isinstance(item, str) for item in value), 'Unsupported exact registry snapshot type')
    return {'value': value}


def unpacked(value):
    return base64.b64decode(value['binary_base64'], validate=True) if 'binary_base64' in value else value['value']


def validate_document(value, root, context):
    require(type(value.get('schema_version')) is int and value['schema_version'] == 1
            and value.get('kind') == 'release_qa_webview_policy_snapshot_not_build_identity'
            and value['source_authority'] == context and Path(value['root']) == root
            and value['product_acceptance_claimed'] is False, 'Original same-run WebView policy snapshot required')
    profile = owned(Path(value['profile']), root, exists=True)
    require(type(value['port']) is int and 1024 <= value['port'] <= 65535
            and type(value['configure_process']['token_is_elevated']) is int, 'Exact original QA token and port required')
    elevated = value['configure_process']['token_is_elevated'] != 0
    require(value['mode'] == ('elevated_per_app_hklm' if elevated else 'unchanged_environment')
            and value['registry_writes_planned'] is elevated, 'Original policy mode differs from its actual token')
    require(len(value['keys']) == (len(PARENTS) * len(VIEWS) if elevated else 0)
            and {(item['key'], item['view']) for item in value['keys']} ==
                ({(key, view) for key in PARENTS for view in VIEWS} if elevated else set())
            and all(type(item['existed']) is bool for item in value['keys']), 'Only the fixed policy keys and views may be restored')
    expected = {(key, name, view) for key in KEYS for name in NAMES for view in VIEWS} if elevated else set()
    require(len(value['values']) == len(expected) and
            {(item['key'], item['name'], item['view']) for item in value['values']} == expected,
            'Only the original exact per-app policy values may be restored')
    for item in value['values']:
        data = '--remote-debugging-port=' + str(value['port']) if item['key'] == KEYS[0] else str(profile)
        require(item['after'] == {'exists': True, 'type': 1, 'data': packed(data)}, 'Original temporary policy data changed')
        before = item['before']
        require(type(before.get('exists')) is bool, 'Exact registry value presence required')
        if before['exists']:
            require(set(before) == {'exists', 'type', 'data'} and type(before['type']) is int
                    and before['type'] in (0, 1, 2, 3, 4, 7, 11)
                    and packed(unpacked(before['data'])) == before['data'], 'Unsupported original registry value snapshot')
            decoded = unpacked(before['data'])
            kind = before['type']
            require((kind in (1, 2) and isinstance(decoded, str))
                    or (kind in (0, 3) and (isinstance(decoded, bytes) or kind == 0 and decoded is None))
                    or (kind in (4, 11) and type(decoded) is int and 0 <= decoded < 2 ** (32 if kind == 4 else 64))
                    or (kind == 7 and isinstance(decoded, list) and all(isinstance(item, str) for item in decoded)),
                    'Original registry data does not match its exact type')
        else:
            require(before == {'exists': False}, 'Absent registry snapshot must remain exact')
    return value


class NativeWindows:
    def __init__(self):
        import ctypes
        import winreg
        from ctypes import wintypes
        self.ctypes, self.types, self.registry = ctypes, wintypes, winreg
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        self.security = ctypes.WinDLL('advapi32', use_last_error=True)
        self.kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
        self.kernel.CloseHandle.restype = wintypes.BOOL
        self.kernel.QueryFullProcessImageNameW.argtypes = (wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD))
        self.kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL
        self.kernel.GetProcessTimes.argtypes = (wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4))
        self.kernel.GetProcessTimes.restype = wintypes.BOOL
        self.security.OpenProcessToken.argtypes = (wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE))
        self.security.OpenProcessToken.restype = wintypes.BOOL
        self.security.GetTokenInformation.argtypes = (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD))
        self.security.GetTokenInformation.restype = wintypes.BOOL
        self.security.RegCreateKeyExW.argtypes = (wintypes.HANDLE, wintypes.LPCWSTR, wintypes.DWORD, wintypes.LPWSTR,
            wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.HANDLE), ctypes.POINTER(wintypes.DWORD))
        self.security.RegCreateKeyExW.restype = wintypes.LONG
        self.security.RegCloseKey.argtypes = (wintypes.HANDLE,)
        self.security.RegCloseKey.restype = wintypes.LONG

    def token(self, pid):
        c, t = self.ctypes, self.types
        process = self.kernel.OpenProcess(0x1000, False, pid)
        require(process, 'OpenProcess for actual QA token failed')
        token = t.HANDLE()
        try:
            require(self.security.OpenProcessToken(process, 0x0008, c.byref(token)), 'OpenProcessToken(TOKEN_QUERY) failed')
            result = {'pid': pid}
            for name, number in (('token_is_elevated', 20), ('token_elevation_type', 18)):
                value, length = t.DWORD(), t.DWORD()
                require(self.security.GetTokenInformation(token, number, c.byref(value), c.sizeof(value), c.byref(length))
                        and length.value == c.sizeof(value), 'GetTokenInformation actual elevation query failed')
                result[name] = int(value.value)
            buffer, length = c.create_unicode_buffer(32768), t.DWORD(32768)
            require(self.kernel.QueryFullProcessImageNameW(process, 0, buffer, c.byref(length)), 'Actual QA process image query failed')
            times = [t.FILETIME() for _ in range(4)]
            require(self.kernel.GetProcessTimes(process, *(c.byref(value) for value in times)), 'Actual QA process creation time query failed')
            result.update(executable=buffer.value, creation_time_100ns=(times[0].dwHighDateTime << 32) | times[0].dwLowDateTime)
            return result
        finally:
            if token:
                self.kernel.CloseHandle(token)
            self.kernel.CloseHandle(process)

    def access(self, view):
        return self.registry.KEY_WOW64_64KEY if view == 64 else self.registry.KEY_WOW64_32KEY

    def key_exists(self, key, view):
        try:
            with self.registry.OpenKey(self.registry.HKEY_LOCAL_MACHINE, key, 0, self.registry.KEY_READ | self.access(view)):
                return True
        except FileNotFoundError:
            return False

    def read(self, key, name, view):
        try:
            with self.registry.OpenKey(self.registry.HKEY_LOCAL_MACHINE, key, 0, self.registry.KEY_QUERY_VALUE | self.access(view)) as handle:
                value, kind = self.registry.QueryValueEx(handle, name)
                return {'exists': True, 'type': kind, 'data': packed(value)}
        except FileNotFoundError:
            return {'exists': False}

    def create_key(self, key, view):
        c, t = self.ctypes, self.types
        root = t.HANDLE(c.c_ssize_t(c.c_int32(0x80000002).value).value)
        handle, disposition = t.HANDLE(), t.DWORD()
        code = self.security.RegCreateKeyExW(root, key, 0, None, 0,
            self.registry.KEY_READ | self.registry.KEY_WRITE | self.access(view), None, c.byref(handle), c.byref(disposition))
        require(code == 0, 'Create exact per-app policy key failed: ' + str(code))
        try:
            return disposition.value == 1
        finally:
            self.security.RegCloseKey(handle)

    def write(self, key, name, view, snapshot):
        with self.registry.OpenKey(self.registry.HKEY_LOCAL_MACHINE, key, 0, self.registry.KEY_SET_VALUE | self.access(view)) as handle:
            if snapshot['exists']:
                self.registry.SetValueEx(handle, name, 0, snapshot['type'], unpacked(snapshot['data']))
            else:
                self.registry.DeleteValue(handle, name)

    def empty_key(self, key, view):
        with self.registry.OpenKey(self.registry.HKEY_LOCAL_MACHINE, key, 0, self.registry.KEY_READ | self.access(view)) as handle:
            children, values, modified = self.registry.QueryInfoKey(handle)
            return children == 0 and values == 0, modified

    def delete_empty_key(self, key, view, observed):
        require(self.empty_key(key, view) == (True, observed), 'Created policy key changed during restoration')
        self.registry.DeleteKeyEx(self.registry.HKEY_LOCAL_MACHINE, key, self.access(view), 0)


def configure(api, state, root, profile, port, context):
    require(not state.exists() and 1024 <= port <= 65535, 'Fresh QA snapshot and explicit private CDP port required')
    token = api.token(os.getpid())
    elevated = token['token_is_elevated'] != 0
    snapshots, keys = [], []
    if elevated:
        for view in VIEWS:
            require(api.key_exists(r'SOFTWARE\Policies\Microsoft', view), 'Existing system policy parent required')
            keys.extend({'key': key, 'view': view, 'existed': api.key_exists(key, view)} for key in PARENTS)
            for key in KEYS:
                value = '--remote-debugging-port=' + str(port) if key == KEYS[0] else str(profile)
                for name in NAMES:
                    snapshots.append({'key': key, 'name': name, 'view': view, 'before': api.read(key, name, view),
                                      'after': {'exists': True, 'type': 1, 'data': packed(value)}})
    profile.mkdir(parents=True, exist_ok=True)
    document = {'schema_version': 1, 'kind': 'release_qa_webview_policy_snapshot_not_build_identity',
        'source_authority': context, 'root': str(root), 'profile': str(profile), 'port': port,
        'configure_process': token, 'mode': 'elevated_per_app_hklm' if elevated else 'unchanged_environment',
        'keys': keys, 'values': snapshots, 'registry_writes_planned': elevated, 'product_acceptance_claimed': False}
    validate_document(document, root, context)
    save(state, document)
    journal = Events(Path(str(state) + '.configure.log'))
    changed = []
    try:
        journal.add(event='configure_start', snapshot_sha256=hashlib.sha256(state.read_bytes()).hexdigest(), mode=document['mode'])
        if elevated:
            for item in keys:
                if not api.key_exists(item['key'], item['view']):
                    require(not item['existed'], 'Original policy key disappeared before configure')
                    journal.add(event='key_create_intent', **item)
                    actual = api.create_key(item['key'], item['view'])
                    journal.add(event='key_create_result', created=actual, **item)
            for index, item in enumerate(snapshots):
                actual = api.read(item['key'], item['name'], item['view'])
                alias = any(prior['key'] == item['key'] and prior['name'] == item['name'] for prior in changed)
                require(actual == item['before'] or alias and actual == item['after'], 'Policy changed concurrently before configure')
                if actual != item['after']:
                    journal.add(event='value_write_intent', index=index)
                    api.write(item['key'], item['name'], item['view'], item['after'])
                    require(api.read(item['key'], item['name'], item['view']) == item['after'], 'Temporary policy readback differs')
                    changed.append(item)
                    journal.add(event='value_written', index=index)
                else:
                    journal.add(event='value_original_or_alias_already_matches', index=index)
            require(all(api.read(item['key'], item['name'], item['view']) == item['after'] for item in snapshots),
                    'Both registry views must read back the exact per-app policy')
        journal.add(event='configure_complete', status='configured', mode=document['mode'])
        return document
    except BaseException as error:
        journal.add(event='configure_complete', status='failed', error=str(error))
        raise
    finally:
        journal.close()


def inspect(api, state, document, pid, executable):
    require(pid > 0 and executable.name.lower() == 'dezirclip.exe', 'Actual owned Desktop process required')
    result = {'schema_version': 1, 'kind': 'release_qa_actual_desktop_token_not_build_identity',
              'expected_executable': str(executable), 'status': 'failed',
              'source_authority': document['source_authority'], 'snapshot_sha256': hashlib.sha256(state.read_bytes()).hexdigest(),
              'policy_mode': document['mode'], 'product_acceptance_claimed': False}
    try:
        token = api.token(pid)
        result['actual_process'] = token
        require(Path(token['executable']).resolve() == executable.resolve(), 'Actual Desktop process image differs')
        require(bool(token['token_is_elevated']) == bool(document['configure_process']['token_is_elevated']),
                'Actual Desktop elevation differs from the configured QA host')
        result['status'] = 'observed'
        return result
    except BaseException as error:
        result['error'] = str(error)
        raise
    finally:
        save(Path(str(state) + '.inspect.log'), result)


def restore(api, state, document):
    original = events(Path(str(state) + '.configure.log'))
    for item in original:
        if item.get('event') == 'value_write_intent':
            require(type(item.get('index')) is int and 0 <= item['index'] < len(document['values']), 'Original policy journal index differs')
        if item.get('event') == 'key_create_result' and item.get('created') is True:
            require(any(key['key'] == item.get('key') and key['view'] == item.get('view') and key['existed'] is False
                        for key in document['keys']), 'Only this snapshot\'s newly created fixed keys may be removed')
    intents = {item['index'] for item in original if item.get('event') == 'value_write_intent'}
    journal = Events(Path(str(state) + '.restore.log'), append=True)
    failures = ['Original configuration journal has incomplete lines; only confirmed intents are restored'] if any(
        item.get('event') == 'incomplete_configuration_journal' for item in original) else []
    try:
        journal.add(event='restore_start', snapshot_sha256=hashlib.sha256(state.read_bytes()).hexdigest(), mode=document['mode'])
        for index in sorted(intents, reverse=True):
            item = document['values'][index]
            try:
                actual = api.read(item['key'], item['name'], item['view'])
                if actual != item['before']:
                    require(actual == item['after'],
                            'Concurrent policy change preserved; restoration refused')
                    api.write(item['key'], item['name'], item['view'], item['before'])
                require(api.read(item['key'], item['name'], item['view']) == item['before'], 'Original policy readback differs')
                journal.add(event='value_restore', index=index, status='restored_or_unchanged')
            except BaseException as error:
                failures.append(str(error)); journal.add(event='value_restore', index=index, status='failed', error=str(error))
        for index, item in enumerate(document['values']):
            if index in intents:
                continue
            try:
                require(api.read(item['key'], item['name'], item['view']) == item['before'],
                        'Unwritten policy view changed concurrently; preserved')
                journal.add(event='unwritten_value_verify', index=index, status='original')
            except BaseException as error:
                failures.append(str(error)); journal.add(event='unwritten_value_verify', index=index, status='failed', error=str(error))
        created = [item for item in original if item.get('event') == 'key_create_result' and item.get('created') is True]
        for item in reversed(created):
            try:
                if api.key_exists(item['key'], item['view']):
                    empty, modified = api.empty_key(item['key'], item['view'])
                    require(empty, 'Created policy key now has concurrent data; preserved')
                    api.delete_empty_key(item['key'], item['view'], modified)
                journal.add(event='key_restore', key=item['key'], view=item['view'], status='removed_owned_empty_key')
            except BaseException as error:
                failures.append(str(error)); journal.add(event='key_restore', key=item['key'], view=item['view'], status='failed', error=str(error))
        for item in document['keys']:
            try:
                require(api.key_exists(item['key'], item['view']) is item['existed'], 'Original policy key presence changed; preserved')
                journal.add(event='key_presence_verify', key=item['key'], view=item['view'], status='original')
            except BaseException as error:
                failures.append(str(error)); journal.add(event='key_presence_verify', key=item['key'], view=item['view'], status='failed', error=str(error))
        journal.add(event='restore_complete', status='failed' if failures else 'restored', failures=failures)
    finally:
        journal.close()
    require(not failures, 'Release QA WebView policy restoration failed; see original restoration log')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('configure', 'inspect', 'restore'))
    parser.add_argument('--source', required=True)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--state', required=True, type=Path)
    parser.add_argument('--profile', type=Path)
    parser.add_argument('--port', type=int)
    parser.add_argument('--pid', type=int)
    parser.add_argument('--executable', type=Path)
    args = parser.parse_args()
    require(sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
            and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted' and args.source == SOURCE,
            'WebView QA policy requires the actual disposable alpha.2 Windows job')
    context = authority(args.source)
    temporary = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    root = owned(args.root, temporary, exists=True)
    require(root.is_dir(), 'Owned release QA root must be a directory')
    state = owned(args.state, root)
    require(state.parent.is_dir() and state.suffix in ('.json', '.log'), 'Owned JSON or log snapshot path required')
    config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    require(config['identifier'] == NAMES[1], 'Only the original DezirClip application policy names are allowed')
    api = NativeWindows()
    if args.mode == 'configure':
        require(args.profile is not None and args.port is not None, 'Configure needs the explicit private profile and CDP port')
        value = configure(api, state, root, owned(args.profile, root), args.port, context)
        print(json.dumps({'status': 'configured', 'mode': value['mode'], 'snapshot': str(state)}))
    else:
        document, snapshot_sha = read_json(owned(state, root, exists=True))
        validate_document(document, root, context)
        if args.mode == 'inspect':
            require(args.pid is not None and args.executable is not None, 'Inspect needs the actual child PID and executable')
            value = inspect(api, state, document, args.pid, owned(args.executable, root, exists=True))
            print(json.dumps(value))
        else:
            restore(api, state, document)
            print(json.dumps({'status': 'restored', 'snapshot_sha256': snapshot_sha, 'snapshot': str(state)}))


if __name__ == '__main__':
    main()
