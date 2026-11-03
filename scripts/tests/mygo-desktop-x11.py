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


def mygo_nodes(atspi, executable):
    from gi.repository import GLib
    desktop = atspi.get_desktop(0)
    pending = []
    for index in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(index)
            if app and Path(f'/proc/{app.get_process_id()}/exe').resolve(strict=True) == executable:
                pending.append(app)
        except (GLib.Error, FileNotFoundError, ProcessLookupError):
            continue
    for _ in range(400):
        if not pending: return
        item = pending.pop()
        if item is None: continue
        try:
            name = item.get_name() or ''
            interfaces = item.get_interfaces()
            text = atspi.Text.get_text(item, 0, -1) if 'Text' in interfaces else ''
            pending.extend(item.get_child_at_index(i) for i in range(item.get_child_count()))
        except GLib.Error:
            # The next scan starts with live application roots and the same deadline.
            continue
        yield item, name, interfaces, text


def check_pinned(row, atspi, gtk, marker, target, editor, executable):
    def wait_for(check, message):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            while gtk.events_pending(): gtk.main_iteration_do(False)
            result = check()
            if result: return result
            time.sleep(.04)
        tree = [(name, text[:80]) for _, name, _, text in mygo_nodes(atspi, executable) if name or text]
        raise AssertionError(f'{message}: {tree}')

    def find_item(name, text=False):
        for item, label, interfaces, content in mygo_nodes(atspi, executable):
            if text:
                if content == name: return item
            elif label == name and 'Action' in interfaces: return item

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
    cancel = wait_for(lambda: find_item('取消'), 'Missing explicit copy confirmation')
    assert atspi.Action.do_action(cancel, 0)
    wait_for(lambda: find_item('取消') is None, 'Cancel did not dismiss confirmation')
    assert clipboard.wait_for_text() == sentinel and editor.get_text() == marker
    click()
    accept = wait_for(lambda: find_item('仅复制'), 'Missing copy-only action')
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
        def shortcut(keys=(Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F8)):
            for pressed in (True, False):
                for sym in (keys if pressed else reversed(keys)):
                    mapped, entries = keymap.get_entries_for_keyval(sym)
                    assert mapped and entries
                    assert atspi.generate_keyboard_event(entries[0].keycode, None,
                        atspi.KeySynthType.PRESS if pressed else atspi.KeySynthType.RELEASE)
        def main_active():
            return subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1] == main
        shortcut()
        wait_for(main_active, 'Configured keyboard shortcut did not focus the existing MyGo window')
        tags = wait_for(lambda: find_item('标签管理'), 'Missing tag manager in shared WebView')
        assert atspi.Action.do_action(tags, 0)
        wait_for(lambda: find_item('标签管理') is None, 'Tag manager did not open')
        tagged = wait_for(lambda: find_item(marker, text=True), 'Tagged history row missing')
        bounds = atspi.Component.get_extents(tagged, atspi.CoordType.SCREEN)
        click()
        wait_for(lambda: alternate.is_active() and other.get_text() == marker, 'Tag row did not paste into the second editor')
        assert editor.get_text() == marker
        clipboard.set_text(sentinel, -1)
        tagged = wait_for(lambda: find_item(marker, text=True), 'Tag row disappeared after delivery')
        bounds = atspi.Component.get_extents(tagged, atspi.CoordType.SCREEN)
        click()
        accept = wait_for(lambda: find_item('仅复制'), 'Tag row omitted the copy-only confirmation')
        assert atspi.Action.do_action(accept, 0)
        wait_for(lambda: clipboard.wait_for_text() == marker, 'Tag-row manual copy did not write its source')
        assert other.get_text() == marker and editor.get_text() == marker
        add = wait_for(lambda: find_item('添加条目'), 'Missing manual entry command')
        assert atspi.Action.do_action(add, 0)
        field = wait_for(lambda: find_item('在此处输入条目内容...'), 'Manual text editor did not open')
        note = 'manual' + secrets.token_hex(8)
        assert atspi.Component.grab_focus(field)
        for char in note:
            shortcut((ord(char),))
        wait_for(lambda: find_item(note, text=True), 'Manual editor did not receive the note')
        save = wait_for(lambda: find_item('保存'), 'Missing save action')
        assert atspi.Action.do_action(save, 0)
        wait_for(lambda: find_item(note, text=True) if find_item('保存') is None else None,
                 'Manual note did not appear in its original tag after saving')
        print('Verified creating a manual text entry in the shared React tag manager')
        saved = find_item(note, text=True)
        bounds = atspi.Component.get_extents(saved, atspi.CoordType.SCREEN)
        assert atspi.generate_mouse_event(bounds.x + bounds.width//2, bounds.y + bounds.height//2, 'b3c')
        qr = wait_for(lambda: find_item('QR 码'), 'Missing QR action on the saved entry')
        assert atspi.Action.do_action(qr, 0)
        wait_for(lambda: any(name == 'QR Code' for _, name, _, _ in mygo_nodes(atspi, executable)),
                 'The shared QR dialog did not render its PNG')
        copy_svg = wait_for(lambda: find_item('复制 SVG'), 'Missing SVG copy action')
        assert atspi.Action.do_action(copy_svg, 0)
        wait_for(lambda: '<svg' in (clipboard.wait_for_text() or ''), 'SVG did not reach the system clipboard')
        assert other.get_text() == marker and editor.get_text() == marker
        close = wait_for(lambda: find_item('关闭'), 'Missing QR dialog close action')
        assert atspi.Action.do_action(close, 0)
        print('Verified QR preview and SVG copy from the shared tag row without pasting into either editor')
        alternate.present()
        wait_for(alternate.is_active, 'Second editor did not regain focus')
        shortcut()
        wait_for(main_active, 'Shortcut failed to show the pinned window again')
        shortcut()
        wait_for(lambda: subprocess.run(['xwininfo','-id',main], capture_output=True).returncode != 0,
                 'Foreground shortcut did not close the WebView')
        shortcut()
        wait_for(lambda: find_item('图片'), 'Resident shortcut did not reopen the shared WebView')
        print('Verified shared tag-row paste into the second editor and closing/reopening with the configured shortcut')
        def open_settings_group(name, control):
            if not find_item(control):
                heading = wait_for(lambda: find_item(name) or find_item(name, text=True), f'{name} settings unavailable')
                if 'Action' in heading.get_interfaces():
                    assert atspi.Action.do_action(heading, 0)
                else:
                    atspi.Component.scroll_to(heading, atspi.ScrollType.ANYWHERE)
                    box = atspi.Component.get_extents(heading, atspi.CoordType.SCREEN)
                    assert atspi.generate_mouse_event(box.x + box.width//2, box.y + box.height//2, 'b1c')
            return wait_for(lambda: find_item(control), f'{control} did not appear')

        def open_appearance(settings, group, theme):
            button = wait_for(lambda: find_item(settings), 'Settings button unavailable')
            box = atspi.Component.get_extents(button, atspi.CoordType.SCREEN)
            assert atspi.generate_mouse_event(box.x + box.width//2, box.y + box.height//2, 'b1c')
            return open_settings_group(group, theme)

        wait_for(lambda: find_item(note, text=True), 'Reopened history did not restore the saved note')
        retro = open_appearance('设置', '界面设置', '复古')
        assert atspi.Action.do_action(retro, 0)
        dark = wait_for(lambda: find_item('深色'), 'Dark mode unavailable')
        assert atspi.Action.do_action(dark, 0)
        english = wait_for(lambda: find_item('English'), 'English language unavailable')
        assert atspi.Action.do_action(english, 0)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: dict(db.execute("SELECT key, value FROM settings WHERE key IN ('app.theme','app.color_mode','app.language')"))
                     == {'app.theme': 'retro', 'app.color_mode': 'dark', 'app.language': 'en'}, 'Appearance changes did not reach the profile')
            font_input = wait_for(lambda: next((item for item, name, _, _ in mygo_nodes(atspi, executable)
                                                if name == 'UI Font' and item.get_role() == atspi.Role.COMBO_BOX), None),
                                  'UI font selector unavailable')
            atspi.Component.scroll_to(font_input, atspi.ScrollType.ANYWHERE)
            assert atspi.Component.grab_focus(font_input)
            shortcut((Gdk.KEY_Down,))
            wait_for(lambda: any(item.get_role() == atspi.Role.LIST_BOX and item.get_child_count() >= 2
                                 for item, _, _, _ in mygo_nodes(atspi, executable)),
                     'Installed font options did not load')
            # Start on the theme default, then select the first installed proportional family.
            shortcut((Gdk.KEY_Home,))
            shortcut((Gdk.KEY_Down,))
            shortcut((Gdk.KEY_Return,))
            family = wait_for(lambda: next((value for value, in db.execute(
                "SELECT value FROM settings WHERE key='app.font_main'") if value), None), 'UI font selection was not saved')
        # AT-SPI can operate on background controls. Address the actual WebView
        # before asking the window manager to close it.
        def settings_window():
            for window in Gdk.Screen.get_default().get_window_stack() or []:
                xid = str(window.get_xid())
                props = subprocess.run(['xprop', '-id', xid, '_NET_WM_NAME', '_NET_WM_PID'],
                                       capture_output=True, text=True)
                if '_NET_WM_NAME(UTF8_STRING) = "DezirClip"' not in props.stdout:
                    continue
                pid = props.stdout.split()[-1]
                if pid.isdecimal() and Path(f'/proc/{pid}/exe').resolve() == executable:
                    return window
        settings_main = wait_for(settings_window, 'Cannot identify MyGo settings window')
        original = settings_main.get_xid()
        settings_main.focus(Gdk.CURRENT_TIME)
        wait_for(lambda: subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1]
                 == hex(original), 'Settings window did not receive focus')
        shortcut((Gdk.KEY_Alt_L, Gdk.KEY_F4))
        wait_for(lambda: subprocess.run(['xwininfo','-id',str(original)], capture_output=True).returncode != 0,
                 'Settings WebView did not close')
        shortcut()
        wait_for(lambda: find_item('Image'), 'New WebView did not restore the selected language')
        selected = open_appearance('Settings', 'Appearance', 'Retro')
        assert selected.get_state_set().contains(atspi.StateType.PRESSED)
        wait_for(lambda: find_item(family, text=True), 'New WebView did not restore the selected font family')
        print('Verified saved theme, color mode, language and installed UI font selection in a newly opened WebView')
        details = open_settings_group('About', 'View build details')
        assert atspi.Action.do_action(details, 0)
        build = json.loads(subprocess.check_output([executable, '--build-info']))
        for value in [build['mygo_version'], build['target'], build['build_id']]:
            wait_for(lambda: find_item(value, text=True), f'Main window did not report its own build detail {value}')
        open_settings_group('Data Management', 'EXPORT CLIPBOARD HISTORY')
        directory = str(Path(os.environ['DEZIRCLIP_DB_PATH']).parent)
        wait_for(lambda: find_item(directory, text=True), 'Settings did not display the active Rust profile')
        print('Verified About shows the running MyGo binary identity and Data Management shows the isolated Service profile')
        imported_note = 'import-' + secrets.token_hex(8)
        backup = Path(os.environ['HOME'])/'fixture-backup.json'
        backup.write_text(json.dumps({'version': 'dezirclip-export-v2', 'exported_at': 1, 'entries': [{
            'id': 1, 'content_type': 'text', 'content': imported_note, 'created_at': 1, 'updated_at': 1, 'tags': []
        }]}))
        def action(name):
            return next((item for item, label, interfaces, _ in mygo_nodes(atspi, executable)
                         if label.casefold() == name.casefold() and 'Action' in interfaces), None)
        assert atspi.Action.do_action(action('Import Clipboard History'), 0)
        browse = wait_for(lambda: action('Browse File...'), 'Backup file chooser button unavailable')
        def choose_file(browse, path):
            previous_main = subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1]
            assert atspi.Action.do_action(browse, 0)
            wait_for(lambda: subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1] != previous_main
                     and action('Open'), 'Native file chooser did not open')
            shortcut((Gdk.KEY_Control_L, Gdk.KEY_l))
            location = wait_for(lambda: next((item for item, _, interfaces, _ in mygo_nodes(atspi, executable)
                                if 'EditableText' in interfaces and item.get_state_set().contains(atspi.StateType.FOCUSED)), None),
                                'Native file chooser location field did not take focus')
            assert atspi.EditableText.set_text_contents(location, str(path))
            accept_file = wait_for(lambda: next((item for item, label, interfaces, _ in mygo_nodes(atspi, executable)
                                   if label == 'Open' and item.get_role() == atspi.Role.PUSH_BUTTON and 'Action' in interfaces
                                   and item.get_state_set().contains(atspi.StateType.SENSITIVE)), None),
                                   'Native chooser did not enable Open')
            assert atspi.Action.do_action(accept_file, 0)
        choose_file(browse, backup)
        wait_for(lambda: find_item(imported_note, text=True), 'Selected backup did not show its preview')
        assert atspi.Action.do_action(action('Import Clipboard History'), 0)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: db.execute('SELECT COUNT(*) FROM clipboard_history WHERE content=?', (imported_note,)).fetchone()[0] == 1,
                     'Confirmed import did not persist the previewed entry')
        print('Verified choosing a backup file in the native dialog, previewing its text and importing it through the shared React settings')
        custom_rules = open_settings_group('Clipboard', 'Custom Rules')
        assert atspi.Action.do_action(custom_rules, 0)
        rules_field = wait_for(lambda: next((item for item, name, interfaces, _ in mygo_nodes(atspi, executable)
                               if name == 'Custom Rules' and item.get_state_set().contains(atspi.StateType.EDITABLE)), None),
                               'Custom privacy rule editor did not open')
        atspi.Component.scroll_to(rules_field, atspi.ScrollType.ANYWHERE)
        assert atspi.Component.grab_focus(rules_field)
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_a))
        rule = 'qvx' + secrets.token_hex(7)
        for char in rule:
            shortcut((ord(char),))
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            read_rule = lambda: db.execute("SELECT value FROM settings WHERE key='app.privacy_protection_custom_rules'").fetchone()
            try:
                wait_for(lambda: read_rule() == (rule,), 'Continuous typing lost the final privacy rule')
            except AssertionError as error:
                raise AssertionError(f'Synthetic rule expected {rule!r}, persisted {read_rule()!r}, editor {atspi.Text.get_text(rules_field, 0, -1)!r}') from error
        print('Verified the last custom privacy rule edit persisted during continuous keyboard input')
        mica = open_settings_group('Appearance', 'Mica')
        assert atspi.Action.do_action(mica, 0)
        wait_for(lambda: action('Choose Background'), 'Custom background picker unavailable')
        window = settings_window()
        xid = window.get_xid()
        window.focus(Gdk.CURRENT_TIME)
        shortcut((Gdk.KEY_Alt_L, Gdk.KEY_F4))
        wait_for(lambda: subprocess.run(['xwininfo','-id', str(xid)], capture_output=True).returncode != 0,
                 'WebView did not close after switching themes')
        shortcut()
        choose = open_appearance('Settings', 'Appearance', 'Choose Background')
        from gi.repository import GdkPixbuf
        wallpaper = Path(os.environ['HOME'])/'mygo-background.png'
        pixels = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 128, 128)
        pixels.fill(0xff00ffff)
        pixels.savev(str(wallpaper), 'png', [], [])
        observed = {}
        def painted():
            window = settings_window()
            if not window: return False
            _, left, top = window.get_origin()
            frame = Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(), left, top, window.get_width(), window.get_height())
            if not frame: return False
            data = frame.get_pixels()
            hits = 0
            peak = 0
            for y in range(0, frame.get_height(), 12):
                for x in range(0, frame.get_width(), 12):
                    offset = y * frame.get_rowstride() + x * frame.get_n_channels()
                    r, g, b = data[offset:offset+3]
                    chroma = min(r-g, b-g)
                    peak = max(peak, chroma)
                    hits += chroma > 60
            observed.update(hits=hits, peak=peak)
            return hits > 25
        assert not painted()
        baseline = observed.copy()
        choose_file(choose, wallpaper)
        wait_for(lambda: action('Clear Background'), 'Selected file did not appear in Settings')
        def show_history():
            close = wait_for(lambda: find_item('Close'), 'Settings close action unavailable')
            assert atspi.Action.do_action(close, 0)
            wait_for(lambda: find_item('Stickers'), 'Main window did not return to history')
            settings_window().focus(Gdk.CURRENT_TIME)
        show_history()
        try:
            wait_for(painted, 'Selected wallpaper did not paint its magenta pixels in the MyGo WebView')
        except AssertionError as error:
            with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
                settings = dict(db.execute("SELECT key,value FROM settings WHERE key IN ('app.custom_background','app.surface_opacity','app.custom_background_opacity','app.theme')"))
            raise AssertionError(f'Isolated wallpaper config {settings}, original {baseline}, observed {observed}') from error
        clear_bg = open_appearance('Settings', 'Appearance', 'Clear Background')
        assert atspi.Action.do_action(clear_bg, 0)
        wait_for(lambda: action('Choose Background'), 'Clear did not remove the selected file')
        show_history()
        wait_for(lambda: not painted(), 'Clearing the background left its pixels visible')
        print('Verified selecting, visibly painting and clearing a custom background in the shared MyGo WebView')
        full = open_appearance('Settings', 'Tools', 'Full Screen Capture')
        clipboard.set_text('waiting for explicit capture', -1)
        assert atspi.Action.do_action(full, 0)
        screen = Gdk.get_default_root_window()
        def copied_screen():
            image = clipboard.wait_for_image()
            return image is not None and (image.get_width(), image.get_height()) == (screen.get_width(), screen.get_height())
        wait_for(copied_screen, 'Settings full-screen capture did not copy the display')
        wait_for(lambda: find_item('Copied to clipboard', text=True), 'Full-screen capture was not acknowledged')
        region = wait_for(lambda: action('Region Capture'), 'Region capture button unavailable')
        assert atspi.Action.do_action(region, 0)
        def active_region():
            window = subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1]
            props = subprocess.run(['xprop','-id',window,'_NET_WM_NAME','_NET_WM_PID'], capture_output=True, text=True)
            if 'DezirClip 截图' in props.stdout:
                pid = props.stdout.split()[-1]
                if pid.isdecimal() and Path(f'/proc/{pid}/exe').resolve() == executable:
                    return int(pid)
        resident_pid = wait_for(active_region, 'Settings did not present the native screenshot window')
        assert '--main-window' not in Path(f'/proc/{resident_pid}/cmdline').read_bytes().decode().split('\0')
        shortcut((Gdk.KEY_Escape,))
        wait_for(lambda: not active_region(), 'Resident screenshot selector did not dismiss')
        assert copied_screen()
        print('Verified full-screen and region capture buttons reached the resident screenshot implementation')
        show_history()
        alternate.present()
        wait_for(alternate.is_active, 'Emoji editor did not acquire focus')
        other.set_text('')
        shortcut()
        wait_for(lambda: not alternate.is_active(), 'Shortcut did not focus MyGo before selecting an emoji')
        stickers = wait_for(lambda: find_item('Stickers'), 'Emoji panel was not available in the restored main window')
        assert atspi.Action.do_action(stickers, 0)
        button, emoji = wait_for(lambda: next(((item, name) for item, name, interfaces, _ in mygo_nodes(atspi, executable)
                                               if 'Action' in interfaces and any(ord(char) >= 0x1f000 for char in name)), None),
                                 'No emoji button in the shared panel')
        assert atspi.Action.do_action(button, 0)
        wait_for(lambda: alternate.is_active() and other.get_text() == emoji,
                 'Selected emoji did not reach its original editor')
        assert clipboard.wait_for_text() == emoji and editor.get_text() == marker
        print('Verified generated emoji pasted into the original editor through the shared Host')
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
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.custom_background_opacity','100')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.emoji_panel_enabled','true')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.window_pinned','true')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.hotkey','Ctrl+Shift+F8')")
    if pinned:
        entry = subprocess.check_output([cli, 'list', '1', '--ids'], text=True).strip()
        subprocess.run([cli, 'tag', 'add', str(int(entry)), 'test-tag'], check=True, stdout=subprocess.DEVNULL)
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
            names, row = [], None
            filter_button = None
            for item, name, interfaces, text in mygo_nodes(Atspi, desktop):
                names.append((name or '') + ' ' + (text or ''))
                if name == ('图片' if filter_stage == 0 else '文本') and 'Action' in interfaces:
                    filter_button = item
                if text == marker:
                    row = item
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
                    check_pinned(row, Atspi, Gtk, marker, target, editor, desktop)
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
