"""Exercise screenshot buttons and shortcuts on an isolated X11 desktop."""
import os
import ctypes
import sqlite3
import subprocess
import time


def check(desktop, helpers):
    assert os.environ.get('GITHUB_ACTIONS') == 'true'
    assert os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('Atspi', '2.0')
    from gi.repository import Gtk, Gdk, Atspi
    cli = desktop.with_name('dzc')
    subprocess.run([cli, 'add', 'screenshot fixture'], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', [
            ('app.window_pinned', 'true'), ('app.language', 'en'), ('app.silent_start', 'false'),
            ('app.screenshot_enabled', 'true'), ('app.screenshot_hotkey', 'Ctrl+Shift+A'),
            ('app.screenshot_full_hotkey', 'Ctrl+Shift+F'), ('app.window_width', '750'), ('app.window_height', '650')])
    subprocess.run([cli, 'service', 'start', '--json'], check=True, stdout=subprocess.DEVNULL)
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    root = Gdk.get_default_root_window()
    width, height = root.get_width(), root.get_height()
    x11, xtest = ctypes.CDLL('libX11.so.6'), ctypes.CDLL('libXtst.so.6')
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [ctypes.c_char_p], ctypes.c_void_p
    x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = x11.XOpenDisplay(None)
    assert display

    def pump(seconds=.05):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            while Gtk.events_pending(): Gtk.main_iteration_do(False)
            time.sleep(.01)

    def wait(predicate, message):
        end = time.monotonic() + 12
        while time.monotonic() < end:
            pump()
            result = predicate()
            if result: return result
        tree = [(name, text[:120]) for _, name, _, text in helpers['mygo_nodes'](Atspi, desktop) if name or text]
        image = clipboard.wait_for_image()
        dimensions = (image.get_width(), image.get_height()) if image is not None else None
        active = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1]
        geometry = subprocess.run(['xwininfo', '-id', active], capture_output=True, text=True).stdout
        raise AssertionError(f'{message}; clipboard image={dimensions}; active window={geometry}; accessibility={tree}')

    def find(label):
        return next((node for node, name, _, text in helpers['mygo_nodes'](Atspi, desktop)
                     if label in (name, text)), None)

    def click(label):
        node = wait(lambda: find(label), f'{label} unavailable')
        if 'Action' in node.get_interfaces():
            assert Atspi.Action.do_action(node, 0)
        else:
            Atspi.Component.scroll_to(node, Atspi.ScrollType.ANYWHERE)
            bounds = Atspi.Component.get_extents(node, Atspi.CoordType.SCREEN)
            assert Atspi.generate_mouse_event(bounds.x + bounds.width//2, bounds.y + bounds.height//2, 'b1c')

    def chord(letter):
        keymap = Gdk.Keymap.get_for_display(root.get_display())
        keys = [Gdk.KEY_Control_L, Gdk.KEY_Shift_L, letter]
        for pressed in (True, False):
            for sym in keys if pressed else reversed(keys):
                found, entries = keymap.get_entries_for_keyval(sym)
                assert found and entries
                assert Atspi.generate_keyboard_event(entries[0].keycode, None,
                    Atspi.KeySynthType.PRESS if pressed else Atspi.KeySynthType.RELEASE)
        pump(.1)

    def captured(w, h):
        image = clipboard.wait_for_image()
        return image if image is not None and (image.get_width(), image.get_height()) == (w, h) else None

    def selector():
        active = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1]
        props = subprocess.run(['xprop', '-id', active, '_NET_WM_NAME', '_NET_WM_PID'], capture_output=True, text=True)
        return 'DezirClip 截图' in props.stdout and props.stdout.split()[-1] == str(process.pid)

    def region(trigger):
        clipboard.set_text('waiting for region screenshot', -1)
        x, y = width-150, height-150
        expected = Gdk.pixbuf_get_from_window(root, x+20, y+20, 1, 1).get_pixels()[:3]
        trigger()
        wait(selector, 'Region selector did not appear')
        xtest.XTestFakeMotionEvent(display, -1, x, y, 0)
        x11.XSync(display, False)
        pump(.1)
        xtest.XTestFakeButtonEvent(display, 1, True, 0)
        x11.XSync(display, False)
        pump(.1)
        for step in range(1, 11):
            xtest.XTestFakeMotionEvent(display, -1, x+80*step//10, y+65*step//10, 0)
            x11.XSync(display, False)
            pump(.04)
        xtest.XTestFakeButtonEvent(display, 1, False, 0)
        x11.XSync(display, False)
        image = wait(lambda: captured(80, 65), 'Region screenshot was not copied')
        offset = 20*image.get_rowstride()+20*image.get_n_channels()
        assert image.get_pixels()[offset:offset+3] == expected, 'Selector leaked into the captured pixels'
        wait(lambda: not selector(), 'Completed selector remained visible')

    process = subprocess.Popen([desktop])
    try:
        click('Settings')
        click('Tools')
        wait(lambda: find('Full Screen Capture'), 'Capture controls missing')
        clipboard.set_text('waiting for button full screenshot', -1)
        click('Full Screen Capture')
        wait(lambda: captured(width, height), 'Full-screen button did not copy the display')
        region(lambda: click('Region Capture'))
        clipboard.set_text('waiting for hotkey full screenshot', -1)
        chord(Gdk.KEY_f)
        wait(lambda: captured(width, height), 'Full-screen shortcut did not copy the display')
        region(lambda: chord(Gdk.KEY_a))
        assert process.poll() is None
        print('Verified X11 full-screen and region screenshots through buttons and global shortcuts', flush=True)
    finally:
        with helpers['observed_service_cleanup'](cli):
            process.terminate()
            process.wait(timeout=20)
        x11.XCloseDisplay(display)
