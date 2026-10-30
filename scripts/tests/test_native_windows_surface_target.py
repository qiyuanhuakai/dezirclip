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


if __name__ == '__main__':
    unittest.main()
