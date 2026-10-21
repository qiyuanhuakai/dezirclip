import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('native-windows-clipboard.py')
spec = importlib.util.spec_from_file_location('native_clipboard_fixture', SCRIPT)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class SyntheticClipboardBoundaryTests(unittest.TestCase):
    def test_only_disposable_windows_context_accepts_the_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_TEMP': directory,
                           'DEZIRCLIP_NATIVE_CAPTURE_TEXT': 'nativeupgradefixturemarker' + 'a' * 32 + '-native-a'}
            self.assertEqual(fixture.validate_environment(environment, 'win32'), environment['DEZIRCLIP_NATIVE_CAPTURE_TEXT'])
            for field, value in [('GITHUB_ACTIONS', 'false'), ('RUNNER_ENVIRONMENT', 'self-hosted'), ('RUNNER_TEMP', ''),
                                 ('DEZIRCLIP_NATIVE_CAPTURE_TEXT', 'ordinary user text'), ('DEZIRCLIP_NATIVE_CAPTURE_TEXT', 'nativeupgradefixturemarker' + 'a' * 32 + '-native-a\x00')]:
                with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                    fixture.validate_environment({**environment, field: value}, 'win32')
            with self.assertRaises(RuntimeError):
                fixture.validate_environment(environment, 'linux')

    def test_actual_child_rejects_before_any_native_operation_outside_ci(self):
        environment = {**os.environ, 'GITHUB_ACTIONS': 'false'}
        result = subprocess.run([sys.executable, str(SCRIPT)], env=environment, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn('disposable GitHub-hosted Windows runner', result.stdout)
        self.assertNotIn('"phase": "started"', result.stdout)
        self.assertNotIn('"phase": "written"', result.stdout)


if __name__ == '__main__':
    unittest.main()
