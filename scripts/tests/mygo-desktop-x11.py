"""Inspect the packaged MyGo WebView and copy a synthetic row in a private Xvfb session."""
import json
import ctypes
import os
from pathlib import Path
import secrets
import select
import sqlite3
import subprocess
import sys
import tempfile
import time


def memory_sample(label, roots):
    parents = {}
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            parents[int(path.parent.name)] = int(path.read_text().rsplit(')', 1)[1].split()[1])
        except (OSError, ValueError):
            continue
    selected = set(roots)
    while children := {pid for pid, parent in parents.items() if parent in selected} - selected:
        selected.update(children)
    pss = 0
    missing = 0
    for pid in selected:
        try:
            lines = Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
            pss += int(next(line for line in lines if line.startswith('Pss:')).split()[1])
        except (OSError, StopIteration):
            missing += 1
    print(f'{label} PSS {pss/1024:.1f} MiB across {len(selected)-missing} processes, {missing} unreadable', flush=True)
    return selected


def check_native_panels(marker, desktop, service_pid, target, editor, paste_row):
    import gi
    gi.require_version('Gtk', '3.0')
    from gi.repository import Gtk, Gdk, GdkPixbuf

    x11 = ctypes.CDLL('libX11.so.6')
    xtest = ctypes.CDLL('libXtst.so.6')
    ptr, xid = ctypes.c_void_p, ctypes.c_ulong
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [ctypes.c_char_p], ptr
    x11.XGetInputFocus.argtypes = [ptr, ctypes.POINTER(xid), ctypes.POINTER(ctypes.c_int)]
    x11.XKeysymToKeycode.argtypes = [ptr, xid]
    x11.XSync.argtypes = [ptr, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ptr]
    xtest.XTestFakeKeyEvent.argtypes = [ptr, ctypes.c_uint, ctypes.c_int, xid]
    xtest.XTestFakeMotionEvent.argtypes = [ptr, ctypes.c_int, ctypes.c_int, ctypes.c_int, xid]
    xtest.XTestFakeButtonEvent.argtypes = [ptr, ctypes.c_uint, ctypes.c_int, xid]
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
        roots = [desktop.pid, service_pid]
        processes = memory_sample('Main window open', roots)
        # Keep descriptors across close so a reparented WebKit process still
        # counts as live. A disappearing parent-child link is insufficient.
        peers = [pid for pid in processes - set(roots) if
                 Path(f'/proc/{pid}/comm').read_text().strip().startswith(('WebKit', 'dzc-desktop'))]
        assert any(Path(f'/proc/{pid}/exe').resolve() == Path(f'/proc/{desktop.pid}/exe').resolve() for pid in peers)
        watchers = [os.pidfd_open(pid) for pid in peers]
        main_id = subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1]
        paste_row()
        deadline = time.monotonic() + 6
        while editor.get_text() != marker or subprocess.run(['xwininfo', '-id', main_id], capture_output=True).returncode == 0:
            assert time.monotonic() < deadline, f'Main row did not paste into the original editor and close (text={editor.get_text()!r})'
            pump()
            time.sleep(.05)
        assert target.is_active()
        assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-o']).decode() == marker
        print('Verified shared React row pasted complete text into the original editor and closed the main window', flush=True)
        try:
            ready, _, _ = select.select(watchers, [], [], 6)
            deadline = time.monotonic() + 6
            while len(ready) < len(watchers):
                pending = set(watchers)-set(ready)
                more, _, _ = select.select(list(pending), [], [], max(0, deadline-time.monotonic()))
                assert more, 'WebView child remained resident after closing the main window'
                ready.extend(more)
        finally:
            for fd in watchers: os.close(fd)
        assert desktop.poll() is None
        time.sleep(1)
        memory_sample('Main closed, before native panels', roots)
        editor.set_text('')
        other = 'Different quick-paste fixture ' + secrets.token_hex(8)
        subprocess.run(['xclip', '-selection', 'clipboard', '-i'], input=other.encode(),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-o']).decode() == other
        editor.grab_focus()
        pump()
        assert target.is_active(), 'Original editor lost focus before the resident shortcut'
        original = focus()
        assert original not in (0, 1)
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

        root = Gdk.get_default_root_window()
        width, height = root.get_width(), root.get_height()
        start_x, start_y = width-150, height-150
        expected = Gdk.pixbuf_get_from_window(root, start_x+20, start_y+20, 1, 1).get_pixels()[:3]

        def read_png(w, h):
            deadline = time.monotonic() + 7
            while time.monotonic() < deadline:
                pump()
                result = subprocess.run(['xclip', '-selection', 'clipboard', '-t', 'image/png', '-o'],
                                        capture_output=True, timeout=2)
                if result.returncode == 0 and result.stdout.startswith(b'\x89PNG'):
                    loader = GdkPixbuf.PixbufLoader.new_with_type('png')
                    loader.write(result.stdout)
                    loader.close()
                    image = loader.get_pixbuf()
                    if (image.get_width(), image.get_height()) == (w, h):
                        return image
                time.sleep(.05)
            raise AssertionError(f'Expected clipboard screenshot {w}x{h}')

        chord(0xffe3, 0xffe1, ord('f'))
        read_png(width, height)
        chord(0xffe3, 0xffe1, ord('a'))
        deadline = time.monotonic()+7
        while focus() in (0, 1, original):
            assert time.monotonic() < deadline, 'Region selector did not acquire input focus'
            pump()
            time.sleep(.025)
        xtest.XTestFakeMotionEvent(display, -1, start_x, start_y, 0)
        xtest.XTestFakeButtonEvent(display, 1, 1, 50)
        xtest.XTestFakeMotionEvent(display, -1, start_x+80, start_y+65, 50)
        xtest.XTestFakeButtonEvent(display, 1, 0, 50)
        x11.XSync(display, False)
        image = read_png(80, 65)
        index = 20*image.get_rowstride()+20*image.get_n_channels()
        assert image.get_pixels()[index:index+3] == expected
        time.sleep(1)
        assert desktop.poll() is None
        memory_sample('Main closed, after native panels', roots)
        print('Verified full screenshot and native region selection with matching dimensions and frozen pixel')
    finally:
        target.destroy()
        x11.XCloseDisplay(display)


def check_pinned(row, atspi, gtk, marker, target, editor):
    from gi.repository import GLib
    def wait_for(check, message):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            while gtk.events_pending(): gtk.main_iteration_do(False)
            result = check()
            if result: return result
            time.sleep(.04)
        raise AssertionError(message)

    def find_button(name):
        pending = [atspi.get_desktop(0)]
        while pending:
            item = pending.pop()
            if item is None: continue
            try:
                if item.get_name() == name and 'Action' in item.get_interfaces(): return item
                pending.extend(item.get_child_at_index(i) for i in range(item.get_child_count()))
            except GLib.Error:
                # The original accessible is removed when the dialog or WebView closes.
                continue

    main = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1]
    assert '_NET_WM_STATE_ABOVE' in subprocess.check_output(['xprop','-id',main,'_NET_WM_STATE'], text=True)
    bounds = atspi.Component.get_extents(row, atspi.CoordType.SCREEN)
    def click():
        assert atspi.generate_mouse_event(bounds.x + bounds.width//2, bounds.y + bounds.height//2, 'b1c')
    click()
    wait_for(lambda: target.is_active() and editor.get_text() == marker, 'First pinned paste failed')
    assert subprocess.run(['xwininfo','-id',main], capture_output=True).returncode == 0
    # Change the clipboard without adding another history row to the no-capture fixture.
    from gi.repository import Gdk
    clipboard = gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    sentinel = 'pending manual copy'
    clipboard.set_text(sentinel, -1)
    click()
    cancel = wait_for(lambda: find_button('取消'), 'Missing explicit copy confirmation')
    assert atspi.Action.do_action(cancel, 0)
    wait_for(lambda: find_button('取消') is None, 'Cancel did not dismiss confirmation')
    assert clipboard.wait_for_text() == sentinel and editor.get_text() == marker
    click()
    accept = wait_for(lambda: find_button('仅复制'), 'Missing copy-only action')
    assert atspi.Action.do_action(accept, 0)
    wait_for(lambda: clipboard.wait_for_text() == marker, 'Confirmed copy did not write selected content')
    assert editor.get_text() == marker
    print('Verified native pinned window, first delivery, cancellation, and a separately confirmed copy without replay')
    alternate = gtk.Window(title='Second independent paste target')
    other = gtk.Entry()
    alternate.add(other)
    alternate.show_all()
    other.grab_focus()
    alternate.present()
    try:
        wait_for(alternate.is_active, 'Second editor did not acquire focus')
        # A real configured global shortcut must bind the editor before the MyGo window regains focus.
        keymap = Gdk.Keymap.get_for_display(Gdk.Display.get_default())
        def shortcut():
            for pressed in (True, False):
                keys = [Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F8]
                for sym in (keys if pressed else reversed(keys)):
                    mapped, entries = keymap.get_entries_for_keyval(sym)
                    assert mapped and entries
                    assert atspi.generate_keyboard_event(entries[0].keycode, None,
                        atspi.KeySynthType.PRESS if pressed else atspi.KeySynthType.RELEASE)
        def main_active():
            return subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1] == main
        shortcut()
        wait_for(main_active, 'Configured keyboard shortcut did not focus the existing MyGo window')
        click()
        wait_for(lambda: alternate.is_active() and other.get_text() == marker, 'Pinned paste did not rebind to second editor')
        assert editor.get_text() == marker
        shortcut()
        wait_for(main_active, 'Shortcut failed to show the pinned window again')
        shortcut()
        wait_for(lambda: subprocess.run(['xwininfo','-id',main], capture_output=True).returncode != 0,
                 'Foreground shortcut did not close the WebView')
        shortcut()
        wait_for(lambda: find_button('图片'), 'Resident shortcut did not reopen the shared WebView')
        print('Verified configured main shortcut rebinds the second editor, closes and reopens the shared window')
    finally:
        alternate.destroy()


def check(desktop, pinned):
    import gi
    gi.require_version('Atspi', '2.0')
    gi.require_version('Gtk', '3.0')
    from gi.repository import Atspi, Gtk

    cli = desktop.with_name('dzc')
    marker = 'MyGo desktop fixture ' + secrets.token_hex(12)
    subprocess.run([cli, 'add', marker], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.compact_mode','true')")
        if pinned:
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.window_pinned','true')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.hotkey','Ctrl+Shift+F8')")
    if pinned:
        subprocess.run([cli, 'service', 'start', '--no-capture', '--json'], check=True, stdout=subprocess.DEVNULL)
    target = Gtk.Window(title='Independent paste target')
    editor = Gtk.Entry()
    target.add(editor)
    target.show_all()
    editor.grab_focus()
    target.present()
    deadline = time.monotonic() + 5
    while not target.is_active():
        while Gtk.events_pending(): Gtk.main_iteration_do(False)
        assert time.monotonic() < deadline, 'No focused editor before opening MyGo'
        time.sleep(.025)
    process = subprocess.Popen([desktop], stdout=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 35
        last = []
        filter_stage = 0
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'Desktop exited before presenting history ({process.returncode})')
            pending = [Atspi.get_desktop(0)]
            names, row = [], None
            filter_button = None
            while pending and len(names) < 300:
                item = pending.pop()
                if item is None:
                    continue
                name = item.get_name()
                text = Atspi.Text.get_text(item, 0, -1) if 'Text' in item.get_interfaces() else ''
                names.append((name or '') + ' ' + (text or ''))
                if name == ('图片' if filter_stage == 0 else '文本') and 'Action' in item.get_interfaces():
                    filter_button = item
                if text == marker:
                    row = item
                pending.extend(item.get_child_at_index(i) for i in range(item.get_child_count()))
            last = names
            has_marker = any(marker in name for name in names)
            if filter_stage == 0 and has_marker and filter_button and not any('无法确认桌面能力' in name for name in names):
                assert Atspi.Action.do_action(filter_button, 0), 'Image filter did not accept input'
                filter_stage = 1
            elif filter_stage == 1 and not has_marker and filter_button:
                assert Atspi.Action.do_action(filter_button, 0), 'Text filter did not accept input'
                filter_stage = 2
            elif filter_stage == 2 and has_marker:
                print('Verified shared React main window: image filter excluded the live text fixture and text filter restored it', flush=True)
                filter_stage = 3
            if filter_stage == 3 and row and has_marker:
                if pinned:
                    check_pinned(row, Atspi, Gtk, marker, target, editor)
                    return
                # The Host closes idle desktop streams after 30 seconds.
                # Exercise copy and both resident shortcuts after that deadline.
                time.sleep(32)
                check_preview(row, Atspi)
                break
            time.sleep(.15)
        else:
            raise RuntimeError(f'History actions not available (filter stage {filter_stage}): {last}')
        state = json.loads(subprocess.check_output([cli, 'service', 'status', '--json']))
        assert state['mode'] == 'service' and state['capture_owner']
        endpoint = json.loads(subprocess.check_output([cli, 'service', 'desktop-endpoint']))
        def paste_row():
            rect = Atspi.Component.get_extents(row, Atspi.CoordType.SCREEN)
            assert rect.width > 0 and rect.height > 0
            assert Atspi.generate_mouse_event(rect.x + rect.width//2, rect.y + rect.height//2, 'b1c')
        check_native_panels(marker, process, endpoint['pid'], target, editor, paste_row)
        print('Verified independent MyGo WebView, visible synthetic history, native Copy action and clipboard text')
    finally:
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)
        subprocess.run([cli, 'service', 'stop'], check=True)


def check_preview(button, atspi):
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk
    screen = Gdk.Screen.get_default()
    pointer = screen.get_display().get_default_seat().get_pointer()
    rect = atspi.Component.get_extents(button, atspi.CoordType.SCREEN)
    assert rect.width > 0 and rect.height > 0
    def visible():
        info = subprocess.run(['xwininfo', '-id', str(preview_id)], capture_output=True, text=True)
        return info.returncode == 0 and 'Map State: IsViewable' in info.stdout
    def active():
        return subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True)
    focus = active()
    pointer.warp(screen, rect.x + rect.width//2, rect.y + rect.height//2)
    screen.get_display().sync()
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        tree = subprocess.check_output(['xwininfo','-root','-tree'], text=True)
        found = next((line for line in tree.splitlines() if '"DezirClip 悬浮预览"' in line), None)
        if found:
            preview_id = int(found.strip().split()[0], 16)
            if visible(): break
        time.sleep(.05)
    else: raise AssertionError('Hover did not show a native preview window')
    assert active() == focus, 'Hover preview took keyboard focus'
    pointer.warp(screen, 0, 0)
    screen.get_display().sync()
    deadline = time.monotonic() + 5
    while visible():
        assert time.monotonic() < deadline, 'Hover preview did not close after leaving the row'
        time.sleep(.05)
    print('Verified native hover preview, original focus and dismissal')


if __name__ == '__main__':
    desktop = Path(sys.argv[1]).resolve(strict=True)
    if len(sys.argv) == 2 or sys.argv[2] == '--pinned':
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
            subprocess.run(['xvfb-run', '-a', 'dbus-run-session', '--', sys.executable, __file__, desktop, path, *sys.argv[2:]],
                           env=env, check=True, timeout=105)
    else:
        home = Path(sys.argv[2]).resolve(strict=True)
        assert home == Path(os.environ['HOME']) and home/'data/clipboard.db' == Path(os.environ['DEZIRCLIP_DB_PATH'])
        # Rust checks which process owns the foreground window. A bare Xvfb
        # server has no window manager to maintain that desktop state.
        wm = subprocess.Popen(['openbox', '--sm-disable'], stdout=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 5
            while '_NET_SUPPORTING_WM_CHECK(WINDOW)' not in subprocess.check_output(
                    ['xprop', '-root', '_NET_SUPPORTING_WM_CHECK'], text=True):
                assert wm.poll() is None and time.monotonic() < deadline, 'Window manager failed to start'
                time.sleep(.05)
            check(desktop, '--pinned' in sys.argv[3:])
        finally:
            wm.terminate()
            wm.wait(timeout=5)
