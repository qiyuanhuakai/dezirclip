"""Pure source-derivation checks; never construct a native fixture or launch a product."""
import ast
import importlib.util
from pathlib import Path
import tempfile
import unittest

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
        for name in ('__init__', 'install_window_procedure', 'focus_snapshot', 'target_preflight', 'activate', 'process', 'bind', 'tree', 'cleanup'):
            with self.subTest(method=name):
                self.assertEqual(original[name], generated[name])

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


if __name__ == '__main__':
    unittest.main()
