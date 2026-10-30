"""All-fake checks of the dedicated wrapper's original-root ownership boundary."""
import ast
import json
from pathlib import Path
import tempfile
import types
import unittest

SOURCE = Path(__file__).with_name('native-windows-service-paste-target.py')


class FakePath(type(Path())):
    def is_junction(self):
        return False


def wrapper(base_native, string_at=None, save=None):
    tree = ast.parse(SOURCE.read_text(encoding='utf-8'))
    node = next(item for item in tree.body if isinstance(item, ast.ClassDef) and item.name == 'ServiceTarget')
    node.body = [method for method in node.body if isinstance(method, ast.FunctionDef) and method.name != '__init__']
    def require(value, message):
        if not value:
            raise RuntimeError(message)
    saved = []
    def seal(path, value):
        if save is not None:
            save(path, value)
        saved.append((path, value))
    scope = {'base': types.SimpleNamespace(Native=base_native, require=require, save=seal), 'Path': FakePath,
             'json': json, 'os': types.SimpleNamespace(environ={'GITHUB_SHA': 'a' * 40}),
             'hashlib': __import__('hashlib'), 're': __import__('re')}
    scope['ctypes'] = types.SimpleNamespace(string_at=string_at)
    scope['time'] = types.SimpleNamespace(monotonic_ns=lambda: 1000)
    exec(compile(ast.fix_missing_locations(ast.Module(body=[node], type_ignores=[])), str(SOURCE), 'exec'), scope)
    kind = scope['ServiceTarget']
    kind.saved = saved
    return kind


class ServiceWrapperTests(unittest.TestCase):
    def test_epoch_archives_complete_old_ring_before_clear_and_never_accepts_another_label(self):
        from collections import deque
        def seal(path, value):
            with path.open('x', encoding='utf-8') as stream:
                json.dump(value, stream)
        with tempfile.TemporaryDirectory() as temporary:
            kind = wrapper(object, save=seal)
            native = kind.__new__(kind)
            native.root = Path(temporary)
            native.observation_epoch, native.observation_label = 0, None
            native.focus_messages = deque([{'message': 7}, {'message': 8}], maxlen=64)
            native.callback_errors = []
            native.pump_messages = lambda: None
            native.focus_snapshot = lambda: {'foreground_hwnd': 11, 'messages': list(native.focus_messages)}
            result = native.begin_observation_epoch('correct_original_target')
            archive = json.loads((native.root / result['archive']['file']).read_text(encoding='utf-8'))
            self.assertEqual(archive['previous_focus']['messages'], [{'message': 7}, {'message': 8}])
            self.assertEqual(archive['previous_epoch'], 0)
            self.assertEqual(archive['boundary_monotonic_ns'], '1000')
            self.assertEqual(list(native.focus_messages), [])
            self.assertEqual(native.observation_epoch, 1)
            with self.assertRaisesRegex(RuntimeError, 'epoch order'):
                native.begin_observation_epoch('correct_original_target')
            self.assertEqual(len(kind.saved), 1)

    def test_epoch_saturation_or_archive_error_never_clears_events_or_advances(self):
        from collections import deque
        for fault in ['saturation', 'archive']:
            def fail_save(path, value):
                raise OSError('exclusive archive failed')
            kind = wrapper(object, save=fail_save)
            native = kind.__new__(kind)
            native.root = Path('synthetic-root')
            native.observation_epoch, native.observation_label = 1, 'correct_original_target'
            native.focus_messages = deque([{'message': 7}] * (64 if fault == 'saturation' else 2), maxlen=64)
            native.callback_errors = []
            native.pump_messages = lambda: None
            native.focus_snapshot = lambda: {'messages': list(native.focus_messages)}
            before = list(native.focus_messages)
            with self.assertRaises((RuntimeError, OSError)):
                native.begin_observation_epoch('stale_original_target_refused')
            self.assertEqual(list(native.focus_messages), before)
            self.assertEqual(native.observation_epoch, 1)
            self.assertEqual(native.observation_label, 'correct_original_target')
            self.assertEqual(kind.saved, [])

    def test_service_only_failure_cleanup_anchors_the_existing_strict_tree_and_restores_role(self):
        class Original:
            def cleanup(self):
                self.observed_anchor = self.desktop
                self.observed_error = self.binding_error
                return {'remaining_owned_processes': []}
        kind = wrapper(Original)
        native = kind.__new__(kind)
        native.desktop = None
        native.service = 47
        native.root = Path('synthetic-root')
        native.binding_error = 'original Desktop binding failed'
        result = native.cleanup()
        self.assertEqual(native.observed_anchor, 47)
        self.assertEqual(native.observed_error, 'original Desktop binding failed')
        self.assertIsNone(native.desktop)
        self.assertEqual(result['discovery_anchor_role'], 'service_original_before_desktop_bind')
        self.assertIsNone(result['actual_desktop_binding'])
        self.assertEqual(kind.saved[0][1]['original_binding_error'], 'original Desktop binding failed')

    def test_cleanup_exception_restores_actual_desktop_and_cannot_claim_a_result(self):
        class Original:
            def cleanup(self):
                raise RuntimeError('strict discovery failed')
        kind = wrapper(Original)
        native = kind.__new__(kind)
        native.desktop = None
        native.service = 47
        native.root = Path('synthetic-root')
        with self.assertRaisesRegex(RuntimeError, 'strict discovery failed'):
            native.cleanup()
        self.assertIsNone(native.desktop)

    def test_existing_desktop_remains_the_anchor_and_authenticated_service_is_not_renamed(self):
        class Original:
            def cleanup(self):
                self.observed_anchor = self.desktop
                return {}
        kind = wrapper(Original)
        native = kind.__new__(kind)
        native.desktop = 53
        native.service = 47
        native.root = Path('synthetic-root')
        result = native.cleanup()
        self.assertEqual(native.observed_anchor, 53)
        self.assertEqual(native.desktop, 53)
        self.assertEqual(result['actual_desktop_binding'], 53)

    def test_unvalidated_service_is_never_used_as_a_discovery_anchor(self):
        class Original:
            def cleanup(self):
                self.observed_anchor = self.desktop
                raise RuntimeError('pending handle close only')
        kind = wrapper(Original)
        native = kind.__new__(kind)
        native.desktop = None
        native.service = None
        native.root = Path('synthetic-root')
        with self.assertRaisesRegex(RuntimeError, 'pending handle close only'):
            native.cleanup()
        self.assertIsNone(native.observed_anchor)

    def test_service_hash_build_image_and_one_time_binding_precede_certificate_promotion(self):
        import hashlib
        class Original:
            def process(self, pid, phase):
                self.phases.append(phase)
                self.handles[pid] = 71
                image = self.root / ('candidate/dzc-service.exe' if self.valid_image else 'candidate/not-the-service.exe')
                self.identities[pid] = {'image': str(image), 'creation_time_100ns': '79'}
                return {'pid': pid, 'running': True, **self.identities[pid]}
        for failure in [None, 'hash', 'build', 'image']:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                root = FakePath(temporary)
                (root / 'candidate').mkdir()
                (root / 'candidate/dzc-service.exe').write_bytes(b'synthetic non executable bytes')
                receipt = {'pid': 47, 'sha256': hashlib.sha256(b'synthetic non executable bytes').hexdigest(),
                           'build': {'component': 'service', 'source_state': 'verified_checkout', 'source_revision': 'a' * 40,
                                     'target': 'x86_64-pc-windows-msvc', 'component_version': 'fixture', 'build_id': 'fixture@' + 'a' * 40}}
                if failure == 'hash':
                    receipt['sha256'] = 'b' * 64
                if failure == 'build':
                    receipt['build']['component'] = 'cli'
                (root / 'service-identity.json').write_text(json.dumps(receipt), encoding='utf-8')
                kind = wrapper(Original)
                native = kind.__new__(kind)
                native.root = root
                native.service = None
                native.service_binding_attempted = False
                native.expected_bindings = {'service'}
                native.valid_image = failure != 'image'
                native.handles, native.identities, native.validated_owned_handles = {}, {}, {}
                native.phases = []
                if failure:
                    with self.assertRaises(RuntimeError):
                        native.bind_service(47)
                    self.assertIsNone(native.service)
                    self.assertEqual(native.validated_owned_handles, {})
                    self.assertIn('Service ownership validation failed', native.binding_error)
                    self.assertEqual(bool(native.handles), failure == 'image', 'only a checked-image failure retains a pending handle')
                else:
                    result = native.bind_service(47)
                    self.assertEqual(result['role'], 'service_original')
                    self.assertEqual(native.service, 47)
                    self.assertEqual(native.validated_owned_handles[47], {'handle': 71, 'identity': native.identities[47]})
                    with self.assertRaisesRegex(RuntimeError, 'One original Service'):
                        native.bind_service(47)
                    self.assertEqual(native.phases, ['service-original-binding'])

    def test_spawn_reservation_without_authenticated_binding_is_fatal_before_restore(self):
        class Original:
            def cleanup(self):
                self.observed_error = self.binding_error
                raise RuntimeError(self.binding_error)
        kind = wrapper(Original)
        native = kind.__new__(kind)
        native.root = Path('synthetic-root')
        native.desktop = None
        native.service = 47
        native.expected_bindings = {'service', 'desktop'}
        with self.assertRaisesRegex(RuntimeError, 'Unconfirmed reserved originals: desktop'):
            native.cleanup()
        self.assertIsNone(native.desktop)
        self.assertEqual(kind.saved[0][1]['unresolved_reserved_originals'], ['desktop'])

    def test_readonly_clipboard_comparison_is_size_bounded_and_never_returns_unknown_content(self):
        expected = 'Service paste synthetic generated ' + 'a' * 32
        payload = expected.encode('utf-16-le') + b'\0\0'
        for data in [payload, payload[:-2] + b'X\0\0\0', b'unknown']:
            calls = []
            kind = wrapper(object, lambda address, length: calls.append(('read', length)) or data[:length])
            native = kind.__new__(kind)
            native.target = 11
            native.user = types.SimpleNamespace(OpenClipboard=lambda hwnd: calls.append(('open', hwnd)) or True,
                GetClipboardData=lambda format: 47, CloseClipboard=lambda: calls.append(('close',)) or True)
            native.kernel = types.SimpleNamespace(GlobalSize=lambda memory: len(data),
                GlobalLock=lambda memory: calls.append(('lock', memory)) or 53,
                GlobalUnlock=lambda memory: calls.append(('unlock', memory)) or True)
            result = native.clipboard_matches(expected)
            self.assertEqual(result['matches_expected'], data == payload)
            self.assertFalse(result['unknown_content_returned'])
            self.assertNotIn('text', result)
            self.assertEqual(calls[-1], ('close',))
            if len(data) >= len(payload):
                self.assertIn(('read', len(payload)), calls)
                self.assertIn(('unlock', 47), calls)
            else:
                self.assertFalse(any(item[0] in ('read', 'lock', 'unlock') for item in calls))

    def test_readonly_clipboard_errors_close_and_unlock_without_free_or_new_input(self):
        expected = 'Service paste synthetic generated ' + 'b' * 32
        for fault in ['size', 'lock', 'read']:
            calls = []
            def fail_read(address, length):
                raise RuntimeError('read failed')
            kind = wrapper(object, fail_read)
            native = kind.__new__(kind)
            native.target = 11
            native.user = types.SimpleNamespace(OpenClipboard=lambda hwnd: True, GetClipboardData=lambda format: 47,
                CloseClipboard=lambda: calls.append('close') or True)
            native.kernel = types.SimpleNamespace(GlobalSize=lambda memory: 0 if fault == 'size' else 1024,
                GlobalLock=lambda memory: 0 if fault == 'lock' else 53,
                GlobalUnlock=lambda memory: calls.append('unlock') or True)
            with self.assertRaises(RuntimeError):
                native.clipboard_matches(expected)
            self.assertEqual(calls, ['unlock', 'close'] if fault == 'read' else ['close'])


if __name__ == '__main__':
    unittest.main()
