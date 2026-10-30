"""Opt-in hosted Windows postmortem collection; imports and parser are platform independent."""
import argparse
import ctypes
import datetime as dt
import hashlib
import json
import ntpath
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import time
import uuid


REPOSITORY = Path(__file__).resolve().parents[2]
WER_BASE = r'SOFTWARE\Microsoft\Windows\Windows Error Reporting'
APP_KEY = 'dezirclip.exe'
MAX_DUMP = 1024 * 1024 * 1024
MAX_METADATA_READ = 4 * 1024 * 1024
ROOT_PATTERN = r'dezirclip-crash-diagnostics-[a-f0-9]{32}'
FUNCTIONAL_PATTERN = r'dezirclip-surface-[a-f0-9]{32}'
LIMITATIONS = [
    'Diagnostic-only run of the original thirteen release functional checks; no ABBA or resource measurement.',
    'WER may change post-crash termination timing. The original functional failure is retained unchanged.',
    'MINIDUMP_MISC_INFO process creation is in whole Unix seconds; the original retained native handle provides the precise FILETIME.',
    'The dump PID, creation second, full executable module path, time window and actual candidate hash must all bind to the original report.',
    'The release profile and symbol stripping are unchanged. A dump does not guarantee a symbolic stack or the corruption origin.',
    'Normal finally and an always cleanup step restore only this run\'s matching child key; forced runner loss cannot guarantee execution of cleanup.',
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def utc_now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def unix_seconds(value):
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(parsed.tzinfo is not None, 'Timestamp must have an explicit timezone')
    return int(parsed.timestamp())


def windows_path(value):
    require(isinstance(value, str) and '\x00' not in value, 'Invalid Windows path')
    for prefix in ('\\\\?\\', '\\??\\'):
        if value.startswith(prefix):
            value = value[len(prefix):]
            break
    require(re.match(r'^[A-Za-z]:[\\/]', value) is not None, 'An absolute local Windows path is required')
    return ntpath.normcase(ntpath.normpath(value))


def plain_path(path, parent=None, directory=False):
    path = Path(os.path.abspath(path))
    if parent is not None:
        parent = Path(os.path.abspath(parent))
        require(path != parent and path.is_relative_to(parent), 'Path escaped its owned root')
    for component in (path, *path.parents):
        if not component.exists():
            continue
        metadata = component.lstat()
        require(not stat.S_ISLNK(metadata.st_mode) and not getattr(metadata, 'st_file_attributes', 0) & 0x400,
                'Reparse points and symbolic links are not accepted')
    metadata = path.stat()
    require(stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode), 'Unexpected file type')
    return path


def digest_file(path, limit=MAX_DUMP):
    path = plain_path(path)
    before = path.stat()
    require(0 < before.st_size <= limit, 'File is empty or exceeds the collection limit')
    digest = hashlib.sha256()
    total = 0
    with path.open('rb') as stream:
        while chunk := stream.read(65536):
            total += len(chunk)
            require(total <= limit, 'File grew past its limit')
            digest.update(chunk)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns, before.st_ino) == (after.st_size, after.st_mtime_ns, after.st_ino),
            'File changed during hashing')
    return {'bytes': total, 'sha256': digest.hexdigest()}


class DumpReader:
    def __init__(self, stream, size):
        require(32 <= size <= MAX_DUMP, 'Invalid dump size')
        self.stream, self.size, self.read_bytes = stream, size, 0

    def read(self, offset, length):
        require(isinstance(offset, int) and isinstance(length, int) and 0 <= offset <= self.size
                and 0 <= length <= self.size - offset, 'Dump range is outside the file')
        self.read_bytes += length
        require(self.read_bytes <= MAX_METADATA_READ, 'Dump metadata read budget exceeded')
        self.stream.seek(offset)
        result = self.stream.read(length)
        require(len(result) == length, 'Truncated dump metadata')
        return result


def parse_minidump(stream, size):
    reader = DumpReader(stream, size)
    header = reader.read(0, 32)
    signature, version, count, directory, _, timestamp, flags = struct.unpack('<IIIIIIQ', header)
    require(signature == 0x504d444d and version & 0xffff == 0xa793, 'Unrecognized minidump header')
    require(flags & 2, 'The dump is not a full-memory dump')
    require(0 < count <= 256, 'Invalid dump stream count')
    entries = reader.read(directory, count * 12)
    streams = {}
    for index in range(count):
        kind, length, offset = struct.unpack_from('<III', entries, index * 12)
        require(0 <= offset <= size and length <= size - offset, 'Stream is outside the dump')
        if kind in (4, 6, 15):
            require(kind not in streams, 'Duplicate identity or exception stream')
            streams[kind] = (offset, length)
    require(15 in streams and 4 in streams, 'Missing process or module identity stream')
    misc_offset, misc_size = streams[15]
    require(misc_size >= 24, 'Truncated process identity')
    info_size, info_flags, pid, created, _, _ = struct.unpack('<IIIIII', reader.read(misc_offset, 24))
    require(24 <= info_size <= misc_size and info_flags & 3 == 3 and pid > 0,
            'Dump lacks valid process ID or process creation time')
    modules_offset, modules_size = streams[4]
    require(modules_size >= 4, 'Truncated module list')
    modules_count = struct.unpack('<I', reader.read(modules_offset, 4))[0]
    require(0 < modules_count <= 1024 and modules_size >= 4 + modules_count * 108, 'Invalid module count')
    module_bytes = reader.read(modules_offset + 4, modules_count * 108)
    executable_modules = []
    for index in range(modules_count):
        address, image_size, checksum, image_time, name_offset = struct.unpack_from('<QIIII', module_bytes, index * 108)
        name_size = struct.unpack('<I', reader.read(name_offset, 4))[0]
        require(0 < name_size <= 32768 and name_size % 2 == 0, 'Invalid module name length')
        name = reader.read(name_offset + 4, name_size).decode('utf-16-le', errors='strict')
        require('\x00' not in name, 'Embedded NUL in module name')
        if ntpath.basename(name).casefold() == APP_KEY:
            executable_modules.append({'path': name, 'base_address': address, 'image_size': image_size,
                                       'checksum': checksum, 'timestamp': image_time})
    exception = None
    if 6 in streams:
        offset, length = streams[6]
        require(length >= 168, 'Truncated exception stream')
        data = reader.read(offset, 168)
        thread_id, _, code, exception_flags = struct.unpack_from('<IIII', data)
        address = struct.unpack_from('<Q', data, 24)[0]
        exception = {'thread_id': thread_id, 'code': f'0x{code:08x}', 'flags': exception_flags, 'address': address}
    return {'pid': pid, 'process_creation_unix_seconds': created, 'dump_timestamp_unix_seconds': timestamp,
            'full_memory': True, 'executable_modules': executable_modules, 'exception': exception,
            'metadata_bytes_read': reader.read_bytes, 'file_bytes': size}


def bind_dump(parsed, report, process_record, candidate, functional_root):
    original = report['desktop_original']
    require(original.get('original_handle_retained') is True, 'Original native handle was not retained')
    require(isinstance(original.get('pid'), int) and original['pid'] > 0, 'Invalid original PID')
    require(str(original.get('creation_time_100ns', '')).isdigit(), 'Missing original FILETIME')
    created = (int(original['creation_time_100ns']) - 116444736000000000) // 10000000
    expected = functional_root / 'candidate' / APP_KEY
    require(windows_path(original['image']) == windows_path(str(expected)), 'Original image is outside the exact candidate path')
    require(process_record['pid'] == original['pid'] and windows_path(process_record['executable']) == windows_path(original['image']),
            'Original process receipt disagrees with its native identity')
    require(parsed['pid'] == original['pid'] and parsed['process_creation_unix_seconds'] == created,
            'Dump belongs to another process incarnation')
    require(created <= parsed['dump_timestamp_unix_seconds'] <= unix_seconds(process_record['closed_at']) + 30,
            'Dump timestamp is outside the original process interval')
    require(abs(unix_seconds(process_record['started_at']) - created) <= 2, 'Original native/process start receipts disagree')
    modules = parsed['executable_modules']
    require(len(modules) == 1 and windows_path(modules[0]['path']) == windows_path(original['image']),
            'Dump main executable module is not the exact original candidate')
    copies = [item for item in report['input_bytes'] if windows_path(item['path']) == windows_path(original['image'])]
    require(len(copies) == 1 and candidate == {'bytes': copies[0]['bytes'], 'sha256': copies[0]['sha256']},
            'Actual candidate hash does not match the functional input receipt')
    require(report.get('input_integrity_after', {}).get('unchanged') is True, 'Functional input integrity did not pass')
    return {'original_pid': original['pid'], 'original_creation_time_100ns': original['creation_time_100ns'],
            'dump_creation_precision': 'whole_unix_seconds', 'candidate': candidate, 'module_path': modules[0]['path'],
            'dump_timestamp_unix_seconds': parsed['dump_timestamp_unix_seconds'], 'exception': parsed['exception']}


def json_value(value):
    if isinstance(value, bytes):
        return {'binary_hex': value.hex()}
    return value


def values_hash(values):
    data = json.dumps(values, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii')
    return hashlib.sha256(data).hexdigest()


def cleanup_matches(snapshot, journal):
    return journal.get('created_child') is True and snapshot == journal.get('child_snapshot')


def reproduction_status(report):
    verdict = report.get('desktop_exit_verdict') or {}
    if verdict.get('abnormal_before_cleanup') is True:
        return 'original_abnormal_exit_observed'
    if report.get('status') == 'passed' and verdict.get('allowed') is True:
        return 'not_reproduced'
    return 'functional_failure_without_confirmed_original_abnormal_exit'

def context(env, platform=sys.platform):
    require(platform == 'win32' and struct.calcsize('P') == 8, 'This collector only operates on 64-bit Windows')
    for key, expected in {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_ARCH': 'X64',
                          'GITHUB_REPOSITORY': 'qiyuanhuakai/dezirclip', 'GITHUB_EVENT_NAME': 'workflow_dispatch',
                          'DZC_WINDOWS_CRASH_DIAGNOSTICS_ENABLED': 'true'}.items():
        require(env.get(key) == expected, 'Diagnostics is restricted to explicit disposable hosted-runner dispatches: ' + key)
    require(env.get('DZC_WINDOWS_PERFORMANCE_ENABLED') != 'true', 'Diagnostic instrumentation cannot run an ABBA comparison')
    require(re.fullmatch('[a-f0-9]{40}', env.get('GITHUB_SHA', '')) is not None, 'Invalid source revision')
    for name in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'):
        require(re.fullmatch('[1-9][0-9]*', env.get(name, '')) is not None, 'Invalid workflow identity')
    return {'source_revision': env['GITHUB_SHA'], 'run_id': env['GITHUB_RUN_ID'], 'run_attempt': env['GITHUB_RUN_ATTEMPT']}


def write_json(path, value, replace=False):
    path = Path(path)
    destination = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp') if replace else path
    with destination.open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, indent=2, ensure_ascii=True)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    if replace:
        plain_path(path)
        os.replace(destination, path)


def read_json(path, limit=4 * 1024 * 1024):
    path = plain_path(path)
    require(path.stat().st_size <= limit, 'JSON receipt exceeds its limit')
    return json.loads(path.read_text(encoding='utf-8'))


def registry_snapshot(key, winreg):
    subkeys, count, written = winreg.QueryInfoKey(key)
    values = []
    for index in range(count):
        name, value, kind = winreg.EnumValue(key, index)
        values.append([name, json_value(value), kind])
    return {'subkeys': subkeys, 'values': sorted(values), 'last_write_100ns': str(written)}


def create_registry_child(parent, name, winreg):
    from ctypes import wintypes
    api = ctypes.WinDLL('advapi32', use_last_error=True).RegCreateKeyExW
    api.argtypes = [wintypes.HKEY, wintypes.LPCWSTR, wintypes.DWORD, wintypes.LPWSTR, wintypes.DWORD,
                    wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.HKEY), ctypes.POINTER(wintypes.DWORD)]
    api.restype = wintypes.LONG
    handle, disposition = wintypes.HKEY(), wintypes.DWORD()
    status = api(int(parent), name, 0, None, 0, winreg.KEY_READ | winreg.KEY_WRITE | winreg.KEY_WOW64_64KEY,
                 None, ctypes.byref(handle), ctypes.byref(disposition))
    if status:
        raise OSError(status, 'Cannot create WER child key')
    return winreg.HKEYType(handle.value), disposition.value == 1


def configure_wer(root, journal):
    import winreg
    require(ctypes.windll.shell32.IsUserAnAdmin() != 0, 'WER collection requires an administrator runner')
    tasklist = Path(os.environ['SystemRoot']) / 'System32' / 'tasklist.exe'
    baseline = subprocess.run([str(tasklist), '/FI', 'IMAGENAME eq dezirclip.exe', '/FO', 'CSV', '/NH'],
                              capture_output=True, timeout=10, check=True)
    require(len(baseline.stdout) <= 65536 and b'"dezirclip.exe"' not in baseline.stdout.lower(),
            'An unrelated DezirClip process already exists on this runner')
    read = winreg.KEY_READ | winreg.KEY_WOW64_64KEY
    for key_name in (r'SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\dezirclip.exe',):
        try:
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key_name, 0, read):
                raise ValueError('Existing per-image debugger configuration is not modified')
        except FileNotFoundError:
            pass
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r'SOFTWARE\Microsoft\Windows NT\CurrentVersion\AeDebug', 0, read) as key:
            try:
                automatic, _ = winreg.QueryValueEx(key, 'Auto')
            except FileNotFoundError:
                automatic = None
            require(str(automatic) != '1', 'Automatic postmortem debugging prevents WER local dump collection')
    except FileNotFoundError:
        pass
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, WER_BASE, 0, read | winreg.KEY_WRITE) as base:
        parent, created = create_registry_child(base, 'LocalDumps', winreg)
        with parent:
            journal['created_parent'] = created
            journal['parent_values_sha256'] = values_hash(registry_snapshot(parent, winreg)['values'])
            child, created = create_registry_child(parent, APP_KEY, winreg)
            with child:
                require(created, 'Preserve the pre-existing per-app WER key')
                journal['created_child'] = True
                journal['child_snapshot'] = registry_snapshot(child, winreg)
                write_json(root / 'registry-journal.json', journal, replace=True)
                expected = [('DumpFolder', str(root / 'raw-dumps'), winreg.REG_EXPAND_SZ),
                            ('DumpCount', 1, winreg.REG_DWORD), ('DumpType', 2, winreg.REG_DWORD)]
                for name, value, kind in expected:
                    winreg.SetValueEx(child, name, 0, kind, value)
                    journal['child_snapshot'] = registry_snapshot(child, winreg)
                    write_json(root / 'registry-journal.json', journal, replace=True)
                require(journal['child_snapshot']['subkeys'] == 0 and journal['child_snapshot']['values'] == sorted([list(item) for item in expected]),
                        'WER configuration readback differs from the owned values')


def restore_wer(root, expected_context):
    import winreg
    journal = read_json(root / 'registry-journal.json')
    require(journal['workflow'] == expected_context and journal['root'] == str(root), 'Cleanup journal belongs to another dispatch')
    if journal.get('cleanup', {}).get('status') == 'restored':
        return journal['cleanup']
    if not journal.get('created_child'):
        return {'status': 'nothing_created', 'created_parent': journal.get('created_parent', False)}
    access = winreg.KEY_READ | winreg.KEY_WRITE | winreg.KEY_WOW64_64KEY
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, WER_BASE + r'\LocalDumps', 0, access) as parent:
        require(values_hash(registry_snapshot(parent, winreg)['values']) == journal['parent_values_sha256'],
                'Global WER values changed; do not modify the registry')
        with winreg.OpenKey(parent, APP_KEY, 0, access) as child:
            snapshot = registry_snapshot(child, winreg)
            require(cleanup_matches(snapshot, journal), 'Owned WER child changed; refuse to delete another configuration')
        winreg.DeleteKeyEx(parent, APP_KEY, winreg.KEY_WOW64_64KEY, 0)
        try:
            with winreg.OpenKey(parent, APP_KEY, 0, winreg.KEY_READ):
                raise ValueError('Owned child key still exists after cleanup')
        except FileNotFoundError:
            pass
        require(values_hash(registry_snapshot(parent, winreg)['values']) == journal['parent_values_sha256'],
                'Global WER values changed during cleanup')
    journal['cleanup'] = {'status': 'restored', 'finished_at': utc_now(),
                          'created_parent': journal['created_parent'], 'parent_retained': True}
    write_json(root / 'registry-journal.json', journal, replace=True)
    return journal['cleanup']


def private_directory(path):
    path.mkdir()
    # ACL changes apply only to the new synthetic diagnostics directory.
    who = subprocess.run(['whoami.exe', '/user', '/fo', 'csv', '/nh'], capture_output=True, timeout=10, check=True)
    found = re.findall(rb'S-1-5-[0-9-]+', who.stdout)
    require(len(found) == 1, 'Cannot identify this runner account')
    sid = found[0].decode('ascii')
    subprocess.run(['icacls.exe', str(path), '/inheritance:r', '/grant:r', '*' + sid + ':(OI)(CI)F',
                    '*S-1-5-18:(OI)(CI)F', '*S-1-5-32-544:(OI)(CI)F'], check=True, timeout=10)
    return sid


def owned_root(value):
    temporary = plain_path(os.environ['RUNNER_TEMP'], directory=True)
    root = plain_path(value, temporary, directory=True)
    require(root.parent == temporary and re.fullmatch(ROOT_PATTERN, root.name), 'Unexpected diagnostic root')
    return root


def tools_inventory():
    result = []
    sdk = Path(r'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64')
    for name in ('cdb.exe', 'gflags.exe'):
        path = sdk / name
        item = {'path': str(path), 'exists': path.exists(), 'executed_for_inventory': False}
        if item['exists']:
            item.update(digest_file(path, 256 * 1024 * 1024))
        result.append(item)
    return result


def collect_dump(root, functional, workflow):
    report_path, process_path = functional / 'report.json', functional / 'desktop.process.json'
    report, process_record = read_json(report_path), read_json(process_path)
    require(report['source_revision'] == workflow['source_revision'] and report['run_id'] == workflow['run_id']
            and report['run_attempt'] == workflow['run_attempt'], 'Functional report belongs to another checkout or run')
    candidate = digest_file(plain_path(functional / 'candidate' / APP_KEY, functional), 256 * 1024 * 1024)
    raw = plain_path(root / 'raw-dumps', root, directory=True)
    entries = list(raw.iterdir())
    require(len(entries) <= 8, 'Unexpected files in the private WER directory')
    manifest = {'functional_report': {'path': str(report_path), **digest_file(report_path, MAX_METADATA_READ)},
                'original_functional_status': report['status'], 'original_functional_error': report.get('error'),
                'original_exit_verdict': report.get('desktop_exit_verdict'),
                'reproduction_status': reproduction_status(report),
                'original_process_receipt': {'path': str(process_path), **digest_file(process_path, MAX_METADATA_READ)},
                'candidates': [], 'accepted': None}
    for path in entries:
        item = {'name': path.name, 'accepted': False}
        try:
            require(re.fullmatch(r'dezirclip\.exe\.[1-9][0-9]*\.dmp', path.name, re.IGNORECASE) is not None,
                    'Unknown dump filename')
            plain_path(path, raw)
            item.update(digest_file(path))
            with path.open('rb') as stream:
                parsed = parse_minidump(stream, item['bytes'])
            item['parsed'] = parsed
            binding = bind_dump(parsed, report, process_record, candidate, functional)
            require(manifest['accepted'] is None, 'More than one dump claims the original process')
            require(digest_file(path) == {'bytes': item['bytes'], 'sha256': item['sha256']}, 'Dump changed while checking identity')
            verified = root / 'verified'
            verified.mkdir(exist_ok=True)
            plain_path(verified, root, directory=True)
            destination = verified / 'original-desktop.dmp'
            with path.open('rb') as source, destination.open('xb') as target:
                while chunk := source.read(65536):
                    target.write(chunk)
            require(digest_file(destination) == {'bytes': item['bytes'], 'sha256': item['sha256']}, 'Verified dump copy differs')
            item['accepted'] = True
            manifest['accepted'] = {'path': str(destination), 'bytes': item['bytes'], 'sha256': item['sha256'], 'binding': binding}
        except (ValueError, OSError, UnicodeError, KeyError) as error:
            item['rejection'] = str(error)
        manifest['candidates'].append(item)
    manifest['dump_status'] = 'identity_bound' if manifest['accepted'] else 'unavailable_or_unbound'
    return manifest


def emit_output(name, value):
    require('\r' not in value and '\n' not in value, 'Invalid workflow output')
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
        stream.write(name + '=' + value + '\n')


def run(args, workflow):
    revision = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=REPOSITORY, capture_output=True, text=True, check=True).stdout.strip()
    require(revision == workflow['source_revision'], 'Diagnostics must use this workflow\'s exact checkout')
    temporary = plain_path(os.environ['RUNNER_TEMP'], directory=True)
    nonce = uuid.uuid4().hex
    root, functional = temporary / ('dezirclip-crash-diagnostics-' + nonce), temporary / ('dezirclip-surface-' + nonce)
    require(not root.exists() and not functional.exists(), 'Preserve existing diagnostics and functional evidence')
    sid = private_directory(root)
    plain_path(root, temporary, directory=True)
    (root / 'raw-dumps').mkdir()
    emit_output('root', str(root))
    emit_output('functional_root', str(functional))
    journal = {'schema_version': 1, 'workflow': workflow, 'root': str(root), 'created_child': False, 'created_parent': False}
    write_json(root / 'registry-journal.json', journal)
    result = {'schema_version': 1, 'kind': 'windows_release_crash_diagnostics', 'workflow': workflow,
              'started_at': utc_now(), 'limits': LIMITATIONS, 'root': str(root), 'runner_sid': sid,
              'functional_root': str(functional), 'functional_exit_code': None, 'cleanup': None}
    exit_code = 1
    try:
        result['tools'] = tools_inventory()
        configure_wer(root, journal)
        command = ['node', str(REPOSITORY / 'scripts/tests/native-windows-surface.mjs'), '--desktop', args.desktop,
                   '--helper', args.helper, '--helper-proof', args.helper_proof, '--python', sys.executable,
                   '--output-root', str(functional)]
        result['functional_command'] = command
        result['functional_exit_code'] = subprocess.run(command, cwd=REPOSITORY, check=False).returncode
        # The original controller retains all of its original deadlines and cleanup behavior.
        exit_code = result['functional_exit_code']
        result['collection'] = collect_dump(root, functional, workflow)
        if result['collection']['accepted']:
            emit_output('accepted_dump', result['collection']['accepted']['path'])
        if exit_code == 0 and result['collection']['accepted']:
            exit_code = 1
    except BaseException as error:
        result['diagnostics_error'] = str(error)
        if exit_code == 0:
            exit_code = 1
    finally:
        try:
            result['cleanup'] = restore_wer(root, workflow)
        except BaseException as error:
            result['cleanup'] = {'status': 'failed', 'error': str(error)}
            if exit_code == 0:
                exit_code = 1
        result['finished_at'] = utc_now()
        result['exit_code'] = exit_code
        write_json(root / 'diagnostics.json', result)
    return exit_code


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=('run', 'cleanup'))
    parser.add_argument('--desktop')
    parser.add_argument('--helper')
    parser.add_argument('--helper-proof')
    parser.add_argument('--root')
    args = parser.parse_args()
    workflow = context(os.environ)
    if args.mode == 'cleanup':
        root = owned_root(args.root)
        result = restore_wer(root, workflow)
        write_json(root / 'always-cleanup.json', result)
        return 0
    require(all((args.desktop, args.helper, args.helper_proof)), 'Release binaries and matching proof are required')
    return run(args, workflow)


if __name__ == '__main__':
    sys.exit(main())
