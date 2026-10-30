import ctypes
import importlib.util
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('surface_target', Path(__file__).with_name('native-windows-surface-target.py'))
TARGET = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TARGET)


class Mouse(ctypes.Structure):
    _fields_ = [('dx', ctypes.c_long), ('dy', ctypes.c_long), ('mouseData', ctypes.c_ulong),
                ('dwFlags', ctypes.c_ulong), ('time', ctypes.c_ulong), ('dwExtraInfo', ctypes.c_size_t)]


class Input(ctypes.Structure):
    _fields_ = [('type', ctypes.c_ulong), ('mi', Mouse)]


class Keyboard(ctypes.Structure):
    _fields_ = [('wVk', ctypes.c_ushort), ('wScan', ctypes.c_ushort), ('dwFlags', ctypes.c_ulong),
                ('time', ctypes.c_ulong), ('dwExtraInfo', ctypes.c_size_t)]


class KeyboardInput(ctypes.Structure):
    _fields_ = [('type', ctypes.c_ulong), ('ki', Keyboard)]


class FakeUser:
    def __init__(self):
        self.pid, self.thread, self.parent, self.hit = os.getpid(), 7, 10, 20
        self.exists, self.visible, self.enabled, self.iconic = True, True, True, False
        self.sent, self.focus_received, self.calls, self.input = 3, True, 0, []
        self.foreground, self.focus = 30, 0
        self.point_probes = []

    def GetWindowThreadProcessId(self, window, pid):
        pid._obj.value = 100 if window == 30 else self.pid
        return 8 if window == 30 else self.thread

    def IsWindow(self, window): return self.exists
    def IsWindowVisible(self, window): return self.visible
    def IsWindowEnabled(self, window): return self.enabled
    def IsIconic(self, window): return self.iconic
    def GetForegroundWindow(self): return self.foreground
    def GetParent(self, window): return self.parent if window == 20 else 0
    def GetAncestor(self, window, flag): return self.parent if window == 20 else window
    def GetWindow(self, window, flag): return 0

    def GetClassNameW(self, window, name, length):
        name.value = {10: 'Static', 20: 'Edit', 30: 'MyGoWindow'}.get(window, 'Other')
        return len(name.value)

    def GetWindowRect(self, window, rect):
        rect._obj.left, rect._obj.top, rect._obj.right, rect._obj.bottom = 0, 0, 100, 100
        return True

    def GetClientRect(self, window, rect):
        rect._obj.left, rect._obj.top, rect._obj.right, rect._obj.bottom = 0, 0, 560, 42
        return True

    def ClientToScreen(self, window, point):
        point._obj.x += -1800
        point._obj.y += 400
        return True

    def GetSystemMetrics(self, index):
        return {76: -1920, 77: 0, 78: 3840, 79: 1080}[index]

    def WindowFromPoint(self, point):
        self.point_probes.append((point.x, point.y))
        return self.hit

    def SendInput(self, count, inputs, size):
        self.calls += 1
        self.input = [(item.type, item.mi.dx, item.mi.dy, item.mi.dwFlags) for item in inputs]
        if self.focus_received:
            self.foreground, self.focus = 10, 20
        return self.sent


def fixture():
    # Bypass Native.__init__: these tests never load user32 or execute native input.
    native = TARGET.Native.__new__(TARGET.Native)
    native.user, native.kernel = FakeUser(), SimpleNamespace(GetCurrentThreadId=lambda: 7)
    native.desktop, native.window_thread_id, native.alternate, native.alternate_edit = 50, 7, 10, 20
    native.root = Path(__file__).parent / 'synthetic-native-root'
    surface = {'pid': 100, 'image': str(native.root / 'candidate/dzc-surface.exe'), 'running': True,
               'creation_time_100ns': '123', 'original_handle_retained': True}
    native.tree = lambda: [surface]
    native.process = lambda pid: surface
    native.Input, native.Mouse = Input, Mouse
    native.pump_messages = lambda: None
    native.focus_snapshot = lambda: {'foreground_hwnd': native.user.foreground, 'own_thread_focus_hwnd': native.user.focus}
    return native


class AlternateClickTests(unittest.TestCase):
    def test_one_batch_click_prefers_original_left_point_and_real_focus_result(self):
        native = fixture()
        receipt = native.click_alternate()
        self.assertEqual(native.user.calls, 1)
        self.assertEqual(receipt['screen_point'], [-1792, 421])
        self.assertEqual(receipt['hit_hwnd'], 20)
        self.assertEqual(receipt['after'], {'foreground_hwnd': 10, 'own_thread_focus_hwnd': 20})
        self.assertEqual([item[0] for item in native.user.input], [0, 0, 0])
        self.assertEqual([item[3] for item in native.user.input], [0xC001, 2, 4])
        self.assertEqual(list(native.user.input[0][1:3]), receipt['absolute_point'])
        self.assertEqual(len(native.user.point_probes), 4)
        self.assertEqual(receipt['click_plan']['selected_index'], 0)
        self.assertEqual(receipt['click_plan']['recheck']['hit']['parent_hwnd'], 10)
        self.assertNotIn('title', receipt['click_plan']['recheck']['hit'])

    def test_readonly_right_candidate_is_used_when_only_original_left_is_covered(self):
        native = fixture()
        def hit(point):
            native.user.point_probes.append((point.x, point.y))
            return 99 if point.x == -1792 else 20
        native.user.WindowFromPoint = hit
        receipt = native.click_alternate()
        self.assertEqual(receipt['screen_point'], [-1248, 421])
        self.assertEqual(receipt['click_plan']['selected_index'], 1)
        self.assertEqual((len(native.user.point_probes), native.user.calls), (4, 1))

    def test_changed_foreground_during_probes_never_sends_or_replans(self):
        native = fixture()
        def hit(point):
            native.user.point_probes.append((point.x, point.y))
            if len(native.user.point_probes) == 3:
                native.user.foreground = 99
            return 20
        native.user.WindowFromPoint = hit
        with self.assertRaisesRegex(RuntimeError, 'changed before input'):
            native.click_alternate()
        self.assertEqual((len(native.user.point_probes), native.user.calls), (4, 0))

    def test_changed_selected_hit_identity_during_recheck_never_sends(self):
        native = fixture()
        def hit(point):
            native.user.point_probes.append((point.x, point.y))
            if len(native.user.point_probes) == 4:
                native.user.parent = 99
            return 20
        native.user.WindowFromPoint = hit
        with self.assertRaisesRegex(RuntimeError, 'changed before input'):
            native.click_alternate()
        self.assertEqual((len(native.user.point_probes), native.user.calls), (4, 0))

    def test_unbound_or_wrong_caller_thread_never_sends_input(self):
        for change in ('unbound', 'wrong-thread'):
            with self.subTest(change=change):
                native = fixture()
                if change == 'unbound': native.desktop = None
                else: native.kernel.GetCurrentThreadId = lambda: 8
                with self.assertRaises(RuntimeError): native.click_alternate()
                self.assertEqual(native.user.calls, 0)

    def test_foreground_requires_the_retained_original_surface_process(self):
        for field, value in (('original_handle_retained', False), ('running', False), ('image', 'unrelated.exe')):
            with self.subTest(field=field):
                native = fixture()
                native.process(100)[field] = value
                with self.assertRaisesRegex(RuntimeError, 'not foreground'):
                    native.click_alternate()
                self.assertEqual((len(native.user.point_probes), native.user.calls), (0, 0))

    def test_changed_window_identity_or_unavailable_editor_never_sends_input(self):
        for field, value in [('pid', os.getpid() + 1), ('thread', 8), ('parent', 99), ('exists', False),
                             ('visible', False), ('enabled', False), ('iconic', True), ('hit', 99), ('hit', 0)]:
            with self.subTest(field=field, value=value):
                native = fixture()
                setattr(native.user, field, value)
                with self.assertRaises(RuntimeError): native.click_alternate()
                self.assertEqual(native.user.calls, 0)

    def test_partial_input_is_not_retried(self):
        native = fixture()
        native.user.sent = 2
        with self.assertRaisesRegex(RuntimeError, 'incomplete'): native.click_alternate()
        self.assertEqual(native.user.calls, 1)

    def test_missing_focus_fails_without_extra_input_or_activation(self):
        native = fixture()
        native.user.focus_received = False
        with patch.object(TARGET.time, 'monotonic', side_effect=[0, 3]):
            with self.assertRaisesRegex(RuntimeError, 'did not focus'): native.click_alternate()
        self.assertEqual(native.user.calls, 1)

    def test_normalized_points_map_back_to_exact_pixels_with_negative_origin(self):
        for width, height in [(1, 1), (1920, 1080), (3840, 2160), (65536, 65536)]:
            for offset_x, offset_y in [(0, 0), (width // 2, height // 2), (width - 1, height - 1)]:
                x, y = TARGET.absolute_mouse_point(-1920 + offset_x, -1080 + offset_y, -1920, -1080, width, height)
                self.assertTrue(0 <= x <= 65535 and 0 <= y <= 65535)
                self.assertEqual((x * width // 65536, y * height // 65536), (offset_x, offset_y))

    def test_invalid_desktop_or_outside_point_is_rejected(self):
        for args in [(0, 0, 0, 0, 0, 100), (0, 0, 0, 0, 100, 65537),
                     (-1, 0, 0, 0, 100, 100), (100, 0, 0, 0, 100, 100),
                     (0, -1, 0, 0, 100, 100), (0, 100, 0, 0, 100, 100)]:
            with self.subTest(args=args), self.assertRaises(RuntimeError): TARGET.absolute_mouse_point(*args)


class KeyboardDiagnosticTests(unittest.TestCase):
    def fixture(self):
        native = fixture()
        native.target, native.Input, native.Keyboard = 10, KeyboardInput, Keyboard
        native.observe = lambda: {'foreground': {'hwnd': 10}, 'processes': []}
        native.user.foreground, native.user.focus = 10, 20
        native.user.key_reads = []
        native.user.key_phase, native.user.key_accepted = 0, 8

        def key_state(vk):
            native.user.key_reads.append((native.user.key_phase, vk))
            return ({0x10: -32767, 0x11: 1} if native.user.key_phase == 0 else {0x2D: -32768, 0x79: 1}).get(vk, 0)

        def send(count, inputs, size):
            native.user.calls += 1
            native.user.input = [(item.ki.wVk, item.ki.wScan, item.ki.dwFlags) for item in inputs]
            native.user.key_phase = 1
            return native.user.key_accepted

        native.user.GetAsyncKeyState, native.user.SendInput = key_state, send
        return native

    def test_fixed_high_bit_snapshots_do_not_change_or_repeat_the_input_batch(self):
        native = self.fixture()
        with patch.object(TARGET.time, 'monotonic_ns', side_effect=[100, 110, 120, 130, 140, 150]):
            receipt = native.keys({'role': 'target', 'key': 'hotkey'})
        self.assertEqual(native.user.calls, 1)
        self.assertEqual(native.user.input, [(17, 0, 0), (18, 0, 0), (16, 0, 0), (121, 0, 0),
                                             (121, 0, 2), (16, 0, 2), (18, 0, 2), (17, 0, 2)])
        expected_keys = [16, 17, 18, 160, 161, 162, 163, 164, 165, 91, 92, 45, 121]
        self.assertEqual(native.user.key_reads, [(phase, vk) for phase in (0, 1) for vk in expected_keys])
        self.assertEqual((receipt['input_monotonic_ns'], receipt['input_returned_monotonic_ns']), ('120', '130'))
        self.assertEqual((receipt['keyboard_before']['began_monotonic_ns'], receipt['keyboard_before']['completed_monotonic_ns']), ('100', '110'))
        self.assertEqual((receipt['keyboard_after']['began_monotonic_ns'], receipt['keyboard_after']['completed_monotonic_ns']), ('140', '150'))
        self.assertTrue(receipt['keyboard_before']['down']['shift'])
        self.assertFalse(receipt['keyboard_before']['down']['control'], 'The low pressed-since bit is not down state')
        self.assertTrue(receipt['keyboard_after']['down']['insert'])
        self.assertFalse(receipt['keyboard_after']['down']['f10'])
        self.assertIn('not an atomic state or message-delivery acknowledgement', receipt['keyboard_before']['scope'])

    def test_partial_input_preserves_failure_and_never_retries_or_inserts_releases(self):
        native = self.fixture()
        native.user.key_accepted = 7
        with self.assertRaisesRegex(RuntimeError, 'complete original input'):
            native.keys({'role': 'target', 'key': 'hotkey'})
        self.assertEqual(native.user.calls, 1)
        self.assertEqual(len(native.user.input), 8)
        self.assertEqual(len(native.user.key_reads), 26)

    def test_unowned_or_empty_input_does_not_read_keyboard_state(self):
        for request in ({'role': 'unowned', 'key': 'hotkey'}, {'role': 'target'}):
            native = self.fixture()
            with self.assertRaises(RuntimeError): native.keys(request)
            self.assertEqual(native.user.calls, 0)
            self.assertEqual(native.user.key_reads, [])

    def test_owned_key_message_timestamps_preserve_bounded_capture_and_original_procedure(self):
        native = self.fixture()
        native.window_procedures, native.focus_messages = {}, TARGET.deque(maxlen=64)
        native.callback_errors, native.window_callback = TARGET.deque(maxlen=8), lambda callback: callback
        native.user.GetFocus = lambda: native.user.focus
        native.user.GetWindowLongPtrW = lambda window, index: 40
        native.user.SetWindowLongPtrW = lambda window, index, address: 40
        delegated = []
        native.user.CallWindowProcW = lambda *args: delegated.append(args) or 42
        with patch.object(TARGET.ctypes, 'cast', return_value=SimpleNamespace(value=41)):
            native.install_window_procedure(20, 'target-edit')
        callback = native.window_procedures[20]['callback']
        with patch.object(TARGET.time, 'monotonic_ns', side_effect=range(100, 166)):
            for message in [0x0100, 0x0101, 0x0104, 0x0105, 0x0302, *([0x0100] * 61)]:
                self.assertEqual(callback(20, message, 16, 0), 42)
        self.assertEqual(len(native.focus_messages), 64)
        self.assertEqual(len(delegated), 66)
        self.assertEqual(native.focus_messages[-1]['observed_monotonic_ns'], '165')
        self.assertEqual(native.focus_messages[0]['message'], 0x0104)
        self.assertTrue(all(item['role'] == 'target-edit' and item['hwnd'] == 20 for item in native.focus_messages))
        self.assertEqual(list(native.callback_errors), [])


class CleanupTests(unittest.TestCase):
    def test_open_failure_reports_pid_access_error_and_phase_without_retry(self):
        native = TARGET.Native.__new__(TARGET.Native)
        native.handles, calls = {}, []
        native.kernel = SimpleNamespace(OpenProcess=lambda *args: calls.append(args) or 0)
        with patch.object(TARGET.ctypes, 'get_last_error', return_value=5, create=True):
            with self.assertRaisesRegex(RuntimeError, 'Cannot retain') as caught:
                native.process(8123, phase='tree')
        self.assertIn('"pid": 8123', str(caught.exception))
        self.assertIn('"winerror": 5', str(caught.exception))
        self.assertIn('"phase": "tree"', str(caught.exception))
        self.assertEqual(calls, [(0x101011, False, 8123)])
        self.assertEqual(native.handles, {})

    def test_failed_memory_read_is_null_with_its_original_error(self):
        class Memory(ctypes.Structure):
            _fields_ = [('cb', ctypes.c_ulong), ('WorkingSetSize', ctypes.c_size_t), ('PrivateUsage', ctypes.c_size_t)]
        native = TARGET.Native.__new__(TARGET.Native)
        native.Memory, native.handles, native.identities = Memory, {50: 500}, {}
        def times(handle, created, exited, kernel, user):
            created._obj.dwLowDateTime = 123
            return True
        def code(handle, result): result._obj.value = 259; return True
        def image(handle, flags, text, size): text.value = 'owned-image'; return True
        native.kernel = SimpleNamespace(GetProcessTimes=times, GetExitCodeProcess=code, QueryFullProcessImageNameW=image)
        native.psapi = SimpleNamespace(GetProcessMemoryInfo=lambda *args: False)
        with patch.object(TARGET.ctypes, 'get_last_error', return_value=299, create=True):
            receipt = native.process(50)
        self.assertIsNone(receipt['working_set_bytes'])
        self.assertIsNone(receipt['private_commit_bytes'])
        self.assertEqual(receipt['memory_query'], {'succeeded': False, 'winerror': 299})

    def fixture(self, discovery_failure=False, wait_result=0):
        native = TARGET.Native.__new__(TARGET.Native)
        native.root, native.desktop, native.target, native.alternate = Path('fake-root'), 50, 10, 20
        native.handles, native.identities = {50: 500}, {50: {'creation_time_100ns': '123', 'image': 'fake-desktop'}}
        calls, live = [], {500: True}

        def exit_code(handle, code):
            calls.append(('exit', handle))
            code._obj.value = 259 if live[handle] else 1
            return True

        def terminate(handle, code):
            calls.append(('terminate', handle, code))
            if wait_result == 0: live[handle] = False
            return True

        native.kernel = SimpleNamespace(GetExitCodeProcess=exit_code, TerminateProcess=terminate,
            WaitForSingleObject=lambda handle, milliseconds: calls.append(('wait', handle, milliseconds)) or wait_result,
            CloseHandle=lambda handle: calls.append(('close', handle)) or True)
        windows = {10, 20}
        native.user = SimpleNamespace(IsWindow=lambda window: window in windows,
            DestroyWindow=lambda window: calls.append(('destroy', window)) or windows.remove(window) or True)

        def discover():
            calls.append(('discover',))
            if discovery_failure: raise RuntimeError('Cannot retain an owned original process handle')
            return []

        native.tree = discover
        return native, calls

    def test_discovery_failure_still_cleans_retained_handles_and_preserves_failure(self):
        native, calls = self.fixture(discovery_failure=True)
        saved = []
        with patch.object(TARGET, 'save', side_effect=lambda path, value: saved.append(value)), patch.object(TARGET.time, 'monotonic', return_value=100):
            with self.assertRaisesRegex(RuntimeError, 'Cannot retain an owned original process handle'):
                native.cleanup()
        self.assertIn(('terminate', 500, 1), calls)
        self.assertIn(('wait', 500, 5000), calls)
        self.assertIn(('destroy', 10), calls)
        self.assertIn(('destroy', 20), calls)
        self.assertIn(('close', 500), calls)
        self.assertFalse(saved[0]['discovery_complete'])
        self.assertIsNone(saved[0]['remaining_owned_processes'])
        self.assertEqual(saved[0]['forced_cleanup_pids'], [50])

    def test_unconfirmed_termination_stays_unknown_and_cannot_restore(self):
        native, calls = self.fixture(wait_result=258)
        saved = []
        with patch.object(TARGET, 'save', side_effect=lambda path, value: saved.append(value)), patch.object(TARGET.time, 'monotonic', return_value=100):
            with self.assertRaisesRegex(RuntimeError, 'has not exited'): native.cleanup()
        self.assertIsNone(saved[0]['remaining_owned_processes'])
        self.assertEqual(saved[0]['forced_cleanup_pids'], [])
        self.assertIn(('close', 500), calls)

    def test_success_retains_original_cleanup_contract(self):
        native, _ = self.fixture()
        with patch.object(TARGET, 'save'), patch.object(TARGET.time, 'monotonic', return_value=100):
            receipt = native.cleanup()
        self.assertEqual(receipt['remaining_owned_processes'], [])
        self.assertTrue(receipt['target_closed'] and receipt['alternate_target_closed'])
        self.assertEqual(receipt['errors'], [])


if __name__ == '__main__':
    unittest.main()
