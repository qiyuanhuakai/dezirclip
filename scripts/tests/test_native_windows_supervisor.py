"""Only fake child processes and ordinary temporary metadata; never execute a controller or API probe."""
from contextlib import redirect_stderr
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from native_windows_supervisor import RECEIPT_NAME, run_controller, write_supervisor_receipt


class FakeChild:
    pid = 1772

    def __init__(self, results, kill_error=None):
        self.results = list(results)
        self.kill_error = kill_error
        self.calls = []

    def wait(self, *, timeout):
        self.calls.append(('wait', timeout))
        result = self.results.pop(0)
        if isinstance(result, BaseException):
            raise result
        return result

    def kill(self):
        self.calls.append(('kill',))
        if self.kill_error is not None:
            raise self.kill_error


class SupervisorTests(unittest.TestCase):
    def run_fake(self, child, *, clock=None, writer_error=None, launch_error=None):
        starts, receipts = [], []

        def start(command, **kwargs):
            starts.append((command, kwargs))
            if launch_error:
                raise launch_error
            return child

        def write(root, receipt):
            receipts.append((root, copy.deepcopy(receipt)))
            if writer_error:
                raise writer_error
            return True

        with redirect_stderr(io.StringIO()):
            result = run_controller(['node', 'controller.fake'], cwd='repository.fake', output_root='output.fake',
                                    popen=start, clock=clock or (lambda: 10), write_receipt=write)
        self.assertEqual(starts, [(['node', 'controller.fake'], {'cwd': 'repository.fake'})])
        return result, receipts[0][1]

    def test_zero_and_original_nonzero_are_retained_without_kill(self):
        for code in (0, 1, 37, 0xc0000374):
            with self.subTest(code=code):
                child = FakeChild([code])
                result, receipt = self.run_fake(child)
                self.assertEqual(result, code)
                self.assertEqual(child.calls, [('wait', 600)])
                self.assertEqual(receipt['controller_exit_code'], code)
                self.assertTrue(receipt['termination_confirmed'])
                self.assertFalse(receipt['kill_requested'])
                self.assertEqual(receipt['product_cleanup'], 'not_verified_by_supervisor')

    def test_spawn_elapsed_counts_against_the_same_total_deadline(self):
        ticks = iter((10, 50, 55, 55))
        child = FakeChild([0])
        result, _ = self.run_fake(child, clock=lambda: next(ticks))
        self.assertEqual(result, 0)
        self.assertEqual(child.calls, [('wait', 560)])

    def test_observed_late_success_is_timeout_with_confirmed_exit_and_no_kill(self):
        ticks = iter((10, 10, 611, 611))
        child = FakeChild([0])
        result, receipt = self.run_fake(child, clock=lambda: next(ticks))
        self.assertEqual(result, 124)
        self.assertTrue(receipt['timed_out'])
        self.assertTrue(receipt['termination_confirmed'])
        self.assertEqual(receipt['controller_exit_code'], 0)
        self.assertEqual(child.calls, [('wait', 600)])

    def test_timeout_kills_only_spawned_child_then_waits_five_seconds(self):
        child = FakeChild([subprocess.TimeoutExpired('fake', 600), 1])
        result, receipt = self.run_fake(child)
        self.assertEqual(result, 124)
        self.assertEqual(child.calls, [('wait', 600), ('kill',), ('wait', 5)])
        self.assertTrue(receipt['kill_call_returned'])
        self.assertTrue(receipt['termination_confirmed'])
        self.assertFalse(receipt['termination_unconfirmed'])
        self.assertEqual(receipt['controller_exit_code'], 1)
        self.assertEqual(receipt['first_failure']['stage'], 'controller_deadline')

    def test_kill_return_is_not_exit_and_unconfirmed_wait_never_retries(self):
        child = FakeChild([subprocess.TimeoutExpired('fake', 600), subprocess.TimeoutExpired('fake', 5)])
        result, receipt = self.run_fake(child)
        self.assertEqual(result, 124)
        self.assertEqual(child.calls, [('wait', 600), ('kill',), ('wait', 5)])
        self.assertTrue(receipt['kill_call_returned'])
        self.assertFalse(receipt['termination_confirmed'])
        self.assertTrue(receipt['termination_unconfirmed'])
        self.assertIsNone(receipt['controller_exit_code'])

    def test_kill_error_still_has_one_bounded_wait_and_keeps_timeout(self):
        child = FakeChild([subprocess.TimeoutExpired('fake', 600), subprocess.TimeoutExpired('fake', 5)],
                          kill_error=PermissionError('synthetic kill denial'))
        result, receipt = self.run_fake(child)
        self.assertEqual(result, 124)
        self.assertFalse(receipt['kill_call_returned'])
        self.assertTrue(receipt['termination_unconfirmed'])
        self.assertEqual([item['stage'] for item in receipt['secondary_errors']], ['controller_kill', 'controller_kill_wait'])

    def test_wait_error_is_retained_after_bounded_teardown(self):
        child = FakeChild([OSError('synthetic first wait failure'), 1])
        result, receipt = self.run_fake(child)
        self.assertEqual(result, 1)
        self.assertEqual(receipt['first_failure']['stage'], 'controller_wait')
        self.assertEqual(child.calls, [('wait', 600), ('kill',), ('wait', 5)])

    def test_launch_error_does_not_claim_process_exit_or_cleanup(self):
        child = FakeChild([])
        result, receipt = self.run_fake(child, launch_error=OSError('synthetic launch failure'))
        self.assertEqual(result, 1)
        self.assertFalse(receipt['child_started'])
        self.assertFalse(receipt['termination_confirmed'])
        self.assertFalse(receipt['termination_unconfirmed'])
        self.assertEqual(receipt['first_failure']['stage'], 'controller_start')
        self.assertEqual(child.calls, [])

    def test_secondary_receipt_failure_never_replaces_original_nonzero_or_timeout(self):
        for code in (37, 0xc0000374, 124):
            with self.subTest(code=code):
                child = FakeChild([code])
                result, _ = self.run_fake(child, writer_error=OSError('synthetic metadata failure'))
                self.assertEqual(result, code)
        child = FakeChild([subprocess.TimeoutExpired('fake', 600), 1])
        result, _ = self.run_fake(child, writer_error=OSError('synthetic metadata failure'))
        self.assertEqual(result, 124)

    def test_receipt_failure_cannot_turn_zero_into_success(self):
        result, _ = self.run_fake(FakeChild([0]), writer_error=OSError('synthetic metadata failure'))
        self.assertEqual(result, 1)

    def test_missing_root_is_never_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'not-created-by-controller'
            self.assertFalse(write_supervisor_receipt(root, {'synthetic': True}))
            self.assertFalse(root.exists())

    def test_new_receipt_preserves_original_files_and_never_overwrites_itself(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = root / 'report.json'
            original.write_bytes(b'original synthetic failure')
            self.assertTrue(write_supervisor_receipt(root, {'synthetic': True}))
            self.assertEqual(json.loads((root / RECEIPT_NAME).read_text(encoding='utf-8')), {'synthetic': True})
            with self.assertRaises(FileExistsError):
                write_supervisor_receipt(root, {'replacement': True})
            self.assertEqual(original.read_bytes(), b'original synthetic failure')
            self.assertEqual(json.loads((root / RECEIPT_NAME).read_text(encoding='utf-8')), {'synthetic': True})
            self.assertFalse(any(root.glob('*.tmp')))

    def test_collector_keeps_timeout_or_original_nonzero_and_runs_wer_finally(self):
        source = Path(__file__).with_name('windows-crash-diagnostics.py')
        spec = importlib.util.spec_from_file_location('supervisor_collector_fake', source)
        collector = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(collector)
        workflow = {'source_revision': 'a' * 40, 'run_id': '123', 'run_attempt': '1'}
        args = SimpleNamespace(desktop='desktop.fake', helper='helper.fake', helper_proof='proof.fake')
        for code in (37, 124):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as temporary:
                outputs, cleanup_calls = {}, []

                def private(root):
                    root.mkdir()
                    return 'synthetic-sid'

                def functional(command, *, cwd, output_root):
                    self.assertEqual(command[0], 'node')
                    output_root.mkdir()
                    (output_root / 'report.json').write_bytes(b'original synthetic functional failure')
                    return code

                def restore(root, identity):
                    cleanup_calls.append((root, identity))
                    return {'status': 'restored'}

                collection = {'accepted': None, 'original_functional_status': 'failed',
                              'original_functional_error': 'synthetic first failure'}
                with patch.dict(os.environ, {'RUNNER_TEMP': temporary}), \
                     patch.dict(collector.sys.modules, {'winreg': SimpleNamespace()}), \
                     patch.object(collector.subprocess, 'run', return_value=SimpleNamespace(stdout=workflow['source_revision'])), \
                     patch.object(collector, 'private_directory', side_effect=private), \
                     patch.object(collector, 'emit_output', side_effect=lambda name, value: outputs.update({name: value})), \
                     patch.object(collector, 'probe_registry_api', return_value={'synthetic': True}), \
                     patch.object(collector, 'tools_inventory', return_value=[]), \
                     patch.object(collector, 'configure_wer'), \
                     patch.object(collector, 'run_controller', side_effect=functional), \
                     patch.object(collector, 'collect_dump', return_value=collection), \
                     patch.object(collector, 'restore_wer', side_effect=restore):
                    self.assertEqual(collector.run(args, workflow), code)
                root = Path(outputs['root'])
                diagnostics = json.loads((root / 'diagnostics.json').read_text(encoding='utf-8'))
                self.assertEqual(diagnostics['functional_exit_code'], code)
                self.assertEqual(diagnostics['exit_code'], code)
                self.assertEqual(diagnostics['collection'], collection)
                self.assertEqual(diagnostics['cleanup'], {'status': 'restored'})
                self.assertEqual(cleanup_calls, [(root, workflow)])
                self.assertEqual((Path(outputs['functional_root']) / 'report.json').read_bytes(),
                                 b'original synthetic functional failure')


if __name__ == '__main__':
    unittest.main()
