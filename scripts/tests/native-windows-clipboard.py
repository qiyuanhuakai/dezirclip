"""Write synthetic acceptance text with a bounded Win32 clipboard acquisition."""
import json
import os
from pathlib import Path
import re
import sys
import time


def validate_environment(environment, platform_name):
    if (platform_name != 'win32' or environment.get('GITHUB_ACTIONS') != 'true'
            or environment.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or not environment.get('RUNNER_TEMP') or not Path(environment['RUNNER_TEMP']).is_dir()):
        raise RuntimeError('Native clipboard acceptance requires a disposable GitHub-hosted Windows runner')
    text = environment.get('DEZIRCLIP_NATIVE_CAPTURE_TEXT', '')
    if not re.fullmatch(r'nativeupgradefixturemarker[a-f0-9]{32}-native-[ab]', text):
        raise RuntimeError('Only the synthetic upgrade marker may be written')
    return text


def observe(phase, **fields):
    print(json.dumps({'phase': phase, **fields}), flush=True)


def write_native(text):
    import ctypes
    from ctypes import wintypes
    user = ctypes.WinDLL('user32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    user.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                   ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.HWND,
                                   wintypes.HMENU, wintypes.HINSTANCE, wintypes.LPVOID]
    user.CreateWindowExW.restype = wintypes.HWND
    user.DestroyWindow.argtypes = [wintypes.HWND]
    user.DestroyWindow.restype = wintypes.BOOL
    user.OpenClipboard.argtypes = [wintypes.HWND]
    user.OpenClipboard.restype = wintypes.BOOL
    user.CloseClipboard.argtypes = []
    user.CloseClipboard.restype = wintypes.BOOL
    user.EmptyClipboard.argtypes = []
    user.EmptyClipboard.restype = wintypes.BOOL
    user.SetClipboardData.argtypes = [wintypes.UINT, wintypes.HANDLE]
    user.SetClipboardData.restype = wintypes.HANDLE
    user.GetOpenClipboardWindow.argtypes = []
    user.GetOpenClipboardWindow.restype = wintypes.HWND
    user.GetClipboardSequenceNumber.argtypes = []
    user.GetClipboardSequenceNumber.restype = wintypes.DWORD
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.GetWindowThreadProcessId.restype = wintypes.DWORD
    kernel.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
    kernel.GlobalAlloc.restype = wintypes.HGLOBAL
    kernel.GlobalLock.argtypes = [wintypes.HGLOBAL]
    kernel.GlobalLock.restype = wintypes.LPVOID
    kernel.GlobalUnlock.argtypes = [wintypes.HGLOBAL]
    kernel.GlobalUnlock.restype = wintypes.BOOL
    kernel.GlobalFree.argtypes = [wintypes.HGLOBAL]
    kernel.GlobalFree.restype = wintypes.HGLOBAL
    observe('started', pid=os.getpid(), sequence=user.GetClipboardSequenceNumber())
    window = user.CreateWindowExW(0, 'STATIC', 'DezirClip synthetic clipboard fixture', 0,
                                 0, 0, 0, 0, wintypes.HWND(-3), None, None, None)
    if not window:
        raise RuntimeError('Synthetic clipboard owner window failed: ' + str(ctypes.get_last_error()))
    memory, opened, transferred = None, False, False
    try:
        payload = text.encode('utf-16-le') + b'\0\0'
        memory = kernel.GlobalAlloc(0x0002, len(payload))
        if not memory:
            raise RuntimeError('Synthetic clipboard allocation failed')
        address = kernel.GlobalLock(memory)
        if not address:
            raise RuntimeError('Synthetic clipboard allocation lock failed')
        try:
            ctypes.memmove(address, payload, len(payload))
        finally:
            kernel.GlobalUnlock(memory)
        observe('acquiring', timeout_seconds=5)
        started, last_owner, attempts = time.monotonic(), None, 0
        while True:
            attempts += 1
            if user.OpenClipboard(window):
                opened = True
                observe('opened', attempts=attempts, elapsed_ms=round((time.monotonic() - started) * 1000))
                break
            error = ctypes.get_last_error()
            owner_window = user.GetOpenClipboardWindow()
            owner_pid = wintypes.DWORD()
            if owner_window:
                user.GetWindowThreadProcessId(owner_window, ctypes.byref(owner_pid))
            owner = (int(owner_window or 0), owner_pid.value)
            elapsed = time.monotonic() - started
            if owner != last_owner or elapsed >= 5:
                observe('occupied', elapsed_ms=round(elapsed * 1000), attempts=attempts,
                        owner_window=owner[0], owner_pid=owner[1], last_error=error)
                last_owner = owner
            if elapsed >= 5:
                raise RuntimeError('Native OpenClipboard exceeded its five second acquisition budget')
            time.sleep(.02)
        if not user.EmptyClipboard():
            raise RuntimeError('Native EmptyClipboard failed: ' + str(ctypes.get_last_error()))
        observe('publishing')
        if not user.SetClipboardData(13, memory):
            raise RuntimeError('Native SetClipboardData failed: ' + str(ctypes.get_last_error()))
        transferred = True
        if not user.CloseClipboard():
            raise RuntimeError('Native CloseClipboard failed: ' + str(ctypes.get_last_error()))
        opened = False
        observe('written', attempts=attempts, elapsed_ms=round((time.monotonic() - started) * 1000),
                sequence=user.GetClipboardSequenceNumber())
    finally:
        if opened:
            user.CloseClipboard()
        if memory and not transferred:
            kernel.GlobalFree(memory)
        user.DestroyWindow(window)


if __name__ == '__main__':
    try:
        write_native(validate_environment(os.environ, sys.platform))
    except Exception as error:
        observe('failed', kind=type(error).__name__, message=str(error))
        raise SystemExit(1)
