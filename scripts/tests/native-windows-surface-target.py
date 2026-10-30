"""Disposable Windows CI target and observations; never imports a product database."""
import argparse
from collections import deque
import ctypes
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import stat
import subprocess
import sys
import threading
import time
import traceback


def require(value, message):
    if not value:
        raise RuntimeError(message)


KEY_STATE_DIAGNOSTICS = (('shift', 0x10), ('control', 0x11), ('alt', 0x12),
                         ('left_shift', 0xA0), ('right_shift', 0xA1),
                         ('left_control', 0xA2), ('right_control', 0xA3),
                         ('left_alt', 0xA4), ('right_alt', 0xA5),
                         ('left_win', 0x5B), ('right_win', 0x5C),
                         ('insert', 0x2D), ('f10', 0x79))


def valid_seed_text(value):
    return isinstance(value, str) and re.fullmatch(r'nativeupgradefixturemarker[a-f0-9]{32}-native-[ab]', value) is not None


def absolute_mouse_point(x, y, left, top, width, height):
    require(0 < width <= 65536 and 0 < height <= 65536, 'Virtual desktop dimensions exceed the click bound')
    require(left <= x < left + width and top <= y < top + height, 'Owned click point is outside the virtual desktop')
    # Aim at the middle of the device pixel in SendInput's 16-bit coordinate space.
    return ((2 * (x - left) + 1) * 65536 // (2 * width),
            (2 * (y - top) + 1) * 65536 // (2 * height))


def alternate_click_candidates(rect):
    require(len(rect) == 4 and all(type(value) is int for value in rect), 'Invalid owned editor rectangle')
    left, top, right, bottom = rect
    require(left == 0 and top == 0 and right > 16 and bottom > 2, 'Owned editor client bounds differ')
    candidates, seen = [], set()
    for name, x in (('left', 8), ('right', right - 8), ('center', right // 2)):
        point = (x, bottom // 2)
        if point not in seen:
            seen.add(point)
            candidates.append({'id': name, 'client_point': list(point)})
    return candidates


def plan_alternate_click(rect, virtual, expected_edit, to_screen, inspect_point):
    require(type(expected_edit) is int and expected_edit > 0, 'An original editor HWND is required')
    require(len(virtual) == 4 and all(type(value) is int for value in virtual), 'Invalid virtual desktop')
    left, top, width, height = virtual
    require(0 < width <= 65536 and 0 < height <= 65536, 'Virtual desktop dimensions exceed the click bound')
    plan = {'client_rect': list(rect), 'virtual_desktop': list(virtual), 'probes': [], 'selected_index': None,
            'scope': 'Read-only point probes and recheck are not atomic with SendInput or window message delivery.'}
    for candidate in alternate_click_candidates(rect):
        probe = {**candidate, 'screen_point': None, 'in_virtual_desktop': False,
                 'absolute_point': None, 'hit': None, 'eligible': False}
        try:
            point = to_screen(candidate['client_point'])
            require(len(point) == 2 and all(type(value) is int for value in point), 'Invalid owned click screen point')
            inside = left <= point[0] < left + width and top <= point[1] < top + height
            probe.update(screen_point=list(point), in_virtual_desktop=inside)
            if inside:
                probe['absolute_point'] = list(absolute_mouse_point(*point, *virtual))
                probe['hit'] = inspect_point(point)
                require(isinstance(probe['hit'], dict), 'Missing read-only hit metadata')
                probe['eligible'] = probe['hit'].get('hwnd') == expected_edit and probe['hit'].get('exists') is True
        except BaseException as error:
            probe['error'] = str(error)
            plan['probes'].append(probe)
            raise RuntimeError('Owned alternate point planning failed: ' + json.dumps(plan)) from error
        if probe['eligible'] and plan['selected_index'] is None:
            plan['selected_index'] = len(plan['probes'])
        plan['probes'].append(probe)
    return plan


def send_planned_click(plan, recheck, send):
    index = plan.get('selected_index')
    if not (type(index) is int and 0 <= index < len(plan['probes'])):
        raise RuntimeError('Owned alternate click has no unoccluded predeclared point: ' + json.dumps(plan))
    selected = plan['probes'][index]
    plan['recheck'] = recheck(selected)
    if not (isinstance(plan['recheck'], dict) and plan['recheck'].get('unchanged') is True):
        raise RuntimeError('Owned alternate click changed before input: ' + json.dumps(plan))
    plan['sent'] = send(selected['absolute_point'])
    if plan['sent'] != 3:
        raise RuntimeError('Single owned mouse input was incomplete: ' + json.dumps(plan))
    return plan['sent']


def save(path, value):
    data = (json.dumps(value, indent=2, ensure_ascii=True) + '\n').encode()
    with path.open('xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    require(path.read_bytes() == data, 'Owned evidence readback differs')


def guarded_root(value, source):
    require(sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
            and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
            and os.environ.get('GITHUB_REPOSITORY') == 'qiyuanhuakai/dezirclip'
            and os.environ.get('GITHUB_SHA') == source,
            'Surface native QA requires the real disposable source-bound GitHub Windows job')
    temporary = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    root = Path(value).absolute()
    require(root != temporary and root.resolve().is_relative_to(temporary), 'Owned runner fixture required')
    for path in (root, *root.parents):
        if path.exists():
            require(not path.is_symlink() and not path.is_junction(), 'Fixture cannot cross a reparse directory')
        if path == temporary:
            break
    require(root.is_dir(), 'Existing fresh fixture root required')
    return root


class Native:
    def __init__(self, root):
        self.root = root
        self.user = ctypes.WinDLL('user32', use_last_error=True)
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        self.psapi = ctypes.WinDLL('psapi', use_last_error=True)
        self.shell = ctypes.WinDLL('shell32', use_last_error=True)
        self.ole = ctypes.WinDLL('ole32', use_last_error=True)
        pointer = ctypes.c_size_t

        class ProcessEntry(ctypes.Structure):
            _fields_ = [('dwSize', W.DWORD), ('cntUsage', W.DWORD), ('th32ProcessID', W.DWORD),
                        ('th32DefaultHeapID', pointer), ('th32ModuleID', W.DWORD), ('cntThreads', W.DWORD),
                        ('th32ParentProcessID', W.DWORD), ('pcPriClassBase', W.LONG), ('dwFlags', W.DWORD),
                        ('szExeFile', W.WCHAR * 260)]

        class Memory(ctypes.Structure):
            _fields_ = [('cb', W.DWORD), ('PageFaultCount', W.DWORD)] + [
                (name, pointer) for name in ('PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
                'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage')]

        class Keyboard(ctypes.Structure):
            _fields_ = [('wVk', W.WORD), ('wScan', W.WORD), ('dwFlags', W.DWORD), ('time', W.DWORD), ('dwExtraInfo', pointer)]

        class Mouse(ctypes.Structure):
            _fields_ = [('dx', W.LONG), ('dy', W.LONG), ('mouseData', W.DWORD), ('dwFlags', W.DWORD), ('time', W.DWORD), ('dwExtraInfo', pointer)]

        class Union(ctypes.Union):
            _fields_ = [('ki', Keyboard), ('mi', Mouse)]

        class Input(ctypes.Structure):
            _anonymous_ = ('data',)
            _fields_ = [('type', W.DWORD), ('data', Union)]

        self.ProcessEntry, self.Memory, self.Input, self.Keyboard = ProcessEntry, Memory, Input, Keyboard
        self.Mouse = Mouse
        declarations = [
            (self.kernel, 'CreateToolhelp32Snapshot', [W.DWORD, W.DWORD], W.HANDLE),
            (self.kernel, 'Process32FirstW', [W.HANDLE, ctypes.POINTER(ProcessEntry)], W.BOOL),
            (self.kernel, 'Process32NextW', [W.HANDLE, ctypes.POINTER(ProcessEntry)], W.BOOL),
            (self.kernel, 'OpenProcess', [W.DWORD, W.BOOL, W.DWORD], W.HANDLE),
            (self.kernel, 'CloseHandle', [W.HANDLE], W.BOOL),
            (self.kernel, 'QueryFullProcessImageNameW', [W.HANDLE, W.DWORD, W.LPWSTR, ctypes.POINTER(W.DWORD)], W.BOOL),
            (self.kernel, 'GetProcessTimes', [W.HANDLE, *([ctypes.POINTER(W.FILETIME)] * 4)], W.BOOL),
            (self.kernel, 'GetExitCodeProcess', [W.HANDLE, ctypes.POINTER(W.DWORD)], W.BOOL),
            (self.kernel, 'TerminateProcess', [W.HANDLE, W.UINT], W.BOOL),
            (self.kernel, 'WaitForSingleObject', [W.HANDLE, W.DWORD], W.DWORD),
            (self.kernel, 'GetModuleHandleW', [W.LPCWSTR], W.HMODULE),
            (self.kernel, 'GetCurrentThreadId', [], W.DWORD),
            (self.user, 'CreateWindowExW', [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, ctypes.c_int, ctypes.c_int,
                                         ctypes.c_int, ctypes.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID], W.HWND),
            (self.user, 'GetForegroundWindow', [], W.HWND),
            (self.user, 'SetForegroundWindow', [W.HWND], W.BOOL),
            (self.user, 'SetFocus', [W.HWND], W.HWND),
            (self.user, 'GetFocus', [], W.HWND),
            (self.user, 'GetAsyncKeyState', [ctypes.c_int], ctypes.c_short),
            (self.user, 'GetWindowLongPtrW', [W.HWND, ctypes.c_int], ctypes.c_ssize_t),
            (self.user, 'SetWindowLongPtrW', [W.HWND, ctypes.c_int, ctypes.c_ssize_t], ctypes.c_ssize_t),
            (self.user, 'CallWindowProcW', [ctypes.c_void_p, W.HWND, W.UINT, W.WPARAM, W.LPARAM], ctypes.c_ssize_t),
            (self.user, 'ShowWindow', [W.HWND, ctypes.c_int], W.BOOL),
            (self.user, 'IsWindowVisible', [W.HWND], W.BOOL),
            (self.user, 'IsWindow', [W.HWND], W.BOOL),
            (self.user, 'IsWindowEnabled', [W.HWND], W.BOOL),
            (self.user, 'IsIconic', [W.HWND], W.BOOL),
            (self.user, 'GetParent', [W.HWND], W.HWND),
            (self.user, 'GetAncestor', [W.HWND, W.UINT], W.HWND),
            (self.user, 'GetWindow', [W.HWND, W.UINT], W.HWND),
            (self.user, 'GetClientRect', [W.HWND, ctypes.POINTER(W.RECT)], W.BOOL),
            (self.user, 'ClientToScreen', [W.HWND, ctypes.POINTER(W.POINT)], W.BOOL),
            (self.user, 'WindowFromPoint', [W.POINT], W.HWND),
            (self.user, 'GetSystemMetrics', [ctypes.c_int], ctypes.c_int),
            (self.user, 'GetWindowThreadProcessId', [W.HWND, ctypes.POINTER(W.DWORD)], W.DWORD),
            (self.user, 'GetWindowTextW', [W.HWND, W.LPWSTR, ctypes.c_int], ctypes.c_int),
            (self.user, 'GetClassNameW', [W.HWND, W.LPWSTR, ctypes.c_int], ctypes.c_int),
            (self.user, 'GetWindowRect', [W.HWND, ctypes.POINTER(W.RECT)], W.BOOL),
            (self.user, 'SetWindowTextW', [W.HWND, W.LPCWSTR], W.BOOL),
            (self.user, 'DestroyWindow', [W.HWND], W.BOOL),
            (self.user, 'PeekMessageW', [ctypes.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT], W.BOOL),
            (self.user, 'TranslateMessage', [ctypes.POINTER(W.MSG)], W.BOOL),
            (self.user, 'DispatchMessageW', [ctypes.POINTER(W.MSG)], ctypes.c_ssize_t),
            (self.user, 'SendInput', [W.UINT, ctypes.POINTER(Input), ctypes.c_int], W.UINT),
            (self.psapi, 'GetProcessMemoryInfo', [W.HANDLE, ctypes.POINTER(Memory), W.DWORD], W.BOOL),
        ]
        for library, name, args, result in declarations:
            function = getattr(library, name)
            function.argtypes, function.restype = args, result
        self.enum_callback = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
        self.user.EnumWindows.argtypes = [self.enum_callback, W.LPARAM]
        self.user.EnumWindows.restype = W.BOOL
        self.handles = {}
        self.identities = {}
        self.desktop = None
        self.window_thread_id = self.kernel.GetCurrentThreadId()
        self.focus_messages = deque(maxlen=64)
        self.callback_errors = deque(maxlen=8)
        self.window_procedures = {}
        self.window_callback = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, W.HWND, W.UINT, W.WPARAM, W.LPARAM)
        self.target = self.user.CreateWindowExW(0, 'STATIC', 'DezirClip Surface QA ' + root.name,
            0x10CF0000, 60, 60, 620, 260, None, None, self.kernel.GetModuleHandleW(None), None)
        require(self.target, 'Could not create the dedicated synthetic target')
        self.edit = self.user.CreateWindowExW(0x200, 'EDIT', '', 0x50000080,
            16, 48, 560, 42, self.target, None, self.kernel.GetModuleHandleW(None), None)
        require(self.edit, 'Could not create the synthetic edit control')
        self.alternate = self.user.CreateWindowExW(0, 'STATIC', 'DezirClip Surface QA alternate ' + root.name,
            0x00CF0000, 100, 360, 620, 260, None, None, self.kernel.GetModuleHandleW(None), None)
        require(self.alternate, 'Could not create the bounded alternate focus target')
        self.alternate_edit = self.user.CreateWindowExW(0x200, 'EDIT', '', 0x50000080,
            16, 48, 560, 42, self.alternate, None, self.kernel.GetModuleHandleW(None), None)
        require(self.alternate_edit, 'Could not create the alternate synthetic edit control')
        self.install_window_procedure(self.target, 'target', self.edit)
        self.install_window_procedure(self.alternate, 'alternate', self.alternate_edit)
        self.install_window_procedure(self.edit, 'target-edit')
        self.install_window_procedure(self.alternate_edit, 'alternate-edit')

    def install_window_procedure(self, window, role, editor=None):
        # STATIC has no editor-focus policy. A normal editor restores its own
        # child focus when Windows activates the top-level window.
        @self.window_callback
        def callback(hwnd, message, wparam, lparam):
            previous = self.window_procedures[int(hwnd)]['original']
            try:
                if message in (0x0006, 0x0007, 0x0008, 0x0100, 0x0101, 0x0104, 0x0105, 0x0302):
                    observed_monotonic_ns = time.monotonic_ns()
                    self.focus_messages.append({'role': role, 'hwnd': int(hwnd), 'message': message,
                                                'wparam': int(wparam), 'own_thread_focus': int(self.user.GetFocus() or 0),
                                                'observed_monotonic_ns': str(observed_monotonic_ns)})
                if message == 0x0007 and editor and self.user.IsWindow(editor):
                    self.user.SetFocus(editor)
                    return 0
            except BaseException as error:
                self.callback_errors.append(str(error))
            return self.user.CallWindowProcW(previous, hwnd, message, wparam, lparam)
        address = ctypes.cast(callback, ctypes.c_void_p).value
        original = self.user.GetWindowLongPtrW(window, -4)
        require(original, 'Owned target original window procedure unavailable')
        self.window_procedures[int(window)] = {'original': original, 'callback': callback}
        previous = self.user.SetWindowLongPtrW(window, -4, address)
        require(previous == original, 'Owned target window subclass failed or changed concurrently')

    def pump_messages(self):
        message = W.MSG()
        while self.user.PeekMessageW(ctypes.byref(message), None, 0, 0, 1):
            self.user.TranslateMessage(ctypes.byref(message))
            self.user.DispatchMessageW(ctypes.byref(message))

    def focus_snapshot(self):
        require(not self.callback_errors, 'Owned window callback failed: ' + '; '.join(self.callback_errors))
        return {'foreground_hwnd': int(self.user.GetForegroundWindow() or 0),
                'own_thread_focus_hwnd': int(self.user.GetFocus() or 0),
                'target_hwnd': int(self.target), 'edit_hwnd': int(self.edit),
                'messages': list(self.focus_messages)}

    def target_preflight(self, expected):
        require(self.desktop is None, 'Independent target calibration must precede Desktop binding')
        require(valid_seed_text(expected), 'Only the original synthetic clipboard seed may calibrate the target')
        require(self.target_text() == '', 'Calibration requires the original empty target')
        self.activate()
        self.activate(alternate=True)
        # Exercise the same top-level-only restoration used by the product.
        # No late SetFocus(edit), target reactivation, or extra paste repairs it.
        require(self.user.SetForegroundWindow(self.target), 'Target calibration restoration denied')
        deadline = time.monotonic() + 2
        while True:
            self.pump_messages()
            focus = self.focus_snapshot()
            if focus['foreground_hwnd'] == int(self.target) and focus['own_thread_focus_hwnd'] == int(self.edit):
                break
            require(time.monotonic() < deadline, 'Restored target did not regain its editor focus')
            time.sleep(.01)
        sent = self.keys({'role': 'target', 'key': 'paste'})
        deadline = time.monotonic() + 2
        while True:
            self.pump_messages()
            actual = self.target_text()
            if actual == expected:
                break
            require(time.monotonic() < deadline, 'Independent target Ctrl+V calibration failed')
            time.sleep(.01)
        receipt = {'focus_before_input': focus, 'input': sent, 'actual_target_text': actual,
                   'paste_attempts': 1, 'focus_after_input': self.focus_snapshot(),
                   'restoration': 'SetForegroundWindow(top-level), ordinary WM_SETFOCUS editor forwarding'}
        require(self.user.SetWindowTextW(self.edit, ''), 'Could not clear the calibration text')
        require(self.target_text() == '', 'Calibration text remained in the product target')
        return receipt

    def process(self, pid):
        if pid not in self.handles:
            handle = self.kernel.OpenProcess(0x00100000 | 0x1000 | 0x10 | 1, False, pid)
            require(handle, 'Cannot retain an owned original process handle')
            self.handles[pid] = handle
        handle = self.handles[pid]
        created, exited, kernel, user = W.FILETIME(), W.FILETIME(), W.FILETIME(), W.FILETIME()
        require(self.kernel.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel), ctypes.byref(user)), 'Owned process times unavailable')
        code = W.DWORD()
        require(self.kernel.GetExitCodeProcess(handle, ctypes.byref(code)), 'Owned original process exit unavailable')
        size = W.DWORD(32768)
        name = ctypes.create_unicode_buffer(size.value)
        queried = bool(self.kernel.QueryFullProcessImageNameW(handle, 0, name, ctypes.byref(size)))
        require(queried or code.value != 259 and pid in self.identities, 'Owned image unavailable')
        image = name.value if queried else self.identities[pid]['image']
        value = lambda t: (t.dwHighDateTime << 32) | t.dwLowDateTime
        identity = {'creation_time_100ns': str(value(created)), 'image': image}
        require(pid not in self.identities or self.identities[pid] == identity, 'Original process identity changed')
        self.identities[pid] = identity
        memory = self.Memory()
        memory.cb = ctypes.sizeof(memory)
        measured = bool(self.psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb))
        return {'pid': pid, **identity,
                'running': code.value == 259, 'exit_code': None if code.value == 259 else code.value,
                'cpu_kernel_100ns': value(kernel), 'cpu_user_100ns': value(user),
                'working_set_bytes': memory.WorkingSetSize if measured else None,
                'private_commit_bytes': memory.PrivateUsage if measured else None,
                'original_handle_retained': True}

    def bind(self, pid):
        require(self.desktop is None and isinstance(pid, int) and pid > 0, 'One original Desktop binding required')
        info = self.process(pid)
        require(info['running'] and Path(info['image']).resolve() == (self.root / 'candidate/dezirclip.exe').resolve(), 'Desktop original image differs')
        self.desktop = pid
        return info

    def tree(self):
        require(self.desktop, 'Desktop binding required')
        snapshot = self.kernel.CreateToolhelp32Snapshot(2, 0)
        require(snapshot and snapshot != ctypes.c_void_p(-1).value, 'Process snapshot unavailable')
        parents = {}
        try:
            entry = self.ProcessEntry()
            entry.dwSize = ctypes.sizeof(entry)
            found = self.kernel.Process32FirstW(snapshot, ctypes.byref(entry))
            while found:
                parents[entry.th32ProcessID] = entry.th32ParentProcessID
                found = self.kernel.Process32NextW(snapshot, ctypes.byref(entry))
        finally:
            self.kernel.CloseHandle(snapshot)
        owned = {self.desktop} if self.process(self.desktop)['running'] else set()
        while True:
            more = {pid for pid, parent in parents.items() if parent in owned}
            if more <= owned:
                break
            owned |= more
        result = []
        for pid in sorted(owned | set(self.handles)):
            try:
                result.append(self.process(pid))
            except RuntimeError:
                if pid in parents:
                    raise
        return result

    def observe(self):
        # Before Desktop starts, calibration may inspect only this fixture's
        # windows. Product observations still use the original bound tree.
        processes = self.tree() if self.desktop is not None else []
        owned = {p['pid'] for p in processes if p['running']} | {os.getpid()}
        windows = []
        @self.enum_callback
        def collect(window, _):
            pid = W.DWORD()
            thread = self.user.GetWindowThreadProcessId(window, ctypes.byref(pid))
            if pid.value not in owned:
                return True
            name, cls = ctypes.create_unicode_buffer(512), ctypes.create_unicode_buffer(256)
            self.user.GetWindowTextW(window, name, len(name))
            self.user.GetClassNameW(window, cls, len(cls))
            rect = W.RECT()
            ok = bool(self.user.GetWindowRect(window, ctypes.byref(rect)))
            windows.append({'hwnd': int(window), 'pid': pid.value, 'thread_id': thread, 'title': name.value,
                            'class_name': cls.value, 'visible': bool(self.user.IsWindowVisible(window)),
                            'rect': {'x': rect.left, 'y': rect.top, 'width': rect.right - rect.left, 'height': rect.bottom - rect.top} if ok else None})
            return True
        require(self.user.EnumWindows(collect, 0), 'Owned window enumeration failed')
        fg = self.user.GetForegroundWindow()
        foreground = next((w for w in windows if w['hwnd'] == fg), None)
        return {'processes': processes, 'windows': windows, 'foreground': foreground or {'kind': 'other-uninspected'},
                'target_hwnd': int(self.target), 'edit_hwnd': int(self.edit),
                'alternate_hwnd': int(self.alternate),
                'target_focus': self.focus_snapshot(),
                'memory_scope': 'All observed Desktop descendants; working set/private commit are not PSS or USS.'}

    def activate(self, alternate=False):
        window, edit = (self.alternate, self.alternate_edit) if alternate else (self.target, self.edit)
        before = self.focus_snapshot()
        self.user.ShowWindow(window, 5)
        activated = bool(self.user.SetForegroundWindow(window))
        require(activated, 'Synthetic target activation was denied: ' + json.dumps({
            'role': 'alternate' if alternate else 'target', 'requested_hwnd': int(window),
            'before': before, 'after': self.focus_snapshot()}))
        self.user.SetFocus(edit)
        require(self.user.GetForegroundWindow() == window, 'Synthetic target is not foreground')
        return {'foreground_target': True, 'hwnd': int(window), 'alternate': alternate, 'target_focus': self.focus_snapshot()}

    def click_window_metadata(self, window):
        window = int(window or 0)
        pid, cls, rect = W.DWORD(), ctypes.create_unicode_buffer(256), W.RECT()
        exists = bool(window and self.user.IsWindow(window))
        thread = self.user.GetWindowThreadProcessId(window, ctypes.byref(pid)) if exists else 0
        if exists:
            self.user.GetClassNameW(window, cls, len(cls))
        positioned = exists and bool(self.user.GetWindowRect(window, ctypes.byref(rect)))
        return {'hwnd': window, 'exists': exists, 'pid': pid.value, 'thread_id': thread, 'class_name': cls.value,
                'parent_hwnd': int(self.user.GetParent(window) or 0) if exists else 0,
                'root_hwnd': int(self.user.GetAncestor(window, 2) or 0) if exists else 0,
                'owner_hwnd': int(self.user.GetWindow(window, 4) or 0) if exists else 0,
                'visible': bool(exists and self.user.IsWindowVisible(window)),
                'enabled': bool(exists and self.user.IsWindowEnabled(window)),
                'iconic': bool(exists and self.user.IsIconic(window)),
                'rect': [rect.left, rect.top, rect.right, rect.bottom] if positioned else None}

    def click_alternate(self):
        require(self.desktop is not None, 'External click requires the original Desktop binding')
        require(self.kernel.GetCurrentThreadId() == self.window_thread_id, 'Owned click must run on the original window thread')

        def state():
            rect = W.RECT()
            require(self.user.GetClientRect(self.alternate_edit, ctypes.byref(rect)), 'Owned editor client rectangle unavailable')
            return {'alternate': self.click_window_metadata(self.alternate),
                    'editor': self.click_window_metadata(self.alternate_edit),
                    'foreground': self.click_window_metadata(self.user.GetForegroundWindow()),
                    'client_rect': [rect.left, rect.top, rect.right, rect.bottom],
                    'virtual_desktop': [self.user.GetSystemMetrics(index) for index in (76, 77, 78, 79)]}

        def screen_point(client_point):
            point = W.POINT(*client_point)
            require(self.user.ClientToScreen(self.alternate_edit, ctypes.byref(point)), 'Owned click screen coordinates unavailable')
            return [point.x, point.y]

        def hit_point(point):
            return self.click_window_metadata(self.user.WindowFromPoint(W.POINT(*point)))

        initial = state()
        for name, cls in (('alternate', 'static'), ('editor', 'edit')):
            window = initial[name]
            require(window['exists'] and window['pid'] == os.getpid() and window['thread_id'] == self.window_thread_id
                    and window['class_name'].casefold() == cls and window['visible'] and window['enabled']
                    and not window['iconic'] and window['rect'] is not None, 'Original alternate window identity unavailable: ' + json.dumps(initial))
        require(initial['editor']['parent_hwnd'] == int(self.alternate)
                and initial['editor']['root_hwnd'] == int(self.alternate)
                and initial['alternate']['root_hwnd'] == int(self.alternate), 'Original alternate editor parent changed: ' + json.dumps(initial))
        foreground = initial['foreground']
        surface = next((item for item in self.tree() if item['pid'] == foreground['pid'] and item['running']
                        and Path(item['image']).resolve() == (self.root / 'candidate/dzc-surface.exe').resolve()), None)
        require(surface is not None and surface.get('original_handle_retained') is True
                and foreground['exists'] and foreground['visible'] and foreground['class_name']
                and foreground['rect'] is not None, 'Original native Surface is not foreground before the external click: ' + json.dumps(initial))
        process_keys = ('pid', 'creation_time_100ns', 'image', 'running', 'original_handle_retained')
        surface_identity = {key: surface[key] for key in process_keys}
        before = self.focus_snapshot()
        require(before['foreground_hwnd'] == foreground['hwnd'], 'Foreground changed before point planning: ' + json.dumps(initial))
        plan = plan_alternate_click(initial['client_rect'], initial['virtual_desktop'], int(self.alternate_edit), screen_point, hit_point)
        plan.update(initial_state=initial, foreground_process=surface_identity, before=before)

        def recheck(selected):
            try:
                process = self.process(surface_identity['pid'])
                identity = {key: process[key] for key in process_keys}
                current = state()
                current_point = screen_point(selected['client_point'])
                focus = self.focus_snapshot()
                hit = hit_point(current_point)
                return {'state': current, 'screen_point': current_point, 'hit': hit, 'foreground_process': identity, 'focus': focus,
                        'unchanged': current == initial and current_point == selected['screen_point']
                        and hit == current['editor'] and identity == surface_identity
                        and focus['foreground_hwnd'] == foreground['hwnd']}
            except BaseException as error:
                return {'unchanged': False, 'error': str(error)}

        def send(absolute):
            inputs = (self.Input * 3)()
            inputs[0].mi = self.Mouse(*absolute, 0, 0x8000 | 0x4000 | 0x0001, 0, 0)
            inputs[1].mi = self.Mouse(0, 0, 0, 0x0002, 0, 0)
            inputs[2].mi = self.Mouse(0, 0, 0, 0x0004, 0, 0)
            return self.user.SendInput(3, inputs, ctypes.sizeof(self.Input))

        sent = send_planned_click(plan, recheck, send)
        selected = plan['probes'][plan['selected_index']]
        receipt = {'method': 'one-native-mouse-click', 'sent': sent, 'hwnd': int(self.alternate),
                   'edit_hwnd': int(self.alternate_edit), 'pid': os.getpid(), 'thread_id': self.window_thread_id,
                   'screen_point': selected['screen_point'], 'absolute_point': selected['absolute_point'],
                   'virtual_desktop': initial['virtual_desktop'], 'hit_hwnd': int(self.alternate_edit), 'before': before, 'click_plan': plan}
        deadline = time.monotonic() + 2
        while True:
            self.pump_messages()
            after = self.focus_snapshot()
            if after['foreground_hwnd'] == int(self.alternate) and after['own_thread_focus_hwnd'] == int(self.alternate_edit):
                return {**receipt, 'after': after}
            require(time.monotonic() < deadline, 'Single owned click did not focus its editor: ' + json.dumps({**receipt, 'after': after}))
            time.sleep(.01)

    def target_text(self):
        text = ctypes.create_unicode_buffer(4097)
        self.user.GetWindowTextW(self.edit, text, len(text))
        require(len(text.value) < 4096, 'Synthetic target text exceeded its bound')
        return text.value

    def keyboard_snapshot(self):
        began = time.monotonic_ns()
        down = {name: bool(self.user.GetAsyncKeyState(vk) & 0x8000) for name, vk in KEY_STATE_DIAGNOSTICS}
        return {'began_monotonic_ns': str(began), 'completed_monotonic_ns': str(time.monotonic_ns()),
                'down': down, 'scope': 'Fixed modifiers, Insert and F10 high-bit observations only; sequential reads, not an atomic state or message-delivery acknowledgement. Zero may also mean unavailable state.'}

    def keys(self, request):
        observation = self.observe()
        foreground = observation['foreground']
        role = request.get('role')
        if role == 'target':
            require(foreground.get('hwnd') == int(self.target), 'Input target is not the dedicated foreground target')
        elif role == 'surface':
            require(any(p['pid'] == foreground.get('pid') and Path(p['image']).resolve() == (self.root / 'candidate/dzc-surface.exe').resolve() for p in observation['processes']), 'Native Surface is not foreground')
        else:
            raise RuntimeError('Only explicit target/Surface input is permitted')
        inputs = []
        def key(vk, scan=0, flags=0):
            item = self.Input()
            item.type = 1
            item.ki = self.Keyboard(vk, scan, flags, 0, 0)
            inputs.append(item)
        chords = {'hotkey': [0x11, 0x12, 0x10, 0x79], 'select-all': [0x11, 0x41],
                  'escape': [0x1B], 'enter': [0x0D], 'down': [0x28], 'up': [0x26], 'paste': [0x11, 0x56]}
        if request.get('key'):
            require(request['key'] in chords, 'Unrecognized bounded key command')
            if request['key'] == 'paste':
                require(role == 'target' and self.user.GetFocus() == self.edit, 'Calibration paste requires the owned foreground editor')
            keys = chords[request['key']]
            for vk in keys:
                key(vk)
            for vk in reversed(keys):
                key(vk, flags=2)
        if request.get('text') is not None:
            text = request['text']
            require(isinstance(text, str) and 0 < len(text) <= 128 and text.isascii() and text.isprintable(), 'Only bounded synthetic query text is allowed')
            for value in text.encode('utf-16-le')[::2]:
                key(0, value, 4)
                key(0, value, 6)
        require(inputs, 'Native input must not be empty')
        array = (self.Input * len(inputs))(*inputs)
        keyboard_before = self.keyboard_snapshot()
        input_monotonic_ns = time.monotonic_ns()
        sent = self.user.SendInput(len(inputs), array, ctypes.sizeof(self.Input))
        input_returned_monotonic_ns = time.monotonic_ns()
        keyboard_after = self.keyboard_snapshot()
        require(sent == len(inputs), 'SendInput did not accept the complete original input')
        return {'sent': sent, 'foreground_before': foreground, 'native_input': True, 'target_focus': self.focus_snapshot(),
                'input_monotonic_ns': str(input_monotonic_ns), 'input_returned_monotonic_ns': str(input_returned_monotonic_ns),
                'keyboard_before': keyboard_before, 'keyboard_after': keyboard_after}

    def accessibility(self, pid):
        observation = self.observe()
        window = next(w for w in observation['windows'] if w['pid'] == pid and w['visible'])
        require(any(p['pid'] == pid and Path(p['image']).resolve() == (self.root / 'candidate/dzc-surface.exe').resolve() for p in observation['processes']), 'UIA is limited to the actual owned Surface')
        script = r'''[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]([long]$env:DZC_QA_UIA_HWND))
if (-not $root) { throw 'Owned UIA root unavailable' }
$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
if ($items.Count -gt 512) { throw 'Owned UIA tree exceeded bound' }
$rows = @()
foreach ($item in $items) {
  $name = $item.Current.Name
  if ($name.Length -gt 1024) { throw 'Owned UIA name exceeded bound' }
  $value = $null
  $pattern = $null
  if ($item.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
    $value = $pattern.Current.Value
    if ($value.Length -gt 128) { throw 'Owned input value exceeded bound' }
  }
  $rows += @{ name=$name; value=$value; control_type=$item.Current.ControlType.ProgrammaticName; enabled=$item.Current.IsEnabled; offscreen=$item.Current.IsOffscreen }
}
ConvertTo-Json -InputObject $rows -Depth 5 -Compress
'''
        env = {**os.environ, 'DZC_QA_UIA_HWND': str(window['hwnd'])}
        result = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', script],
            env=env, capture_output=True, timeout=8, creationflags=0x08000000)
        require(result.returncode == 0 and len(result.stdout) <= 256 * 1024, 'Actual owned UIA query failed: ' + result.stderr.decode(errors='replace')[:1000])
        return {'hwnd': window['hwnd'], 'pid': pid, 'elements': json.loads(result.stdout.decode('utf-8-sig') or '[]')}

    def cleanup(self):
        forced = []
        if self.desktop:
            current = self.tree()
            for item in reversed(current):
                if self.process(item['pid'])['running']:
                    require(self.kernel.TerminateProcess(self.handles[item['pid']], 1), 'Owned cleanup termination failed')
                    require(self.kernel.WaitForSingleObject(self.handles[item['pid']], 5000) == 0, 'Owned original process has not exited')
                    forced.append(item['pid'])
        final_processes = [self.process(pid) for pid in self.handles]
        remaining = [item for item in final_processes if item['running']]
        require(not remaining, 'Owned processes remain alive')
        if self.target and self.user.IsWindow(self.target):
            require(self.user.DestroyWindow(self.target), 'Synthetic target close failed')
        if self.alternate and self.user.IsWindow(self.alternate):
            require(self.user.DestroyWindow(self.alternate), 'Alternate synthetic target close failed')
        for handle in self.handles.values():
            self.kernel.CloseHandle(handle)
        return {'remaining_owned_processes': remaining, 'forced_cleanup_pids': forced,
                'final_owned_processes': final_processes,
                'target_closed': not bool(self.user.IsWindow(self.target)),
                'alternate_target_closed': not bool(self.user.IsWindow(self.alternate)), 'natural_application_shutdown_tested': False}


def bootstrap(root, port, source):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    import release_windows_webview as webview
    # Reuse the original policy transaction with this actual CI context; its tag-only CLI is not called.
    context = {'mode': 'phase6_native_ci', 'source_revision': source, 'run_id': os.environ['GITHUB_RUN_ID'],
               'run_attempt': os.environ['GITHUB_RUN_ATTEMPT'], 'event_name': os.environ['GITHUB_EVENT_NAME']}
    api = webview.NativeWindows()
    state = root / 'webview-policy.json'
    redirects = []
    result = {'kind': 'phase6_owned_bootstrap', 'context': context, 'redirects': redirects}
    try:
        from release_identity_windows_paths import FOLDERS, query_known_folder
        observation = query_known_folder(FOLDERS['roaming'][0])
        result['roaming_known_folder'] = observation
        require(observation['hresult'] == 0 and observation['runtime_path'], 'Roaming KnownFolder query failed')
        roaming = Path(observation['runtime_path'])
        require(roaming.is_dir() and not roaming.is_symlink() and not roaming.is_junction(), 'A regular KnownFolder is required')
        roaming = roaming.resolve(strict=True)
        for identifier in ('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard'):
            directory = roaming / identifier
            require(not directory.exists() and not directory.is_symlink(), 'Existing application profile or redirect; fresh runner required')
        for identifier in ('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard'):
            directory = roaming / identifier
            directory.mkdir()
            identity = directory.stat()
            record = {'directory': str(directory), 'dev': identity.st_dev, 'ino': identity.st_ino, 'redirect_created': False}
            redirects.append(record)
            path = directory / 'datapath.txt'
            data = str(root / 'profile').encode('utf-8')
            with path.open('xb') as handle:
                handle.write(data); handle.flush(); os.fsync(handle.fileno())
            require(path.read_bytes() == data, 'Synthetic redirect readback differs')
            record.update(redirect_created=True, path=str(path), sha256=hashlib.sha256(data).hexdigest())
        webview.configure(api, state, root, root / 'webview', port, context)
        result['policy_mode'] = webview.read_json(state)[0]['mode']
        save(root / 'bootstrap.json', result)
        return webview, api, state, redirects
    except BaseException:
        result['error'] = traceback.format_exc()
        save(root / 'bootstrap-failed.json', result)
        restore_bootstrap(webview, api, state, redirects, root)
        raise


def restore_bootstrap(webview, api, state, redirects, root):
    errors = []
    if state.exists():
        try:
            webview.restore(api, state, webview.read_json(state)[0])
        except BaseException:
            errors.append(traceback.format_exc())
    for item in reversed(redirects):
        try:
            directory = Path(item['directory'])
            require(not directory.is_symlink() and not directory.is_junction(), 'Original owned redirect directory changed')
            identity = directory.stat()
            require((identity.st_dev, identity.st_ino) == (item['dev'], item['ino']), 'Redirect directory identity changed')
            if item['redirect_created']:
                path = Path(item['path'])
                require(not path.is_symlink() and hashlib.sha256(path.read_bytes()).hexdigest() == item['sha256'], 'Owned redirect changed; leave it intact')
                path.unlink()
            directory.rmdir()
        except BaseException:
            errors.append(traceback.format_exc())
    save(root / 'bootstrap-cleanup.json', {'errors': errors, 'only_created_redirects_removed': not errors})
    require(not errors, 'Owned bootstrap restoration failed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--port', required=True, type=int)
    args = parser.parse_args()
    root = guarded_root(args.root, args.source)
    require(1024 <= args.port <= 65535, 'Explicit private loopback port required')
    state = bootstrap(root, args.port, args.source)
    native = None
    requests = queue.Queue(maxsize=32)
    def reader():
        try:
            for line in sys.stdin:
                require(len(line) <= 8192, 'QA control input exceeded its bound')
                requests.put(json.loads(line))
        finally:
            requests.put({'id': None, 'operation': 'stop'})
    failure = None
    try:
        native = Native(root)
        threading.Thread(target=reader, daemon=True).start()
        print(json.dumps({'event': 'ready', 'target_pid': os.getpid(), 'target_hwnd': int(native.target)}), flush=True)
        deadline = time.monotonic() + 300
        stopped = False
        while not stopped:
            require(time.monotonic() < deadline, 'Native QA control deadline expired')
            native.pump_messages()
            try:
                request = requests.get(timeout=0.01)
            except queue.Empty:
                continue
            operation = request.get('operation')
            try:
                if operation == 'stop':
                    stopped = True
                    value = {'stopping': True}
                elif operation == 'bind':
                    value = native.bind(request.get('pid'))
                    document = state[0].read_json(state[2])[0]
                    state[0].inspect(state[1], state[2], document, request['pid'], root / 'candidate/dezirclip.exe')
                elif operation == 'observe': value = native.observe()
                elif operation == 'activate-target': value = native.activate()
                elif operation == 'activate-alternate': value = native.activate(alternate=True)
                elif operation == 'click-alternate': value = native.click_alternate()
                elif operation == 'target-text': value = {'text': native.target_text(), 'target_focus': native.focus_snapshot()}
                elif operation == 'target-preflight': value = native.target_preflight(request.get('expected_text'))
                elif operation == 'clear-target':
                    require(native.user.SetWindowTextW(native.edit, ''), 'Cannot clear the synthetic target')
                    value = {'text': native.target_text()}
                elif operation == 'keys': value = native.keys(request)
                elif operation == 'accessibility': value = native.accessibility(request.get('pid'))
                else: raise RuntimeError('Unknown QA control operation')
                print(json.dumps({'id': request.get('id'), 'value': value}), flush=True)
            except BaseException as error:
                print(json.dumps({'id': request.get('id'), 'error': str(error)}), flush=True)
    except BaseException:
        failure = traceback.format_exc()
        raise
    finally:
        cleanup = {'primary_error': failure}
        try:
            cleanup['native'] = native.cleanup() if native else {'not_created': True}
        except BaseException:
            cleanup['native_error'] = traceback.format_exc()
        # Never restore a policy while an owned application may still be running.
        if 'native_error' not in cleanup:
            try: restore_bootstrap(*state, root)
            except BaseException: cleanup['bootstrap_error'] = traceback.format_exc()
        save(root / 'native-cleanup.json', cleanup)
        require('native_error' not in cleanup and 'bootstrap_error' not in cleanup, 'Original owned cleanup failed')


if __name__ == '__main__':
    main()
