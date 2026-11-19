"""Exercise screenshot buttons and shortcuts on an isolated X11 desktop."""
import os
import ctypes
import sqlite3
import subprocess
import time


def check(desktop, helpers, *, native_main=False):
    assert os.environ.get('GITHUB_ACTIONS') == 'true'
    assert os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('Atspi', '2.0')
    from gi.repository import Gtk, Gdk, GdkPixbuf, Atspi
    cli = desktop.with_name('dzc')
    subprocess.run([cli, 'add', 'screenshot fixture'], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        settings = [
            ('app.window_pinned', 'true'), ('app.language', 'en'), ('app.silent_start', 'false'),
            ('app.screenshot_enabled', 'true'), ('app.screenshot_hotkey', 'Ctrl+Shift+A'),
            ('app.screenshot_full_hotkey', 'Ctrl+Shift+F'), ('app.window_width', '750'), ('app.window_height', '650')]
        if native_main:
            settings.extend([('app.theme', 'mica'), ('app.arrow_key_selection', 'true'),
                             ('app.preview_keyboard_enabled', 'false'),
                             ('app.privacy_protection', 'true'),
                             ('app.privacy_protection_custom_rules', '^fixture-private-only-20261120$')])
        db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', settings)
    subprocess.run([cli, 'service', 'start', '--json'], check=True, stdout=subprocess.DEVNULL)
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    root = Gdk.get_default_root_window()
    width, height = root.get_width(), root.get_height()
    x11, xtest = ctypes.CDLL('libX11.so.6'), ctypes.CDLL('libXtst.so.6')
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [ctypes.c_char_p], ctypes.c_void_p
    x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = x11.XOpenDisplay(None)
    assert display
    target = Gtk.Window(title='Independent screenshot shortcut target')
    target.set_default_size(240, 100)
    target.add(Gtk.Entry())

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

    def find(label, role=None):
        return next((node for node, name, _, text in helpers['mygo_nodes'](Atspi, desktop)
                     if label in (name, text) and (role is None or node.get_role() == role)), None)

    def click(label):
        def ready():
            node = find(label)
            if node and (not native_main or node.get_state_set().contains(Atspi.StateType.SENSITIVE)):
                return node

        node = wait(ready, f'{label} unavailable')
        if 'Action' in node.get_interfaces():
            assert Atspi.Action.do_action(node, 0)
        else:
            Atspi.Component.scroll_to(node, Atspi.ScrollType.ANYWHERE)
            bounds = Atspi.Component.get_extents(node, Atspi.CoordType.SCREEN)
            assert Atspi.generate_mouse_event(bounds.x + bounds.width//2, bounds.y + bounds.height//2, 'b1c')

    def keys(*symbols):
        for pressed in (True, False):
            for sym in symbols if pressed else reversed(symbols):
                keycode = x11.XKeysymToKeycode(display, sym)
                assert keycode
                xtest.XTestFakeKeyEvent(display, keycode, pressed, 0)
                x11.XSync(display, False)
                pump(.03)
        pump(.1)

    def chord(letter):
        target.show_all()
        target.present()
        wait(target.is_active, 'Independent window did not acquire focus before the shortcut')
        keys(Gdk.KEY_Control_L, Gdk.KEY_Shift_L, letter)

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

    def check_native_repaint():
        def geometry():
            active = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1]
            name = subprocess.run(['xprop', '-id', active, '_NET_WM_NAME'], capture_output=True, text=True)
            if '= "DezirClip"' not in name.stdout:
                return None
            info = subprocess.check_output(['xwininfo', '-id', active], text=True)
            values = dict(line.strip().split(':', 1) for line in info.splitlines() if ':' in line)
            bounds = tuple(int(values[key]) for key in ('Absolute upper-left X', 'Absolute upper-left Y', 'Width', 'Height'))
            return active, bounds

        wait(lambda: find('screenshot fixture'), 'Initial native history did not appear')
        xid, bounds = wait(geometry, 'Native history window was not active')
        # The new clipboard image must be visible on the X11 root without resizing
        # the window, which can repaint stale content even with the old renderer.
        color = b'\xe4\x19\x81'

        def thumbnail_visible():
            pixbuf = Gdk.pixbuf_get_from_window(root, *bounds)
            pixels = pixbuf.get_pixels()
            channels, stride = pixbuf.get_n_channels(), pixbuf.get_rowstride()
            return sum(pixels[offset:offset+3] == color
                       for row in range(bounds[3])
                       for offset in range(row*stride, row*stride + bounds[2]*channels, channels)) > 50

        assert not thumbnail_visible(), 'Thumbnail color already present before writing the test image'
        thumbnail = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 40, 24)
        thumbnail.fill(0xe41981ff)
        clipboard.set_image(thumbnail)
        wait(thumbnail_visible, 'Native history did not paint the new thumbnail to X11')
        assert geometry() == (xid, bounds), 'Native window moved or resized during the repaint test'
        print('Verified software-rendered MyGo history repaints X11 after a clipboard change', flush=True)

    def check_native_appearance():
        def setting(key):
            with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
                row = db.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
            return row[0] if row else None

        click('Appearance')
        for label, key, shortcut, value, unit in [
            ('Item font size', 'app.clipboard_item_font_size', Gdk.KEY_End, '18', ' px'),
            ('Tag font size', 'app.clipboard_tag_font_size', Gdk.KEY_End, '14', ' px'),
            ('Preview opacity', 'app.preview_opacity', Gdk.KEY_Home, '0', '%'),
            ('Surface opacity', 'app.surface_opacity', Gdk.KEY_End, '100', '%'),
        ]:
            def ready_slider():
                node = find(label, Atspi.Role.SLIDER)
                if node and node.get_state_set().contains(Atspi.StateType.SENSITIVE):
                    return node

            slider = wait(ready_slider, f'{label} slider missing')
            assert Atspi.Component.grab_focus(slider)
            keys(shortcut)
            wait(lambda: setting(key) == value, f'{label} was not saved')
            wait(lambda: find(value + unit), f'{label} did not show the saved value')
        for label, key, reset in [('Interface font', 'app.font_main', 'Reset interface font'),
                                  ('Monospace font', 'app.font_mono', 'Reset monospace font')]:
            field = wait(lambda: find(label, Atspi.Role.COMBO_BOX), f'{label} picker missing')
            assert Atspi.Component.grab_focus(field)
            keys(Gdk.KEY_Down)
            keys(Gdk.KEY_Return)
            wait(lambda: setting(key), f'{label} was not saved')
            click(reset)
            wait(lambda: setting(key) == '', f'{label} did not reset to its theme default')
        print('Verified native appearance saves typography and independent surface and preview opacity', flush=True)

    def check_native_protected_entry():
        secret = 'fixture-private-only-20261120'
        marker = 'fixture-safe-clipboard-marker'

        def active_window():
            return subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1]

        main = active_window()
        clipboard.set_text(secret, -1)
        wait(lambda: find('Reveal sensitive entry') and find('Sensitive content hidden'),
             'Protected capture did not start concealed')
        assert not find(secret)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            assert db.execute('SELECT COUNT(*) FROM clipboard_history WHERE content=? OR preview=?',
                              (secret, secret)).fetchone()[0] == 0

        click('Reveal sensitive entry')
        wait(lambda: find(secret), 'Explicit reveal did not display the protected fixture')
        click('Hide sensitive entry')
        wait(lambda: find('Reveal sensitive entry') and not find(secret), 'Hide did not conceal the fixture')

        click('Reveal sensitive entry')
        wait(lambda: find(secret), 'A second explicit reveal failed')
        target.show_all()
        target.present()
        wait(target.is_active, 'Independent window did not acquire focus')
        wait(lambda: find('Reveal sensitive entry') and not find(secret), 'Protected content survived focus loss')
        target.hide()
        info = subprocess.check_output(['xwininfo', '-id', main], text=True)
        sizes = dict(line.strip().split(':', 1) for line in info.splitlines() if ':' in line)
        x = int(sizes['Absolute upper-left X']) + int(sizes['Width'])//2
        y = int(sizes['Absolute upper-left Y']) + 15
        xtest.XTestFakeMotionEvent(display, -1, x, y, 0)
        xtest.XTestFakeButtonEvent(display, 1, True, 0)
        xtest.XTestFakeButtonEvent(display, 1, False, 0)
        x11.XSync(display, False)
        wait(lambda: active_window() == main, 'The native window did not regain focus')
        assert not find(secret), 'Refocusing resurrected protected content'

        clipboard.set_text(marker, -1)
        wait(lambda: find(marker), 'Public marker was not captured')
        keys(Gdk.KEY_Down)
        keys(Gdk.KEY_Down)
        keys(Gdk.KEY_Control_L, Gdk.KEY_c)
        wait(lambda: find('Reveal the sensitive entry first'), 'The concealed row did not reject Copy')
        assert clipboard.wait_for_text() == marker, 'A concealed row was allowed to copy'

        click('Reveal sensitive entry')
        wait(lambda: find(secret), 'The fixture could not be revealed after refocusing')
        keys(Gdk.KEY_Control_L, Gdk.KEY_c)
        wait(lambda: clipboard.wait_for_text() == secret, 'Copy did not use the explicitly revealed row')

        if find('Reveal sensitive entry'):
            click('Reveal sensitive entry')
            wait(lambda: find(secret), 'The copied fixture could not be revealed again')
        click('Settings')
        assert not find(secret)
        click('Back')
        wait(lambda: find('Reveal sensitive entry') and not find(secret),
             'The settings transition resurrected protected content')
        print('Verified native protected capture, reveal and hide, focus-loss concealment, and authorized copy', flush=True)

    env = dict(os.environ, DZC_MYGO_NATIVE_MAIN='1', MYGO_GPU='0') if native_main else None
    process = subprocess.Popen([desktop], env=env)
    try:
        if native_main:
            check_native_repaint()
            check_native_protected_entry()
        click('Settings')
        if native_main:
            check_native_appearance()
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
        target.destroy()
        x11.XCloseDisplay(display)
