"""Pure source-derivation checks; never construct a native fixture or launch a product."""
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

SOURCE = Path(__file__).with_name('windows-performance') / 'derive.py'
SPEC = importlib.util.spec_from_file_location('performance_derivation', SOURCE)
derivation = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(derivation)
REPOSITORY = SOURCE.resolve().parents[3]


class PerformanceDerivationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.controller = (REPOSITORY / 'scripts/tests/native-windows-surface.mjs').read_text(encoding='utf-8')
        cls.target = (REPOSITORY / 'scripts/tests/native-windows-surface-target.py').read_text(encoding='utf-8')
        cls.crash_events = (REPOSITORY / 'scripts/tests/native-windows-crash-events.mjs').read_text(encoding='utf-8')

    def test_reviewed_sources_derive_with_either_checkout_line_ending(self):
        native = derivation.derive_sources(self.controller, self.target, self.crash_events)
        converted = derivation.derive_sources(self.controller.replace('\n', '\r\n'), self.target.replace('\n', '\r\n'), self.crash_events.replace('\n', '\r\n'))
        self.assertEqual(native, converted)
        self.assertEqual(set(native), {'native-plumbing.mjs', 'native-target.py', 'native-plumbing.mjs.patch', 'native-target.py.patch', 'native-windows-crash-events.mjs'})
        self.assertEqual(native['native-windows-crash-events.mjs'], self.crash_events)
        ast.parse(native['native-target.py'])

    def test_either_functional_source_drift_requires_an_explicit_review(self):
        with self.assertRaisesRegex(AssertionError, 'controller changed'):
            derivation.derive_sources(self.controller + '\n', self.target, self.crash_events)
        with self.assertRaisesRegex(AssertionError, 'target changed'):
            derivation.derive_sources(self.controller, self.target + '\n', self.crash_events)
        with self.assertRaisesRegex(AssertionError, 'crash diagnostics changed'):
            derivation.derive_sources(self.controller, self.target, self.crash_events + '\n')

    def test_derivation_keeps_ownership_focus_calibration_and_cleanup_implementations(self):
        result = derivation.derive_sources(self.controller, self.target, self.crash_events)['native-target.py']
        def methods(source):
            native = next(node for node in ast.parse(source).body if isinstance(node, ast.ClassDef) and node.name == 'Native')
            return {node.name: ast.dump(node) for node in native.body if isinstance(node, ast.FunctionDef)}
        original, generated = methods(self.target), methods(result)
        for name in ('__init__', 'install_window_procedure', 'focus_snapshot', 'keyboard_snapshot', 'target_preflight', 'activate', 'process', 'bind', 'parent_snapshot', 'tree', 'cleanup', 'click_window_metadata', 'click_alternate'):
            with self.subTest(method=name):
                self.assertEqual(original[name], generated[name])
        for name in ('alternate_click_candidates', 'plan_alternate_click', 'send_planned_click'):
            before = next(node for node in ast.parse(self.target).body if isinstance(node, ast.FunctionDef) and node.name == name)
            after = next(node for node in ast.parse(result).body if isinstance(node, ast.FunctionDef) and node.name == name)
            with self.subTest(function=name):
                self.assertEqual(ast.dump(before), ast.dump(after))

    def test_derived_input_retains_one_immediately_timed_batch_and_two_read_only_snapshots(self):
        source = derivation.derive_sources(self.controller, self.target, self.crash_events)['native-target.py']
        native = next(node for node in ast.parse(source).body if isinstance(node, ast.ClassDef) and node.name == 'Native')
        keys = next(node for node in native.body if isinstance(node, ast.FunctionDef) and node.name == 'keys')
        names = lambda node: [target.id for target in node.targets if isinstance(target, ast.Name)] if isinstance(node, ast.Assign) else []
        assignments = {name: index for index, node in enumerate(keys.body) for name in names(node)}
        self.assertEqual(sum('input_monotonic_ns' in names(node) for node in keys.body), 1)
        self.assertEqual(assignments['input_monotonic_ns'] + 1, assignments['sent'])
        self.assertEqual(assignments['sent'] + 1, assignments['input_returned_monotonic_ns'])
        calls = [node for node in ast.walk(keys) if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)]
        self.assertEqual(sum(node.func.attr == 'SendInput' for node in calls), 1)
        self.assertEqual(sum(node.func.attr == 'keyboard_snapshot' for node in calls), 2)

    def test_derived_tauri_accessibility_requires_the_explicit_original_foreground_panel(self):
        source = derivation.derive_sources(self.controller, self.target, self.crash_events)['native-target.py']
        namespace = {'__name__': 'derived_target_pure', '__file__': str(REPOSITORY / '.harness/unused/native-target.py')}
        exec(compile(source, '<derived native target>', 'exec'), namespace)
        native_class = namespace['Native']
        panel = {'pid': 10, 'hwnd': 20, 'visible': True, 'class_name': 'Tauri Window', 'title': '',
                 'rect': {'width': 480, 'height': 360}}
        observation = {'windows': [panel], 'foreground': {'hwnd': 20, 'pid': 10}, 'processes': []}
        native = native_class.__new__(native_class)
        native.desktop = 10
        native.observe = lambda: observation
        native.process = lambda pid: {'running': True}
        completed = SimpleNamespace(returncode=0, stdout=b'[]', stderr=b'')
        with patch.dict(namespace['os'].environ, {}, clear=True), patch.object(namespace['subprocess'], 'run', return_value=completed) as reader:
            self.assertEqual(native.accessibility(10, 20), {'pid': 10, 'hwnd': 20, 'elements': []})
            reader.assert_called_once()
            for changed in ('missing-hwnd', 'wrong-hwnd', 'other-foreground', 'extra-visible-window', 'wrong-size', 'titled-main', 'dead-desktop'):
                with self.subTest(changed=changed):
                    reader.reset_mock()
                    modified = {**observation, 'windows': [{**panel, 'rect': dict(panel['rect'])}], 'foreground': dict(observation['foreground'])}
                    hwnd = 20
                    if changed == 'missing-hwnd': hwnd = None
                    elif changed == 'wrong-hwnd': hwnd = 21
                    elif changed == 'other-foreground': modified['foreground']['hwnd'] = 21
                    elif changed == 'extra-visible-window': modified['windows'].append({**panel, 'hwnd': 21})
                    elif changed == 'wrong-size': modified['windows'][0]['rect']['width'] = 481
                    elif changed == 'titled-main': modified['windows'][0]['title'] = 'DezirClip'
                    native.observe = lambda: modified
                    native.process = lambda pid: {'running': changed != 'dead-desktop'}
                    with self.assertRaises(RuntimeError): native.accessibility(10, hwnd)
                    reader.assert_not_called()

    def test_generation_cannot_overwrite_an_existing_or_outside_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary).resolve()
            boundary = repository / '.harness'
            boundary.mkdir()
            fresh = boundary / 'windows-performance-fresh'
            self.assertEqual(derivation.validate_destination(repository, fresh), fresh)
            fresh.mkdir()
            for destination in (fresh, repository / 'windows-performance-outside', boundary / 'unrelated', boundary / 'nested/windows-performance-hidden'):
                with self.subTest(destination=destination), self.assertRaises(ValueError):
                    derivation.validate_destination(repository, destination)

    def test_manifest_separates_controller_reference_and_reviewed_diagnostic_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary).resolve()
            names = ['scripts/tests/' + name for name in
                     ('native-windows-surface.mjs', 'native-windows-surface-target.py', 'native-windows-crash-events.mjs', 'native-windows-lifecycle.mjs')]
            names += ['scripts/tests/windows-performance/' + name for name in
                      ('controller.mjs', 'process-sampler.mjs', 'pure.node.mjs', 'tauri-query.mjs')]
            for name in names:
                path = repository / name
                path.parent.mkdir(parents=True, exist_ok=True)
                content = (REPOSITORY / name).read_text(encoding='utf-8')
                path.write_bytes(content.replace('\n', '\r\n').encode('utf-8'))
            generated = derivation.generate(repository, repository / '.harness/windows-performance-provenance')
            proof = json.loads((generated / 'derivation.json').read_text())
            self.assertNotIn('functional_base_commit', proof)
            self.assertEqual(proof['functional_controller_base_commit'], derivation.FUNCTIONAL_CONTROLLER_BASE_COMMIT)
            self.assertEqual(proof['functional_controller_base_scope'], ['native-windows-crash-events.mjs'])
            self.assertEqual(proof['functional_controller_provenance']['reviewed_lf_sha256'], derivation.REVIEWED_CONTROLLER_LF_SHA256)
            self.assertEqual(proof['functional_lifecycle_provenance']['reviewed_lf_sha256'], derivation.REVIEWED_LIFECYCLE_LF_SHA256)
            self.assertEqual(proof['functional_target_provenance']['reviewed_lf_sha256'], derivation.REVIEWED_TARGET_LF_SHA256)
            self.assertEqual(proof['functional_target_provenance']['diagnostic_delta'], 'bounded-key-state-owned-message-timing-predeclared-click-planning-generation-verified-handle-cleanup-lifetime-owned-closure-certified-output-failure-only-parent-diagnostics-and-checked-snapshot-eof')
            self.assertEqual(proof['functional_target_provenance']['original_target_reference']['lf_sha256'], 'cf3ad99a1c21a1267dd3011567a9a9bc434baa8d7214f7e1c8c730bdc30c5780')
            self.assertFalse(proof['native_runtime_executed_by_generator'])
            self.assertEqual(set(proof['source_files']), set(names))
            for name in names:
                with self.subTest(name=name):
                    raw = (repository / name).read_bytes()
                    self.assertEqual(proof['source_files'][name], {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(),
                        'canonical_lf_sha256': hashlib.sha256(raw.decode('utf-8').replace('\r\n', '\n').encode('utf-8')).hexdigest()})
            self.assertNotEqual(proof['source_files']['scripts/tests/native-windows-surface-target.py']['sha256'], derivation.REVIEWED_TARGET_LF_SHA256)

    def test_lifecycle_drift_is_rejected_before_generating_a_fixture(self):
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary).resolve()
            for name in ('native-windows-surface.mjs', 'native-windows-surface-target.py', 'native-windows-crash-events.mjs', 'native-windows-lifecycle.mjs'):
                output = repository / 'scripts/tests' / name
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text((REPOSITORY / 'scripts/tests' / name).read_text(), encoding='utf-8')
            lifecycle = repository / 'scripts/tests/native-windows-lifecycle.mjs'
            lifecycle.write_text(lifecycle.read_text() + '\n', encoding='utf-8')
            destination = repository / '.harness/windows-performance-drift'
            with self.assertRaisesRegex(AssertionError, 'lifecycle changed'):
                derivation.generate(repository, destination)
            self.assertFalse(destination.exists())


if __name__ == '__main__':
    unittest.main()
