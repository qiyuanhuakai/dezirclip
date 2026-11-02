"""Pure/fake Xvfb controller checks; never spawn a display, compiler or application."""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('verify_flatpak', Path(__file__).parents[1] / 'verify-flatpak.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Clock:
    def __init__(self):
        self.now = 0

    def monotonic(self):
        return self.now


class Stream:
    def __init__(self, error=None):
        self.closed, self.error = False, error

    def close(self):
        self.closed = True
        if self.error:
            raise self.error


class Process:
    def __init__(self, polls=None, waits=None, terminate_error=None, kill_error=None):
        self.pid, self.returncode = 123, None
        self.polls, self.waits = list(polls or []), list(waits or [-15])
        self.calls = []
        self.terminate_error, self.kill_error = terminate_error, kill_error
        self.stdin, self.stdout, self.stderr = None, None, None

    def __enter__(self):
        raise AssertionError('Popen context entry is not used')

    def __exit__(self, *args):
        raise AssertionError('Popen context exit could wait without a bound')

    def poll(self):
        self.calls.append(('poll',))
        if self.polls:
            value = self.polls.pop(0)
            if isinstance(value, BaseException):
                raise value
            self.returncode = value
        return self.returncode

    def terminate(self):
        self.calls.append(('terminate',))
        if self.terminate_error:
            raise self.terminate_error

    def kill(self):
        self.calls.append(('kill',))
        if self.kill_error:
            raise self.kill_error

    def wait(self, timeout):
        self.calls.append(('wait', timeout))
        value = self.waits.pop(0)
        if isinstance(value, BaseException):
            raise value
        self.returncode = value
        return value

    def communicate(self, timeout):
        self.calls.append(('communicate', timeout))
        self.returncode = 0
        return 'PASS Ctrl+V clipboard roundtrip\n', ''


def receipt(output, label):
    return json.loads(next(line[len(label) + 1:] for line in output.getvalue().splitlines() if line.startswith(label + ' ')))


class StartupTests(unittest.TestCase):
    def wait_fixture(self, chunks, process=None, events=None):
        process, clock = process or Process(), Clock()
        chunks, events, waits, reads = list(chunks), list(events or []), [], []
        def select(readable, writable, errors, timeout):
            self.assertEqual((readable, writable, errors), ([10], [], []))
            waits.append(timeout)
            advance, ready = events.pop(0) if events else (0, True)
            clock.now += advance
            return (readable if ready else [], [], [])
        def read(fd, limit):
            self.assertEqual(fd, 10)
            self.assertGreater(limit, 0)
            reads.append(limit)
            value = chunks.pop(0)
            if isinstance(value, BaseException):
                raise value
            self.assertLessEqual(len(value), limit)
            return value
        stack = ExitStack()
        stack.enter_context(patch.object(MODULE.time, 'monotonic', clock.monotonic))
        stack.enter_context(patch.object(MODULE.select, 'select', select))
        stack.enter_context(patch.object(MODULE.os, 'read', read))
        return process, stack, waits, reads

    def test_split_decimal_line_and_zero_display_are_accepted_with_exact_bounded_bytes(self):
        for chunks, expected in [([b'12', b'\n'], ':12'), ([b'0\n'], ':0')]:
            with self.subTest(chunks=chunks):
                server, stack, waits, reads = self.wait_fixture(chunks)
                output = io.StringIO()
                with stack, redirect_stderr(output):
                    self.assertEqual(MODULE.wait_xvfb_display(server, 10), expected)
                stated = receipt(output, 'Xvfb startup')
                self.assertEqual(stated['status'], 'ready')
                self.assertEqual(stated['displayfd_hex'], b''.join(chunks).hex())
                self.assertEqual(stated['ready_budget_seconds'], 15)
                self.assertTrue(stated['stderr_inherited'])
                self.assertTrue(all(0 < limit <= 32 for limit in reads))
                self.assertTrue(all(timeout <= 15 for timeout in waits))

    def test_incomplete_invalid_and_overlong_lines_fail_without_unbounded_read(self):
        variants = [[b''], [b'12', b''], [b'\n'], [b' 12\n'], [b'12\r\n'], [b'-1\n'],
                    [b'12\nextra'], [b'\xff\n'], [b'1' * 32]]
        for chunks in variants:
            with self.subTest(chunks=chunks):
                server, stack, _, reads = self.wait_fixture(chunks)
                with stack, redirect_stderr(io.StringIO()), self.assertRaises(RuntimeError):
                    MODULE.wait_xvfb_display(server, 10)
                self.assertTrue(all(0 < limit <= 32 for limit in reads))

    def test_original_server_exit_before_or_after_ready_bytes_is_never_accepted(self):
        for polls in [[7], [None, 7]]:
            with self.subTest(polls=polls):
                server, stack, _, _ = self.wait_fixture([b'12\n'], Process(polls=polls))
                output = io.StringIO()
                with stack, redirect_stderr(output), self.assertRaisesRegex(RuntimeError, 'exited'):
                    MODULE.wait_xvfb_display(server, 10)
                self.assertEqual(receipt(output, 'Xvfb startup')['poll'], 7)
                self.assertFalse(any(call[0] in ('terminate', 'kill', 'wait') for call in server.calls))

    def test_partial_line_keeps_one_total_fifteen_second_budget(self):
        server, stack, waits, _ = self.wait_fixture([b'12'], events=[(10, True), (5, False)])
        output = io.StringIO()
        with stack, redirect_stderr(output), self.assertRaisesRegex(RuntimeError, '15 seconds'):
            MODULE.wait_xvfb_display(server, 10)
        self.assertEqual(waits, [15, 5])
        stated = receipt(output, 'Xvfb startup')
        self.assertEqual(stated['elapsed_seconds'], 15)
        self.assertEqual(stated['displayfd_hex'], b'12'.hex())

    def test_complete_line_after_original_deadline_does_not_extend_readiness(self):
        server, stack, waits, _ = self.wait_fixture([b'12\n'], events=[(15.01, True)])
        with stack, redirect_stderr(io.StringIO()), self.assertRaisesRegex(RuntimeError, '15 seconds'):
            MODULE.wait_xvfb_display(server, 10)
        self.assertEqual(waits, [15])

    def test_read_error_is_preserved_and_has_original_process_diagnostics(self):
        failure = OSError('synthetic displayfd read failure')
        server, stack, _, _ = self.wait_fixture([failure])
        output = io.StringIO()
        with stack, redirect_stderr(output), self.assertRaises(OSError) as raised:
            MODULE.wait_xvfb_display(server, 10)
        self.assertIs(raised.exception, failure)
        self.assertEqual(receipt(output, 'Xvfb startup')['pid'], server.pid)


class CleanupTests(unittest.TestCase):
    def test_only_original_live_object_is_terminated_and_waited_with_a_bound(self):
        process = Process()
        process.stdout, process.stderr = Stream(), Stream()
        output = io.StringIO()
        with redirect_stderr(output), MODULE.original_process(process, 'Xvfb') as original:
            self.assertIs(original, process)
        self.assertIn(('terminate',), process.calls)
        self.assertEqual([call for call in process.calls if call[0] == 'wait'], [('wait', 10)])
        self.assertNotIn(('kill',), process.calls)
        self.assertTrue(process.stdout.closed and process.stderr.closed)
        self.assertTrue(receipt(output, 'Process cleanup')['exit_confirmed'])

    def test_already_exited_original_is_not_signaled_or_waited_again(self):
        process = Process(polls=[0])
        with redirect_stderr(io.StringIO()), MODULE.original_process(process, 'paste receiver'):
            pass
        self.assertEqual(process.calls, [('poll',)])

    def test_force_kill_is_bounded_and_fails_even_when_body_succeeded(self):
        process = Process(waits=[subprocess.TimeoutExpired('fake', 10), -9])
        output = io.StringIO()
        with redirect_stderr(output), self.assertRaisesRegex(RuntimeError, 'cleanup failed'):
            with MODULE.original_process(process, 'Xvfb'):
                pass
        self.assertEqual([call for call in process.calls if call[0] == 'wait'], [('wait', 10), ('wait', 10)])
        self.assertIn(('kill',), process.calls)
        self.assertTrue(receipt(output, 'Process cleanup')['forced_kill'])

    def test_original_failure_survives_force_kill_and_second_wait_failure(self):
        primary = RuntimeError('original readiness failure')
        process = Process(waits=[subprocess.TimeoutExpired('fake', 10), subprocess.TimeoutExpired('fake', 10)])
        output = io.StringIO()
        with redirect_stderr(output), self.assertRaises(RuntimeError) as raised:
            with MODULE.original_process(process, 'Xvfb'):
                raise primary
        self.assertIs(raised.exception, primary)
        stated = receipt(output, 'Process cleanup')
        self.assertFalse(stated['exit_confirmed'])
        self.assertTrue(any('wait after kill' in item for item in stated['errors']))

    def test_terminate_error_is_reported_even_if_original_wait_later_succeeds(self):
        process = Process(terminate_error=OSError('cannot terminate'))
        output = io.StringIO()
        with redirect_stderr(output), self.assertRaisesRegex(RuntimeError, 'cannot terminate'):
            with MODULE.original_process(process, 'Xvfb'):
                pass
        self.assertTrue(receipt(output, 'Process cleanup')['exit_confirmed'])

    def test_poll_and_kill_errors_do_not_skip_the_two_bounded_wait_attempts(self):
        process = Process(polls=[OSError('poll failed')], waits=[OSError('wait failed'), -9], kill_error=OSError('kill failed'))
        with redirect_stderr(io.StringIO()), self.assertRaisesRegex(RuntimeError, 'poll failed'):
            with MODULE.original_process(process, 'Xvfb'):
                pass
        self.assertEqual([call for call in process.calls if call[0] == 'wait'], [('wait', 10), ('wait', 10)])

    def test_stream_close_error_preserves_primary_and_attempts_other_original_streams(self):
        primary = RuntimeError('first paste failure')
        process = Process(polls=[0])
        process.stdout, process.stderr = Stream(OSError('close failed')), Stream()
        output = io.StringIO()
        with redirect_stderr(output), self.assertRaises(RuntimeError) as raised:
            with MODULE.original_process(process, 'paste receiver'):
                raise primary
        self.assertIs(raised.exception, primary)
        self.assertTrue(process.stderr.closed)
        self.assertTrue(any('stdout close' in item for item in receipt(output, 'Process cleanup')['errors']))


class VerificationWiringTests(unittest.TestCase):
    def test_original_single_server_and_actual_paste_commands_remain_in_the_flow(self):
        server, receiver = Process(), Process()
        receiver.stdout, receiver.stderr = Stream(), Stream()
        def run(command, **kwargs):
            if '--list-langs' in command:
                return SimpleNamespace(stdout='eng\nchi_sim\nosd\n')
            if '--psm' in command:
                return SimpleNamespace(stdout='中文测试' if 'chi_sim' in command else 'DEZIRCLIP12345')
            return SimpleNamespace(stdout='')
        with ExitStack() as stack:
            stack.enter_context(redirect_stderr(io.StringIO()))
            stack.enter_context(redirect_stdout(io.StringIO()))
            stack.enter_context(patch.object(MODULE.time, 'monotonic', return_value=0))
            stack.enter_context(patch.object(MODULE.os, 'environ', {'SYNTHETIC': 'fixture'}))
            stack.enter_context(patch.object(MODULE.os, 'pipe', return_value=(10, 11)))
            close = stack.enter_context(patch.object(MODULE.os, 'close'))
            stack.enter_context(patch.object(MODULE.os, 'read', return_value=b'12\n'))
            stack.enter_context(patch.object(MODULE.select, 'select', return_value=([10], [], [])))
            ready = stack.enter_context(patch.object(MODULE, 'read_line', return_value='READY'))
            temp = stack.enter_context(patch.object(MODULE.tempfile, 'TemporaryDirectory'))
            temp.return_value.__enter__.return_value = 'synthetic-temp'
            spawn = stack.enter_context(patch.object(MODULE.subprocess, 'Popen', side_effect=[server, receiver]))
            execute = stack.enter_context(patch.object(MODULE.subprocess, 'run', side_effect=run))
            MODULE.verify(Path('synthetic-build'))
        self.assertEqual(spawn.call_count, 2)
        self.assertEqual(spawn.call_args_list[0].args[0][0], 'Xvfb')
        self.assertIsNone(spawn.call_args_list[0].kwargs['stderr'])
        self.assertEqual(spawn.call_args_list[0].kwargs['pass_fds'], (11,))
        self.assertEqual(close.call_args_list[0].args, (11,))
        self.assertEqual(close.call_args_list[-1].args, (10,))
        ready.assert_called_once_with(receiver)
        paste = [call for call in execute.call_args_list if call.args[0][-2:] == ['key', 'ctrl+v']]
        self.assertEqual(len(paste), 1)
        self.assertEqual(paste[0].kwargs['timeout'], 15)
        self.assertIn(('communicate', 15), receiver.calls)


if __name__ == '__main__':
    unittest.main()
