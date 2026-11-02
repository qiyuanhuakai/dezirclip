"""Synthetic minidump and authority contracts; no Windows API or product execution."""
import copy
import datetime as dt
import importlib.util
import io
from pathlib import Path, PureWindowsPath
import struct
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('windows_crash_diagnostics', Path(__file__).with_name('windows-crash-diagnostics.py'))
DIAGNOSTICS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DIAGNOSTICS)
CREATED = 1430000000
IMAGE = r'Z:\fixture\dezirclip-surface-0123456789abcdef0123456789abcdef\candidate\dezirclip.exe'


def dump_bytes():
    data = bytearray(768)
    struct.pack_into('<IIIIIIQ', data, 0, 0x504d444d, 0xa793, 3, 32, 0, CREATED + 4, 2)
    for index, entry in enumerate(((15, 24, 80), (4, 112, 128), (6, 168, 512))):
        struct.pack_into('<III', data, 32 + index * 12, *entry)
    struct.pack_into('<IIIIII', data, 80, 24, 3, 1772, CREATED, 0, 0)
    struct.pack_into('<I', data, 128, 1)
    struct.pack_into('<QIIII', data, 132, 0x100000, 4096, 0, CREATED, 256)
    name = IMAGE.encode('utf-16-le')
    struct.pack_into('<I', data, 256, len(name))
    data[260:260 + len(name)] = name
    struct.pack_into('<IIII', data, 512, 77, 0, 0xc0000374, 0)
    struct.pack_into('<Q', data, 536, 0x100123)
    return data


def binding_receipts():
    candidate = {'bytes': 4096, 'sha256': 'a' * 64}
    original = {'pid': 1772, 'image': IMAGE, 'original_handle_retained': True,
                'creation_time_100ns': str(CREATED * 10000000 + 116444736000000000)}
    report = {'desktop_original': original, 'input_bytes': [{'path': IMAGE, **candidate}],
              'input_integrity_after': {'unchanged': True}}
    stamp = lambda seconds: dt.datetime.fromtimestamp(seconds, dt.timezone.utc).isoformat()
    process = {'pid': 1772, 'executable': IMAGE, 'started_at': stamp(CREATED), 'closed_at': stamp(CREATED + 5)}
    root = PureWindowsPath(IMAGE).parent.parent
    return report, process, candidate, root


class MinidumpTests(unittest.TestCase):
    def parse(self, data=None):
        data = dump_bytes() if data is None else data
        return DIAGNOSTICS.parse_minidump(io.BytesIO(data), len(data))

    def test_reads_original_process_module_and_heap_exception(self):
        parsed = self.parse()
        self.assertEqual(parsed['pid'], 1772)
        self.assertEqual(parsed['process_creation_unix_seconds'], CREATED)
        self.assertEqual(parsed['executable_modules'][0]['path'], IMAGE)
        self.assertEqual(parsed['exception'], {'thread_id': 77, 'code': '0xc0000374', 'flags': 0, 'address': 0x100123})
        self.assertLess(parsed['metadata_bytes_read'], 2048)
        self.assertTrue(parsed['full_memory'])

    def test_metadata_reads_are_bounded_even_for_a_large_declared_dump(self):
        parsed = DIAGNOSTICS.parse_minidump(io.BytesIO(dump_bytes()), 512 * 1024 * 1024)
        self.assertLess(parsed['metadata_bytes_read'], 2048)
        reader = DIAGNOSTICS.DumpReader(io.BytesIO(b'x' * 32), 32)
        reader.read_bytes = DIAGNOSTICS.MAX_METADATA_READ
        with self.assertRaisesRegex(ValueError, 'budget'):
            reader.read(0, 1)

    def test_rejects_invalid_header_ranges_and_duplicate_identity_streams(self):
        mutations = [(0, 0), (4, 1), (8, 257), (12, 760), (24, 0), (44, 15), (36, 800)]
        for offset, value in mutations:
            data = dump_bytes()
            struct.pack_into('<I', data, offset, value)
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                self.parse(data)
        for size in (0, 31, DIAGNOSTICS.MAX_DUMP + 1):
            with self.subTest(size=size), self.assertRaises(ValueError):
                DIAGNOSTICS.parse_minidump(io.BytesIO(dump_bytes()), size)

    def test_rejects_missing_precise_identity_fields_and_malformed_module_names(self):
        for offset, value in ((84, 1), (88, 0), (128, 1025), (256, 3), (256, 40000)):
            data = dump_bytes()
            struct.pack_into('<I', data, offset, value)
            with self.subTest(offset=offset, value=value), self.assertRaises(ValueError):
                self.parse(data)
        data = dump_bytes()
        data[260:262] = b'\0\0'
        with self.assertRaisesRegex(ValueError, 'NUL'):
            self.parse(data)

    def test_exact_original_receipts_bind_without_reading_or_executing_a_process(self):
        parsed = self.parse()
        binding = DIAGNOSTICS.bind_dump(parsed, *binding_receipts())
        self.assertEqual(binding['original_pid'], 1772)
        self.assertEqual(binding['dump_creation_precision'], 'whole_unix_seconds')
        self.assertEqual(binding['candidate']['sha256'], 'a' * 64)

    def test_rejects_reused_pid_replaced_path_hash_and_unconfirmed_input_integrity(self):
        parsed = self.parse()
        for field, value in (('pid', 1773), ('process_creation_unix_seconds', CREATED + 1),
                             ('dump_timestamp_unix_seconds', CREATED - 1), ('dump_timestamp_unix_seconds', CREATED + 36)):
            changed = copy.deepcopy(parsed)
            changed[field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                DIAGNOSTICS.bind_dump(changed, *binding_receipts())
        report, process, candidate, root = binding_receipts()
        cases = []
        other_report = copy.deepcopy(report)
        other_report['desktop_original']['original_handle_retained'] = False
        cases.append((other_report, process, candidate, root))
        other_report = copy.deepcopy(report)
        other_report['input_integrity_after']['unchanged'] = False
        cases.append((other_report, process, candidate, root))
        cases.append((report, process, {**candidate, 'sha256': 'b' * 64}, root))
        cases.append((report, {**process, 'executable': r'Z:\unrelated\dezirclip.exe'}, candidate, root))
        other_parsed = copy.deepcopy(parsed)
        other_parsed['executable_modules'][0]['path'] = r'Z:\unrelated\dezirclip.exe'
        with self.assertRaises(ValueError):
            DIAGNOSTICS.bind_dump(other_parsed, report, process, candidate, root)
        for index, args in enumerate(cases):
            with self.subTest(case=index), self.assertRaises(ValueError):
                DIAGNOSTICS.bind_dump(parsed, *args)


class AuthorityTests(unittest.TestCase):
    def environment(self):
        return {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_ARCH': 'X64',
                'GITHUB_REPOSITORY': 'qiyuanhuakai/dezirclip', 'GITHUB_EVENT_NAME': 'workflow_dispatch',
                'DZC_WINDOWS_CRASH_DIAGNOSTICS_ENABLED': 'true', 'DZC_WINDOWS_PERFORMANCE_ENABLED': 'false',
                'GITHUB_SHA': 'a' * 40, 'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1'}

    def test_requires_the_explicit_disposable_diagnostic_mode(self):
        expected = {'source_revision': 'a' * 40, 'run_id': '123', 'run_attempt': '1'}
        self.assertEqual(DIAGNOSTICS.context(self.environment(), platform='win32'), expected)
        for field, value in (('GITHUB_ACTIONS', 'false'), ('RUNNER_ENVIRONMENT', 'self-hosted'),
                             ('RUNNER_ARCH', 'ARM64'), ('GITHUB_REPOSITORY', 'other/repository'),
                             ('GITHUB_EVENT_NAME', 'pull_request'), ('DZC_WINDOWS_CRASH_DIAGNOSTICS_ENABLED', 'false'),
                             ('DZC_WINDOWS_PERFORMANCE_ENABLED', 'true'), ('GITHUB_SHA', 'bad'),
                             ('GITHUB_RUN_ID', '0'), ('GITHUB_RUN_ATTEMPT', '0')):
            with self.subTest(field=field), self.assertRaises(ValueError):
                DIAGNOSTICS.context({**self.environment(), field: value}, platform='win32')
        with self.assertRaises(ValueError):
            DIAGNOSTICS.context(self.environment(), platform='linux')

    def test_local_cli_refuses_before_any_registry_or_product_operation(self):
        with patch.dict(DIAGNOSTICS.os.environ, {}, clear=True), patch.object(DIAGNOSTICS.sys, 'argv', ['diagnostics', 'run']), \
                patch.object(DIAGNOSTICS, 'run', side_effect=AssertionError('Must never execute locally')):
            with self.assertRaises(ValueError):
                DIAGNOSTICS.main()

    def test_cleanup_requires_owned_unchanged_configuration(self):
        snapshot = {'subkeys': 0, 'values': [['DumpCount', 1, 4]], 'last_write_100ns': '123'}
        journal = {'created_child': True, 'child_snapshot': copy.deepcopy(snapshot)}
        self.assertTrue(DIAGNOSTICS.cleanup_matches(snapshot, journal))
        self.assertFalse(DIAGNOSTICS.cleanup_matches(snapshot, {**journal, 'created_child': False}))
        for changed in ({**snapshot, 'subkeys': 1}, {**snapshot, 'values': []}, {**snapshot, 'last_write_100ns': '124'}):
            self.assertFalse(DIAGNOSTICS.cleanup_matches(changed, journal))

    def test_rejects_nonlocal_module_paths(self):
        self.assertEqual(DIAGNOSTICS.windows_path(IMAGE.upper()), DIAGNOSTICS.windows_path(IMAGE))
        for value in ('relative.exe', r'\\server\share\dezirclip.exe', 'Z:\\bad\0path', ''):
            with self.subTest(path=value), self.assertRaises(ValueError):
                DIAGNOSTICS.windows_path(value)

    def test_no_reproduction_never_closes_the_historical_heap_failure(self):
        self.assertEqual(DIAGNOSTICS.reproduction_status({'status': 'passed', 'desktop_exit_verdict': {'allowed': True}}), 'not_reproduced')
        self.assertEqual(DIAGNOSTICS.reproduction_status({'status': 'failed', 'desktop_exit_verdict': {'abnormal_before_cleanup': True}}), 'original_abnormal_exit_observed')
        for report in ({}, {'status': 'passed'}, {'status': 'failed', 'desktop_exit_verdict': {'allowed': True}}):
            self.assertEqual(DIAGNOSTICS.reproduction_status(report), 'functional_failure_without_confirmed_original_abnormal_exit')

    def test_journal_replacement_is_complete_and_initial_creation_is_exclusive(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'journal.json'
            DIAGNOSTICS.write_json(path, {'created_child': False})
            with self.assertRaises(FileExistsError):
                DIAGNOSTICS.write_json(path, {'created_child': True})
            DIAGNOSTICS.write_json(path, {'created_child': True, 'snapshot': ['original']}, replace=True)
            self.assertEqual(DIAGNOSTICS.read_json(path), {'created_child': True, 'snapshot': ['original']})
            self.assertEqual(list(Path(directory).iterdir()), [path])


class RegistryLifecycleTests(unittest.TestCase):
    def fake_api(self, outcomes):
        closed, calls = [], []
        state = iter(outcomes)

        class Create:
            def __call__(self, parent, name, reserved, klass, options, access, security, handle, disposition):
                calls.append((parent, name))
                status, value, created = next(state)
                handle._obj.value = value
                disposition._obj.value = created
                return status

        winreg = SimpleNamespace(KEY_READ=0x20019, KEY_WRITE=0x20006, KEY_WOW64_64KEY=0x100, CloseKey=closed.append)
        return winreg, SimpleNamespace(RegCreateKeyExW=Create()), calls, closed

    def test_native_integer_handles_close_child_then_parent_on_exception(self):
        winreg, library, calls, closed = self.fake_api([(0, 200, 1), (0, 201, 1)])
        with patch.object(DIAGNOSTICS.ctypes, 'WinDLL', return_value=library, create=True):
            with self.assertRaisesRegex(RuntimeError, 'functional failure'):
                with DIAGNOSTICS.registry_child(100, 'LocalDumps', winreg) as (parent, created):
                    self.assertEqual((parent, created), (200, True))
                    with DIAGNOSTICS.registry_child(parent, 'dezirclip.exe', winreg) as (child, created):
                        self.assertEqual((child, created), (201, True))
                        raise RuntimeError('functional failure')
        self.assertEqual(calls, [(100, 'LocalDumps'), (200, 'dezirclip.exe')])
        self.assertEqual(closed, [201, 200])
        self.assertFalse(hasattr(winreg, 'HKEYType'), 'No unsupported handle constructor is needed')

    def test_preexisting_child_is_reported_without_losing_its_open_handle(self):
        winreg, library, _, closed = self.fake_api([(0, 201, 2)])
        with patch.object(DIAGNOSTICS.ctypes, 'WinDLL', return_value=library, create=True):
            with DIAGNOSTICS.registry_child(200, 'dezirclip.exe', winreg) as (child, created):
                self.assertEqual(child, 201)
                self.assertFalse(created)
        self.assertEqual(closed, [201])

    def test_failed_child_creation_still_closes_parent_without_closing_unowned_handle(self):
        winreg, library, _, closed = self.fake_api([(0, 200, 1), (5, None, 0)])
        with patch.object(DIAGNOSTICS.ctypes, 'WinDLL', return_value=library, create=True):
            with self.assertRaises(OSError):
                with DIAGNOSTICS.registry_child(100, 'LocalDumps', winreg) as (parent, _):
                    with DIAGNOSTICS.registry_child(parent, 'dezirclip.exe', winreg):
                        self.fail('Failed native creation must not reach the body')
        self.assertEqual(closed, [200])

    def test_read_only_runtime_probe_closes_detached_handle_even_if_query_fails(self):
        for should_fail in (False, True):
            closed, queried = [], []

            class Key:
                def __enter__(self): return self
                def __exit__(self, *_args): return False
                def Detach(self): return 200

            def query(handle):
                queried.append(handle)
                if should_fail:
                    raise OSError('read failure')
                return 0, 0, 0

            winreg = SimpleNamespace(HKEY_LOCAL_MACHINE=100, KEY_READ=0x20019, KEY_WOW64_64KEY=0x100,
                                     OpenKey=lambda *_args: Key(), QueryInfoKey=query, CloseKey=closed.append)
            with patch.object(DIAGNOSTICS.sys, 'version_info', (3, 13, 15)):
                if should_fail:
                    with self.assertRaises(OSError): DIAGNOSTICS.probe_registry_api(winreg)
                else:
                    self.assertFalse(DIAGNOSTICS.probe_registry_api(winreg)['registry_writes'])
            self.assertEqual(queried, [200])
            self.assertEqual(closed, [200])


if __name__ == '__main__':
    unittest.main()
