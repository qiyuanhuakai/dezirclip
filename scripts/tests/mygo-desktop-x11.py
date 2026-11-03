"""Inspect the packaged MyGo WebView and copy a synthetic row in a private Xvfb session."""
import json
import ctypes
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import time


def check_quick_paste(marker):
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('GdkX11', '3.0')
    from gi.repository import Gtk, GdkX11

    target = Gtk.Window(title='Independent paste target')
    editor = Gtk.Entry()
    target.add(editor)
    target.show_all()
    editor.grab_focus()
    # The previous WebView test copied the same row. Change the clipboard so
    # a plain keyboard paste cannot satisfy the native-search assertion.
    other = 'Different quick-paste fixture ' + secrets.token_hex(8)
    subprocess.run(['xclip', '-selection', 'clipboard', '-i'], input=other.encode(),
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-o']).decode() == other
    x11 = ctypes.CDLL('libX11.so.6')
    xtest = ctypes.CDLL('libXtst.so.6')
    ptr, xid = ctypes.c_void_p, ctypes.c_ulong
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [ctypes.c_char_p], ptr
    x11.XSetInputFocus.argtypes = [ptr, xid, ctypes.c_int, xid]
    x11.XGetInputFocus.argtypes = [ptr, ctypes.POINTER(xid), ctypes.POINTER(ctypes.c_int)]
    x11.XKeysymToKeycode.argtypes = [ptr, xid]
    x11.XSync.argtypes = [ptr, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ptr]
    xtest.XTestFakeKeyEvent.argtypes = [ptr, ctypes.c_uint, ctypes.c_int, xid]
    display = x11.XOpenDisplay(None)
    assert display

    def pump():
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)

    def chord(*syms):
        codes = [x11.XKeysymToKeycode(display, sym) for sym in syms]
        assert all(codes)
        for code in codes:
            assert xtest.XTestFakeKeyEvent(display, code, 1, 0)
        for code in reversed(codes):
            assert xtest.XTestFakeKeyEvent(display, code, 0, 0)
        x11.XSync(display, False)

    def focus():
        window, revert = xid(), ctypes.c_int()
        x11.XGetInputFocus(display, ctypes.byref(window), ctypes.byref(revert))
        return window.value

    try:
        pump()
        original = GdkX11.X11Window.get_xid(target.get_window())
        x11.XSetInputFocus(display, original, 2, 0)
        x11.XSync(display, False)
        assert focus() == original
        chord(0xffe3, 0xffe1, ord('v'))
        deadline = time.monotonic() + 6
        while focus() in (0, 1, original):
            assert time.monotonic() < deadline, 'Native quick panel did not acquire input focus'
            pump()
            time.sleep(.025)
        time.sleep(.25)
        # Type the unique query into the actual native search box. Wait through
        # the IME debounce before submitting the selected row exactly once.
        for char in marker.split()[-1]:
            chord(ord(char))
        time.sleep(1)
        chord(0xff0d)
        deadline = time.monotonic() + 6
        while editor.get_text() != marker:
            assert time.monotonic() < deadline, 'Native selection did not paste full text into the original editor'
            pump()
            time.sleep(.025)
        assert focus() == original
        print('Verified independent global shortcut, native search, Enter and full text in the original editor')
    finally:
        target.destroy()
        x11.XCloseDisplay(display)


def check(desktop):
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi

    cli = desktop.with_name('dzc')
    marker = 'MyGo desktop fixture ' + secrets.token_hex(12)
    subprocess.run([cli, 'add', marker], check=True, stdout=subprocess.DEVNULL)
    process = subprocess.Popen([desktop], stdout=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 35
        last = []
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'Desktop exited before presenting history ({process.returncode})')
            pending = [Atspi.get_desktop(0)]
            names, action = [], None
            while pending and len(names) < 300:
                item = pending.pop()
                if item is None:
                    continue
                name = item.get_name()
                text = Atspi.Text.get_text(item, 0, -1) if 'Text' in item.get_interfaces() else ''
                names.append((name or '') + ' ' + (text or ''))
                if name == '复制' and item.get_role() == Atspi.Role.PUSH_BUTTON:
                    action = item
                pending.extend(item.get_child_at_index(i) for i in range(item.get_child_count()))
            last = names
            if action and any(marker in (name or '') for name in names):
                assert Atspi.Action.do_action(action, 0), 'Copy button did not accept its native action'
                break
            time.sleep(.15)
        else:
            raise RuntimeError(f'History row and Copy button not presented: {last}')
        for _ in range(80):
            result = subprocess.run(['xclip', '-selection', 'clipboard', '-o'], capture_output=True, timeout=3)
            if result.returncode == 0 and result.stdout.decode() == marker:
                break
            time.sleep(.05)
        else:
            raise RuntimeError('MyGo row did not copy complete text to the isolated clipboard')
        state = json.loads(subprocess.check_output([cli, 'service', 'status', '--json']))
        assert state['mode'] == 'service' and state['capture_owner']
        check_quick_paste(marker)
        print('Verified independent MyGo WebView, visible synthetic history, native Copy action and clipboard text')
    finally:
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)
        subprocess.run([cli, 'service', 'stop'], check=True)


if __name__ == '__main__':
    desktop = Path(sys.argv[1]).resolve(strict=True)
    if len(sys.argv) == 2:
        with tempfile.TemporaryDirectory(prefix='dzc-mygo-') as path:
            home = Path(path)
            for name in ('runtime', 'data', 'config', 'cache'):
                (home / name).mkdir(mode=0o700)
            env = {**os.environ, 'HOME':path, 'XDG_RUNTIME_DIR':str(home/'runtime'), 'XDG_DATA_HOME':str(home/'data'),
                   'XDG_CACHE_HOME':str(home/'cache'), 'XDG_CONFIG_HOME':str(home/'config'),
                   'DEZIRCLIP_DB_PATH':str(home/'data/clipboard.db'), 'GDK_BACKEND':'x11', 'XDG_SESSION_TYPE':'x11',
                   'GTK_A11Y':'atspi', 'NO_AT_BRIDGE':'0'}
            for name in ('DBUS_SESSION_BUS_ADDRESS', 'WAYLAND_DISPLAY', 'AT_SPI_BUS_ADDRESS'):
                env.pop(name, None)
            subprocess.run(['xvfb-run', '-a', 'dbus-run-session', '--', sys.executable, __file__, desktop, path],
                           env=env, check=True, timeout=65)
    else:
        home = Path(sys.argv[2]).resolve(strict=True)
        assert home == Path(os.environ['HOME']) and home/'data/clipboard.db' == Path(os.environ['DEZIRCLIP_DB_PATH'])
        check(desktop)
