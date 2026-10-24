"""Read actual Windows KnownFolders only on a source-bound disposable CI runner."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys

FOLDERS = {
    'roaming': ('3eb685db-65f9-4cf6-a03a-e3ef65729f3d', 'APPDATA'),
    'local': ('f1b32785-6fba-4fcf-9d55-7b8e7f157091', 'LOCALAPPDATA'),
}


def require(value, message):
    if not value:
        raise ValueError(message)


def profile_layout(profile):
    profile = Path(profile).resolve()
    home = profile / 'home'
    local = home / 'AppData/Local'
    return {'USERPROFILE': home, 'HOME': home, 'APPDATA': home / 'AppData/Roaming',
            'LOCALAPPDATA': local, 'LOCALLOW': home / 'AppData/LocalLow',
            'TEMP': local / 'Temp', 'TMP': local / 'Temp'}


def assert_disposable_windows(platform, environment):
    require(platform == 'win32' and environment.get('GITHUB_ACTIONS') == 'true'
            and environment.get('RUNNER_ENVIRONMENT') == 'github-hosted'
            and environment.get('GITHUB_REPOSITORY') == 'qiyuanhuakai/dezirclip'
            and environment.get('RUNNER_TEMP'), 'KnownFolder diagnostics require the actual disposable Windows runner')


def owned(path, environment):
    path = Path(path)
    boundary = Path(environment['RUNNER_TEMP']).resolve(strict=True)
    require(path.is_absolute(), 'Absolute owned diagnostic paths required')
    actual = path.resolve()
    require(actual != boundary and actual.is_relative_to(boundary), 'Diagnostic path escapes RUNNER_TEMP')
    current = path
    while current != boundary:
        require(not current.is_symlink() and not current.is_junction(), 'Diagnostic path crosses a link')
        parent = current.parent
        require(parent != current, 'Diagnostic path has no owned ancestor')
        current = parent
    return actual


def assess_folder(name, observation, expected, profile):
    value = dict(observation)
    expected = Path(expected).resolve()
    value.update(name=name, expected_path=str(expected), exists=False, owned=False, matches_expected=False, status='failed')
    raw = value.get('runtime_path')
    if value.get('hresult') == 0 and isinstance(raw, str) and raw:
        path = Path(raw)
        actual = path.resolve()
        value.update(exists=path.is_dir(), owned=actual.is_relative_to(Path(profile).resolve()),
                     matches_expected=actual == expected)
        if value['exists'] and value['owned'] and value['matches_expected']:
            value['status'] = 'passed'
    return value


def query_known_folder(identifier):
    assert_disposable_windows(sys.platform, os.environ)
    import ctypes
    from uuid import UUID

    class GUID(ctypes.Structure):
        _fields_ = [('data1', ctypes.c_uint32), ('data2', ctypes.c_uint16),
                    ('data3', ctypes.c_uint16), ('data4', ctypes.c_ubyte * 8)]

    shell = ctypes.WinDLL('shell32', use_last_error=True)
    ole = ctypes.WinDLL('ole32', use_last_error=True)
    get_folder = shell.SHGetKnownFolderPath
    get_folder.argtypes = [ctypes.POINTER(GUID), ctypes.c_uint32, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
    get_folder.restype = ctypes.c_long
    release = ole.CoTaskMemFree
    release.argtypes = [ctypes.c_void_p]
    release.restype = None
    value = GUID.from_buffer_copy(UUID(identifier).bytes_le)
    pointer = ctypes.c_void_p()
    result = int(get_folder(ctypes.byref(value), 0, None, ctypes.byref(pointer)))
    try:
        runtime_path = ctypes.wstring_at(pointer.value) if result == 0 and pointer.value else None
    finally:
        if pointer.value:
            release(pointer)
    return {'hresult': result, 'hresult_hex': f'0x{result & 0xffffffff:08x}', 'flags': 0, 'runtime_path': runtime_path}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True)
    parser.add_argument('--profile', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert_disposable_windows(sys.platform, os.environ)
    profile = owned(args.profile, os.environ)
    output = owned(args.output, os.environ)
    require(profile.is_dir() and not output.exists(), 'A private profile and fresh diagnostics file are required')
    expected = profile_layout(profile)
    record = {'schema_version': 1, 'kind': 'windows_known_folder_preflight_not_build_identity', 'status': 'failed',
              'source_revision': args.source, 'started_at': datetime.now(timezone.utc).isoformat(),
              'profile': str(profile), 'environment': {key: os.environ.get(key) for key in expected if key != 'LOCALLOW'},
              'folders': [], 'registry_modified': False, 'directory_contents_read': False, 'product_executions': 0}
    failure = None
    try:
        from release_recovery import authority
        provenance = authority(args.source)
        record['source_authority'] = provenance
        record['workflow_source_revision'] = provenance['workflow_source_revision']
        for key, path in expected.items():
            if key != 'LOCALLOW':
                require(os.environ.get(key) and Path(os.environ[key]).resolve() == path, 'Private path environment differs: ' + key)
            require(path.is_dir(), 'Canonical private path is missing: ' + key)
        for name, (identifier, key) in FOLDERS.items():
            record['folders'].append(assess_folder(name, query_known_folder(identifier), expected[key], profile))
        require(all(item['status'] == 'passed' for item in record['folders']), 'Runtime KnownFolder paths do not match the private fixture')
        record['status'] = 'passed'
    except BaseException as error:
        record['error'] = {'type': type(error).__name__, 'message': str(error)[:2048]}
        failure = error
    finally:
        record['completed_at'] = datetime.now(timezone.utc).isoformat()
        with output.open('xb') as stream:
            stream.write((json.dumps(record, indent=2) + '\n').encode('utf-8'))
            stream.flush()
            os.fsync(stream.fileno())
        print(json.dumps(record))
    if failure is not None:
        raise failure


if __name__ == '__main__':
    main()
