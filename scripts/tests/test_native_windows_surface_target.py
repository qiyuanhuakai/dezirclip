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

    def GetWindowThreadProcessId(self, window, pid):
        pid._obj.value = self.pid
        return self.thread

    def IsWindow(self, window): return self.exists
    def IsWindowVisible(self, window): return self.visible
    def IsWindowEnabled(self, window): return self.enabled
    def IsIconic(self, window): return self.iconic
    def GetParent(self, window): return self.parent

    def GetClientRect(self, window, rect):
        rect._obj.left, rect._obj.top, rect._obj.right, rect._obj.bottom = 0, 0, 560, 42
        return True

    def ClientToScreen(self, window, point):
        point._obj.x += -1800
        point._obj.y += 400
        return True

    def GetSystemMetrics(self, index):
        return {76: -1920, 77: 0, 78: 3840, 79: 1080}[index]

    def WindowFromPoint(self, point): return self.hit

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
    native.Input, native.Mouse = Input, Mouse
    native.pump_messages = lambda: None
    native.focus_snapshot = lambda: {'foreground_hwnd': native.user.foreground, 'own_thread_focus_hwnd': native.user.focus}
    return native


class AlternateClickTests(unittest.TestCase):
    def test_one_batch_click_uses_fixed_owned_point_and_real_focus_result(self):
        native = fixture()
        receipt = native.click_alternate()
        self.assertEqual(native.user.calls, 1)
        self.assertEqual(receipt['screen_point'], [-1792, 421])
        self.assertEqual(receipt['hit_hwnd'], 20)
        self.assertEqual(receipt['after'], {'foreground_hwnd': 10, 'own_thread_focus_hwnd': 20})
        self.assertEqual([item[0] for item in native.user.input], [0, 0, 0])
        self.assertEqual([item[3] for item in native.user.input], [0xC001, 2, 4])
        self.assertEqual(list(native.user.input[0][1:3]), receipt['absolute_point'])

    def test_unbound_or_wrong_caller_thread_never_sends_input(self):
        for change in ('unbound', 'wrong-thread'):
            with self.subTest(change=change):
                native = fixture()
                if change == 'unbound': native.desktop = None
                else: native.kernel.GetCurrentThreadId = lambda: 8
                with self.assertRaises(RuntimeError): native.click_alternate()
                self.assertEqual(native.user.calls, 0)

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


if __name__ == '__main__':
    unittest.main()
