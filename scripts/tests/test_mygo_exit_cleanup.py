import contextlib
import json
from pathlib import Path
import runpy
import subprocess
import unittest
from unittest.mock import patch


cleanup = runpy.run_path(str(Path(__file__).with_name('mygo-desktop-x11.py')))['observed_service_cleanup']


class DesktopExitCleanup(unittest.TestCase):
    def exercise(self, ready, stop_error=None):
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch('subprocess.check_output', return_value=json.dumps({'pid': 123, 'instance': 'original'}).encode()))
            opened = stack.enter_context(patch('os.pidfd_open', create=True, return_value=42))
            closed = stack.enter_context(patch('os.close'))
            stack.enter_context(patch('select.select', side_effect=[([42] if value else [], [], []) for value in ready]))
            stop = stack.enter_context(patch('subprocess.run', side_effect=stop_error))
            try:
                with cleanup('fixture-cli'):
                    opened.assert_called_once_with(123)
                    stop.assert_not_called()
            finally:
                closed.assert_called_once_with(42)
            return stop

    def test_already_exited_service_needs_no_second_stop(self):
        self.exercise([True, True]).assert_not_called()

    def test_live_original_requires_guarded_stop_and_process_exit(self):
        self.exercise([False, True]).assert_called_once_with(
            ['fixture-cli', 'service', 'stop', '--from-instance', 'original'], check=True)

    def test_unconfirmed_exit_is_not_success(self):
        with self.assertRaisesRegex(AssertionError, 'survived'):
            self.exercise([False, False])

    def test_stop_failure_is_not_swallowed(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.exercise([False], subprocess.CalledProcessError(1, 'fixture-cli'))


if __name__ == '__main__':
    unittest.main()
