"""Inspect the packaged MyGo WebView and copy a synthetic row in a private Xvfb session."""
import array
import contextlib
import json
import ctypes
import errno
import os
from pathlib import Path
import secrets
import runpy
import select
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time


@contextlib.contextmanager
def private_audio(home):
    socket = home / 'audio.sock'
    os.environ.update(PULSE_SERVER=f'unix:{socket}', PULSE_RUNTIME_PATH=str(home/'runtime/pulse'))
    audio = home/'output.pcm'
    with subprocess.Popen(['pulseaudio', '-n', '--daemonize=no', '--use-pid-file=no', '--exit-idle-time=-1',
                           '--disable-shm=yes',
                           '-L', 'module-null-sink sink_name=fixture rate=22050 channels=1',
                           '-L', f'module-native-protocol-unix socket={socket} auth-anonymous=1'],
                          stdout=subprocess.DEVNULL) as server:
        try:
            deadline = time.monotonic() + 8
            while not socket.exists():
                assert server.poll() is None and time.monotonic() < deadline, 'Private audio server did not start'
                time.sleep(.025)
            with audio.open('wb') as stream, subprocess.Popen(
                    ['parec', '-d', 'fixture.monitor', '--raw', '--format=s16le', '--rate=22050',
                     '--channels=1', '--latency-msec=20'], stdout=stream) as recorder:
                try:
                    while audio.stat().st_size < 4096:
                        assert recorder.poll() is None and time.monotonic() < deadline, 'Private monitor unavailable'
                        time.sleep(.025)
                    yield audio
                finally:
                    recorder.terminate()
        finally:
            server.terminate()


def expect_cue(audio, byte_offset, paste):
    # Match the pitches and spacing of the Service cue. GTK and the window manager
    # share this private sink and may produce a UI sound before the clipboard cue.
    def tone(start, low, high):
        segment = pcm[start:start+440]
        cycles = sum(a < 0 <= b for a, b in zip(segment, segment[1:]))
        return max(segment, default=0) > 100 and low <= cycles <= high

    deadline = time.monotonic() + 5
    scanned = 0
    while time.monotonic() < deadline:
        with audio.open('rb') as source:
            source.seek(byte_offset)
            pcm = array.array('h')
            pcm.frombytes(source.read())
        while scanned + 3800 <= len(pcm):
            start = scanned
            scanned += 44
            first_pitch = (17, 21) if paste else (8, 11)
            if not tone(start, *first_pitch):
                continue
            # The monitor may drop the start of a short sound as its sink wakes up.
            # Require two distinct pitched bursts separated by quiet, even then.
            if paste:
                quiet = False
                for sample_offset in range(start+550, start+3300, 110):
                    if max(abs(x) for x in pcm[sample_offset:sample_offset+110]) < 50:
                        quiet = True
                    elif quiet and tone(sample_offset, 20, 25):
                        return
            elif max(abs(x) for x in pcm[start+1500:start+3050]) < 50:
                return
        time.sleep(.04)
    active = []
    for i in range(0, len(pcm)-220, 220):
        frame = pcm[i:i+220]
        peak = max(abs(x) for x in frame)
        if peak > 100:
            active.append((i//220, peak, sum(a < 0 <= b for a, b in zip(frame, frame[1:]))))
    raise AssertionError(f'Missing {"paste" if paste else "copy"} cue, active 10ms frames (index, peak, cycles): {active[:64]}')


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


def check_native_panels(marker, desktop, service_pid, target, editor, paste_row, audio):
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
        audio_start = audio.stat().st_size
        paste_row()
        deadline = time.monotonic() + 6
        while editor.get_text() != marker or subprocess.run(['xwininfo', '-id', main_id], capture_output=True).returncode == 0:
            assert time.monotonic() < deadline, f'Main row did not paste into the original editor and close (text={editor.get_text()!r})'
            pump()
            time.sleep(.05)
        expect_cue(audio, audio_start, paste=True)
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
        audio_start = audio.stat().st_size
        other = 'Different quick-paste fixture ' + secrets.token_hex(8)
        subprocess.run(['xclip', '-selection', 'clipboard', '-i'], input=other.encode(),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        assert subprocess.check_output(['xclip', '-selection', 'clipboard', '-o']).decode() == other
        editor.grab_focus()
        pump()
        assert target.is_active(), 'Original editor lost focus before the resident shortcut'
        expect_cue(audio, audio_start, paste=False)
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
        audio_start = audio.stat().st_size
        chord(0xff0d)
        deadline = time.monotonic() + 6
        while editor.get_text() != marker:
            assert time.monotonic() < deadline, 'Native selection did not paste full text into the original editor'
            pump()
            time.sleep(.025)
        expect_cue(audio, audio_start, paste=True)
        print('Recorded native copy and both main and closed-window quick paste cues on the private monitor')
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


def tray_item(pid, label, activate=True):
    from gi.repository import Gio, GLib
    from xml.etree import ElementTree
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    def call(owner, path, interface, method, params=None):
        return bus.call_sync(owner, path, interface, method, params, None, Gio.DBusCallFlags.NONE, 2000, None).unpack()
    daemon = ('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus')
    owners, = call(*daemon, 'ListNames')
    for name in owners:
        if not name.startswith(':'): continue
        try:
            server, = call(*daemon, 'GetConnectionUnixProcessID', GLib.Variant('(s)', (name,)))
            if server != pid: continue
            base = '/org/ayatana/NotificationItem'
            xml, = call(name, base, 'org.freedesktop.DBus.Introspectable', 'Introspect')
            nodes = ElementTree.fromstring(xml).findall('node')
        except GLib.Error:
            continue
        for node in nodes:
            path = base + '/' + node.get('name', '')
            try:
                status, = call(name, path, 'org.freedesktop.DBus.Properties', 'Get',
                               GLib.Variant('(ss)', ('org.kde.StatusNotifierItem', 'Status')))
                if status == 'Passive': continue
                menu = (name, path + '/Menu', 'com.canonical.dbusmenu')
                _, root = call(*menu, 'GetLayout', GLib.Variant('(iias)', (0, 1, ['label'])))
                item = next((item_id for item_id, props, _ in root[2] if props.get('label') == label), None)
                if item is not None:
                    if activate:
                        call(*menu, 'Event', GLib.Variant('(isvu)', (item, 'clicked', GLib.Variant('i', 0), 0)))
                    return True
            except GLib.Error:
                continue
    return False


def visible_main_window(resident_pid, executable):
    import re
    windows = subprocess.check_output(['xprop', '-root', '_NET_CLIENT_LIST'], text=True)
    for xid in re.findall(r'0x[0-9a-fA-F]+', windows):
        props = subprocess.run(['xprop', '-id', xid, '_NET_WM_NAME', '_NET_WM_PID'], capture_output=True, text=True)
        if '_NET_WM_NAME(UTF8_STRING) = "DezirClip"' not in props.stdout: continue
        pid = props.stdout.split()[-1]
        if not pid.isdecimal(): continue
        try:
            if Path(f'/proc/{pid}/exe').resolve(strict=True) != executable: continue
            if int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[1]) != resident_pid: continue
        except (FileNotFoundError, ProcessLookupError):
            continue
        info = subprocess.run(['xwininfo', '-id', xid], capture_output=True, text=True)
        if 'Map State: IsViewable' in info.stdout: return int(xid, 16)
    return False


def check_pinned(row, atspi, gtk, marker, target, editor, executable, resident):
    def wait_for(check, message, timeout=8):
        deadline = time.monotonic() + timeout
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

        note_row = wait_for(lambda: find_item(note, text=True), 'Reopened history did not restore the saved note')
        def open_in_row(node):
            if node.get_name() == '打开' and 'Action' in node.get_interfaces():
                return node
            for index in range(node.get_child_count()):
                action = open_in_row(node.get_child_at_index(index))
                if action: return action
        bounds = atspi.Component.get_extents(note_row, atspi.CoordType.SCREEN)
        assert atspi.generate_mouse_event(bounds.x + bounds.width//2, bounds.y + bounds.height//2, 'abs')
        def row_action():
            parent = find_item(note, text=True)
            while parent and parent.get_role() != atspi.Role.DOCUMENT_WEB:
                button = open_in_row(parent)
                if button: return button
                parent = parent.get_parent()
        open_note = wait_for(row_action, 'Selected row did not expose its Open action')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            entry_id, used = db.execute('SELECT id, use_count FROM clipboard_history WHERE content=?', (note,)).fetchone()
            assert atspi.Action.do_action(open_note, 0)
            note += ' externally edited'
            wait_for(lambda: db.execute('SELECT content,use_count FROM clipboard_history WHERE id=?', (entry_id,)).fetchone() == (note, used + 1),
                     'Opening the row did not persist the external edit and exactly one use')
        wait_for(lambda: find_item(note, text=True), 'External edit notification did not refresh the open WebView')
        print('Verified MyGo Open launches the system text handler and the Rust Service writes its edit back to the original row')
        cli = executable.with_name('dzc')
        subprocess.run([cli, 'privacy', 'on', '--seconds', '60'], check=True)
        try:
            (Path.home()/'finish-edit').touch()
            wait_for((Path.home()/'late-edit-written').is_file, 'External editor did not finish its late write')
        finally:
            subprocess.run([cli, 'privacy', 'off'], check=True)
        time.sleep(2.2)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            assert db.execute('SELECT content,use_count FROM clipboard_history WHERE id=?', (entry_id,)).fetchone() == (note, used + 1)
        print('Verified an editor retaining its original file cannot write through a changed privacy epoch')
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
        def client_size(xid):
            import re
            info = subprocess.check_output(['xwininfo','-id',str(xid)], text=True)
            return tuple(int(re.search(rf'^\s*{key}: (\d+)$', info, re.M).group(1)) for key in ('Width','Height'))
        assert client_size(original) == (660, 620), 'Existing profile size was not restored'
        settings_main.resize(689, 597)
        settings_main.get_display().sync()
        def saved_size(connection):
            return dict(connection.execute("SELECT key,value FROM settings WHERE key IN ('app.window_width','app.window_height')"))
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: saved_size(db) == {'app.window_width':'689', 'app.window_height':'597'},
                     'Resizing the real window did not persist the physical client size')
        settings_main.focus(Gdk.CURRENT_TIME)
        wait_for(lambda: subprocess.check_output(['xprop','-root','_NET_ACTIVE_WINDOW'], text=True).split()[-1]
                 == hex(original), 'Settings window did not receive focus')
        settings_main.resize(703, 585)
        settings_main.get_display().sync()
        wait_for(lambda: client_size(original) == (703, 585), 'Final native resize did not complete')
        shortcut((Gdk.KEY_Alt_L, Gdk.KEY_F4))
        wait_for(lambda: subprocess.run(['xwininfo','-id',str(original)], capture_output=True).returncode != 0,
                 'Settings WebView did not close')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: saved_size(db) == {'app.window_width':'703', 'app.window_height':'585'},
                     'Closing the WebView lost its final resize')
        shortcut()
        wait_for(lambda: find_item('Image'), 'New WebView did not restore the selected language')
        reopened = wait_for(settings_window, 'Cannot inspect recreated main window')
        measured = client_size(reopened.get_xid())
        assert measured == (703, 585), f'Recreated X11 client size {measured}, expected (703, 585)'
        print('Verified legacy window dimensions and a live resize survive main WebView destruction')
        selected = open_appearance('Settings', 'Appearance', 'Retro')
        assert selected.get_state_set().contains(atspi.StateType.PRESSED)
        opacity = wait_for(lambda: next((node for node,name,interfaces,_ in mygo_nodes(atspi, executable)
                                          if name == 'Preview opacity' and 'Value' in interfaces), None), 'Preview opacity slider')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            for value in (0, 100, 34):
                assert atspi.Value.set_current_value(opacity, value)
                wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.preview_opacity'").fetchone() == (str(value),),
                         'Preview opacity did not reach the Service')
        wait_for(lambda: find_item(family, text=True), 'New WebView did not restore the selected font family')
        print('Verified saved theme, color mode, language and installed UI font selection in a newly opened WebView')
        details = open_settings_group('About', 'View build details')
        wait_for(lambda: find_item('Portable copies need manual updates. Get the full package for your platform from the release page, keep the data directory, then replace program files.', text=True),
                 'MyGo ZIP manual update guidance missing')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            for label, key, value in [('STABLE','app.update_channel','stable'), ('PREVIEW','app.update_channel','preview'),
                                      ('Check for updates automatically','app.update_auto_check','true'),
                                      ('Check for updates automatically','app.update_auto_check','false')]:
                control = wait_for(lambda: find_item(label), label)
                assert atspi.Action.do_action(control, 0)
                wait_for(lambda: db.execute("SELECT value FROM settings WHERE key=?", (key,)).fetchone() == (value,),
                         'Updater setting did not reach the Service')
        assert atspi.Action.do_action(wait_for(lambda: find_item('Check now'), 'Manual update check button'), 0)
        wait_for(lambda: find_item('Cannot reach the update service. Check your connection and retry. The app still works offline.', text=True),
                 'Manual update check did not report the isolated proxy failure', timeout=32)
        print('Verified shared updater settings, manual ZIP guidance and offline channel check')
        assert atspi.Action.do_action(details, 0)
        build = json.loads(subprocess.check_output([executable, '--build-info']))
        for value in [build['mygo_version'], build['target'], build['build_id']]:
            wait_for(lambda: find_item(value, text=True), f'Main window did not report its own build detail {value}')
        open_settings_group('Data Management', 'EXPORT CLIPBOARD HISTORY')
        directory = str(Path(os.environ['DEZIRCLIP_DB_PATH']).parent)
        wait_for(lambda: find_item(directory, text=True), 'Settings did not display the active Rust profile')
        print('Verified About shows the running MyGo binary identity and Data Management shows the isolated Service profile')
        def main_pid():
            xid = visible_main_window(resident, executable)
            if xid:
                props = subprocess.run(['xprop','-id',str(xid),'_NET_WM_PID'], capture_output=True, text=True)
                value = props.stdout.split()[-1:] if props.returncode == 0 else []
                return int(value[0]) if value and value[0].isdecimal() else None
        def gpu_env(pid):
            keys = (b'WEBKIT_DISABLE_COMPOSITING_MODE', b'WEBKIT_DISABLE_DMABUF_RENDERER')
            return {key: value for field in Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
                    for key, _, value in [field.partition(b'=')] if key in keys}
        cli = executable.with_name('dzc')
        def service_id():
            return json.loads(subprocess.check_output([cli, 'service', 'status', '--json']))['state']['service_instance_id']
        original_service = service_id()
        resident_env = gpu_env(resident)
        # MyGo sets a default DMA-BUF policy during Linux resident initialization.
        # The original child includes it in its exec environment.
        inherited = gpu_env(wait_for(main_pid, 'No main window before GPU test'))
        for disabled in [True, False]:
            toggle_gpu = open_settings_group('General Settings', 'Disable WebView GPU')
            old_pid = wait_for(main_pid, 'Could not identify WebView before changing GPU')
            fd = os.pidfd_open(old_pid)
            try:
                assert atspi.Action.do_action(toggle_gpu, 0)
                wait_for(lambda: select.select([fd], [], [], 0)[0], 'Old WebView did not exit after GPU change')
            finally:
                os.close(fd)
            new_pid = wait_for(main_pid, 'No replacement main window after GPU change')
            assert new_pid != old_pid and gpu_env(resident) == resident_env and service_id() == original_service
            expected = {b'WEBKIT_DISABLE_COMPOSITING_MODE': b'1', b'WEBKIT_DISABLE_DMABUF_RENDERER': b'1'} if disabled else inherited
            observed = gpu_env(new_pid)
            assert observed == expected, f'GPU disabled={disabled}, new WebView {new_pid}: {observed}, expected {expected}'
            updated = open_settings_group('General Settings', 'Disable WebView GPU')
            wait_for(lambda: updated.get_state_set().contains(atspi.StateType.CHECKED) == disabled,
                     'Recreated WebView did not restore the committed GPU setting')
        print('Verified both GPU changes replace only the WebView and pass its WebKit launch policy')
        login = open_settings_group('General Settings', 'Launch at Startup')
        silent = wait_for(lambda: find_item('Silent Start'), 'Silent Start checkbox missing')
        hide_tray = wait_for(lambda: find_item('Hide Tray Icon'), 'Hide Tray Icon checkbox missing')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            for label, key, enabled in [('Preview with arrow keys', 'app.preview_keyboard_enabled', True),
                                        ('Preview with arrow keys', 'app.preview_keyboard_enabled', False),
                                        ('Tag manager', 'app.tag_manager_enabled', False),
                                        ('Tag manager', 'app.tag_manager_enabled', True),
                                        ('Sound Effects', 'app.sound_enabled', True),
                                        ('Paste Sound', 'app.sound_paste_enabled', False),
                                        ('Paste Sound', 'app.sound_paste_enabled', True),
                                        ('Sound Effects', 'app.sound_enabled', False)]:
                checkbox = wait_for(lambda: find_item(label), f'{label} checkbox missing')
                wait_for(lambda: checkbox.get_state_set().contains(atspi.StateType.SENSITIVE), f'{label} did not become editable')
                assert atspi.Action.do_action(checkbox, 0)
                wait_for(lambda: db.execute("SELECT value FROM settings WHERE key=?", (key,)).fetchone() == (str(enabled).lower(),)
                         and checkbox.get_state_set().contains(atspi.StateType.CHECKED) == enabled,
                         f'{label} did not reflect the committed setting')
            follow = wait_for(lambda: find_item('Follow Mouse Position'), 'Follow Mouse Position checkbox missing')
            assert follow.get_state_set().contains(atspi.StateType.CHECKED) and atspi.Action.do_action(follow, 0)
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.follow_mouse'").fetchone() == ('false',)
                     and not follow.get_state_set().contains(atspi.StateType.CHECKED), 'Follow Mouse setting did not persist')
            dock = wait_for(lambda: find_item('Edge Docking'), 'Edge Docking checkbox missing')
            window = wait_for(settings_window, 'Cannot identify window for docking')
            screen = Gdk.Screen.get_default()
            pointer = screen.get_display().get_default_seat().get_pointer()
            original = window.get_frame_extents()
            pointer.warp(screen, 50, 170)
            window.move(0,140)
            screen.get_display().sync()
            wait_for(lambda: window.get_frame_extents().x <= 5, 'Test window did not reach left edge')
            assert atspi.Action.do_action(dock, 0)
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.edge_docking'").fetchone() == ('true',),
                     'Edge docking preference not committed')
            assert atspi.generate_mouse_event(50, 170, 'b1p')
            try:
                pointer.warp(screen, screen.get_width()-15, 170)
                screen.get_display().sync()
                time.sleep(1.1)
                assert visible_main_window(resident, executable), 'Docked during a mouse drag'
            finally:
                assert atspi.generate_mouse_event(screen.get_width()-15, 170, 'b1r')
            wait_for(lambda: not visible_main_window(resident, executable), 'Left-edge window did not retract')
            pointer.warp(screen, 1, 20)
            screen.get_display().sync()
            time.sleep(.4)
            assert not visible_main_window(resident, executable), 'Unrelated part of screen edge revealed the window'
            pointer.warp(screen, 1, 180)
            screen.get_display().sync()
            wait_for(lambda: visible_main_window(resident, executable), 'Hover did not restore the docked window')
            assert atspi.Action.do_action(wait_for(lambda: find_item('Edge Docking'), 'Restored settings missing'), 0)
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.edge_docking'").fetchone() == ('false',),
                     'Turning off docking did not persist')
            pointer.warp(screen, screen.get_width()-15,180)
            screen.get_display().sync()
            time.sleep(1)
            assert visible_main_window(resident, executable), 'Disabled docking still concealed the window'
            window.move(original.x,original.y)
            screen.get_display().sync()
            print('Verified native edge docking from Settings, mouse drag suppression, bounded edge reveal and disabling')
            for hidden in [True, False]:
                assert atspi.Action.do_action(hide_tray, 0)
                wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.hide_tray_icon'").fetchone() == (str(hidden).lower(),)
                         and hide_tray.get_state_set().contains(atspi.StateType.CHECKED) == hidden
                         and tray_item(resident, '打开剪贴板', activate=False) != hidden,
                         'Resident tray visibility did not match the saved choice')
        print('Verified Hide Tray Icon removes and restores the live native tray from the shared WebView')
        assert not silent.get_state_set().contains(atspi.StateType.CHECKED)
        assert atspi.Action.do_action(silent, 0)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.silent_start'").fetchone() == ('true',)
                     and silent.get_state_set().contains(atspi.StateType.CHECKED), 'Silent Start did not persist')
        entry = Path(os.environ['XDG_CONFIG_HOME'])/'autostart/dezirclip.desktop'
        assert login.get_state_set().contains(atspi.StateType.CHECKED)
        assert atspi.Action.do_action(login, 0)
        wait_for(lambda: not entry.exists() and not login.get_state_set().contains(atspi.StateType.CHECKED),
                 'Turning off login did not remove the owned desktop entry')
        assert atspi.Action.do_action(login, 0)
        wait_for(lambda: entry.is_file() and login.get_state_set().contains(atspi.StateType.CHECKED),
                 'Turning on login did not register the MyGo desktop')
        assert json.loads(subprocess.check_output([executable.with_name('dzc'), 'desktop-tools', 'login-status']))
        open_settings_group('Data Management', 'EXPORT CLIPBOARD HISTORY')
        print('Verified the shared Launch at Startup checkbox reads and toggles the real isolated login registration')
        open_settings_group('Default Applications', 'Text')
        def text_handler():
            return next((item for item, name, _, _ in mygo_nodes(atspi, executable)
                         if name == 'Text' and item.get_role() == atspi.Role.COMBO_BOX), None)
        select_app = wait_for(text_handler, 'Text application selector unavailable')
        wait_for(lambda: any('dzc-system-fixture.desktop' in text for _, _, _, text in mygo_nodes(atspi, executable)),
                 'The operating system default text handler did not appear')
        atspi.Component.scroll_to(select_app, atspi.ScrollType.ANYWHERE)
        assert atspi.Component.grab_focus(select_app)
        for char in 'dzcfixture':
            shortcut((ord(char),))
        wait_for(lambda: any('DzcFixture Viewer' in (name + text) for _, name, _, text in mygo_nodes(atspi, executable)),
                 'Installed user application did not appear')
        shortcut((Gdk.KEY_Return,))
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            def saved_app():
                return db.execute("SELECT value FROM settings WHERE key='app.text'").fetchone()
            wait_for(lambda: saved_app() == ('/bin/cat',), 'Rust Service did not save the installed text handler')
            def focus_saved_selector():
                selector = text_handler()
                return selector and selector.get_state_set().contains(atspi.StateType.SENSITIVE) and atspi.Component.grab_focus(selector)
            wait_for(focus_saved_selector, 'Text application selector did not reactivate after saving')
            shortcut((Gdk.KEY_Down,))
            shortcut((Gdk.KEY_Home,))
            shortcut((Gdk.KEY_Return,))
            wait_for(lambda: saved_app() == ('',), 'Returning to the system handler did not clear the override')
        open_settings_group('Data Management', 'EXPORT CLIPBOARD HISTORY')
        print('Verified system association, selection of an installed application and restoration of the default')
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
        main_recorder = wait_for(lambda: find_item('Global Hotkey'), 'Main shortcut recorder unavailable')
        atspi.Component.scroll_to(main_recorder, atspi.ScrollType.ANYWHERE)
        assert atspi.Action.do_action(main_recorder, 0)
        wait_for(lambda: main_recorder.get_state_set().contains(atspi.StateType.PRESSED)
                 and not main_recorder.get_state_set().contains(atspi.StateType.BUSY),
                 'Main shortcut recorder did not acquire its recording connection')
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_v))
        wait_for(lambda: any('already registered' in name + text for _, name, _, text in mygo_nodes(atspi, executable)),
                 'Availability probe did not reject the resident quick-paste shortcut')
        assert find_item('Delete Ctrl+Shift+V') is None
        print('Verified the resident availability probe rejects its existing shortcut without adding a main binding', flush=True)
        assert atspi.Action.do_action(main_recorder, 0)
        wait_for(lambda: main_recorder.get_state_set().contains(atspi.StateType.PRESSED)
                 and not main_recorder.get_state_set().contains(atspi.StateType.BUSY),
                 'Main shortcut recorder did not restart for a new binding')
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F9))
        wait_for(lambda: find_item('Delete Ctrl+Shift+F9'), 'New main shortcut was not shown after saving')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.hotkey'").fetchone() == ('Ctrl+Shift+F8\nCtrl+Shift+F9',),
                     'Main shortcut list was not persisted by the Host')
        shortcut_window = settings_window().get_xid()
        alternate.present()
        wait_for(alternate.is_active, 'Independent editor did not regain focus before testing the new shortcut')
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F9))
        wait_for(lambda: int(subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True).split()[-1], 16) == shortcut_window,
                 'New main shortcut did not focus the shared WebView from another application')
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F9))
        wait_for(lambda: subprocess.run(['xwininfo', '-id', str(shortcut_window)], capture_output=True).returncode != 0,
                 'New main shortcut did not close the WebView')
        shortcut((Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F9))
        wait_for(lambda: find_item('Settings'), 'New main shortcut did not recreate the WebView')
        open_appearance('Settings', 'Clipboard', 'Global Hotkey')
        remove_main = wait_for(lambda: find_item('Delete Ctrl+Shift+F9'), 'Saved shortcut did not survive WebView recreation')
        assert atspi.Action.do_action(remove_main, 0)
        wait_for(lambda: not find_item('Delete Ctrl+Shift+F9'), 'Removed main shortcut remained in the editor')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.hotkey'").fetchone() == ('Ctrl+Shift+F8',),
                     'Removing a main shortcut did not preserve the original binding')
        custom_rules = wait_for(lambda: find_item('Custom Rules'), 'Clipboard settings did not survive shortcut editing')
        print('Verified main shortcut addition, persistence, native activation, WebView recreation and removal', flush=True)
        def begin_privacy_recording():
            button = wait_for(lambda: find_item('Toggle privacy session shortcut'), 'Privacy shortcut recorder unavailable')
            atspi.Component.scroll_to(button, atspi.ScrollType.ANYWHERE)
            assert atspi.Action.do_action(button, 0)
            wait_for(lambda: button.get_state_set().contains(atspi.StateType.PRESSED)
                     and not button.get_state_set().contains(atspi.StateType.BUSY),
                     'Privacy shortcut recorder did not acquire its native recording connection')
            return button
        privacy = wait_for(lambda: find_item('Privacy session'), 'Privacy session toggle unavailable')
        was_private = privacy.get_state_set().contains(atspi.StateType.CHECKED)
        for chord in [(Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_v),
                      (Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_a),
                      (Gdk.KEY_Control_L, Gdk.KEY_Shift_L, Gdk.KEY_F8)]:
            button = begin_privacy_recording()
            shortcut(chord)
            wait_for(lambda: not button.get_state_set().contains(atspi.StateType.PRESSED),
                     'Existing native shortcut was not routed into the recorder')
            wait_for(lambda: button.get_state_set().contains(atspi.StateType.SENSITIVE),
                     'Privacy shortcut validation did not finish')
            print(f'Verified native shortcut recording for {chord}', flush=True)
            assert privacy.get_state_set().contains(atspi.StateType.CHECKED) == was_private
            wait_for(lambda: find_item('Toggle privacy session shortcut'), 'Recording activated a native panel or hid the main window')
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            assert db.execute("SELECT value FROM settings WHERE key='app.privacy_session_hotkey'").fetchone() in (None, ('',))
        print('Verified main, quick-paste and screenshot shortcuts reach the recorder and remain unavailable to the privacy shortcut')
        snapshot = wait_for(lambda: find_item('Rich Text Snapshot Preview'), 'Snapshot setting unavailable')
        assert atspi.Action.do_action(snapshot, 0)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.rich_text_snapshot_preview'").fetchone() == ('true',),
                     'Rich text snapshots were not enabled')
            field = wait_for(lambda: next((i for i,n,_,_ in mygo_nodes(atspi, executable)
                             if n == 'Snapshot cache size' and i.get_state_set().contains(atspi.StateType.EDITABLE)), None),
                             'Snapshot cache size unavailable')
            atspi.Component.scroll_to(field, atspi.ScrollType.ANYWHERE)
            assert atspi.Component.grab_focus(field)
            shortcut((Gdk.KEY_Control_L, Gdk.KEY_a))
            shortcut((Gdk.KEY_0,))
            shortcut((Gdk.KEY_Return,))
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.rich_text_snapshot_cache_limit'").fetchone() == ('0',),
                     'Turning off snapshot caching did not persist')
            assert atspi.Action.do_action(snapshot, 0)
            wait_for(lambda: db.execute("SELECT value FROM settings WHERE key='app.rich_text_snapshot_preview'").fetchone() == ('false',),
                     'Turning off rich text snapshots did not persist')
        print('Verified the shared tag-manager switch, rich text snapshot switch and zero-cache setting persist through the Rust Service')
        persistent = wait_for(lambda: find_item('Persistent Storage'), 'Persistent storage switch unavailable')
        if not persistent.get_state_set().contains(atspi.StateType.CHECKED):
            assert atspi.Action.do_action(persistent, 0)
        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
            def saved(key):
                return db.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
            for value in ('false', 'true'):
                switch = wait_for(lambda: find_item('Enable Storage Limit'), 'Storage limit switch unavailable')
                wait_for(lambda: switch.get_state_set().contains(atspi.StateType.SENSITIVE), 'Storage limit switch busy')
                assert atspi.Action.do_action(switch, 0)
                wait_for(lambda: saved('app.persistent_limit_enabled') == (value,), 'Storage limit switch did not persist')
            field = wait_for(lambda: next((item for item, name, _, _ in mygo_nodes(atspi, executable)
                             if name == 'Storage Limit' and item.get_state_set().contains(atspi.StateType.EDITABLE)),
                             None), 'Storage limit field unavailable')
            atspi.Component.scroll_to(field, atspi.ScrollType.ANYWHERE)
            assert atspi.Component.grab_focus(field)
            shortcut((Gdk.KEY_Control_L, Gdk.KEY_a))
            for char in '52':
                shortcut((ord(char),))
            shortcut((Gdk.KEY_Return,))
            wait_for(lambda: saved('app.persistent_limit') == ('52',), 'Storage limit value did not persist')
        print('Verified shared storage limit switch and numeric field save through the MyGo WebView')
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
        assert tray_item(resident, '设置')
        choose = open_settings_group('Appearance', 'Choose Background')
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
        saved_window = settings_window().get_xid()
        shortcut((Gdk.KEY_Alt_L, Gdk.KEY_F4))
        wait_for(lambda: subprocess.run(['xwininfo', '-id', str(saved_window)], capture_output=True).returncode != 0,
                 'WebView did not close with its background saved')
        shortcut()
        wait_for(lambda: find_item('Stickers') and painted(), 'New WebView did not paint the saved background')
        clear_bg = open_appearance('Settings', 'Appearance', 'Clear Background')
        assert atspi.Action.do_action(clear_bg, 0)
        wait_for(lambda: action('Choose Background'), 'Clear did not remove the selected file')
        show_history()
        wait_for(lambda: not painted(), 'Clearing the background left its pixels visible')
        print('Verified selecting, visibly painting and clearing a custom background in the shared MyGo WebView')
        assert tray_item(resident, '设置')
        full = open_settings_group('Tools', 'Full Screen Capture')
        cli_path = executable.with_name('dzc')
        wait_for(lambda: any(str(cli_path) in text for _, _, _, text in mygo_nodes(atspi, executable)),
                 'Tools did not report the bundled Rust CLI')
        if not shutil.which('dzc'):
            add_path = wait_for(lambda: action('Add to PATH'), 'Tools did not offer to link the bundled CLI')
            assert atspi.Action.do_action(add_path, 0)
            link = Path(os.environ['HOME'])/'.local/bin/dzc'
            wait_for(lambda: link.is_symlink() and link.resolve() == cli_path,
                     'Tools did not link the bundled CLI in the isolated home')
            wait_for(lambda: not action('Add to PATH') and any('dzc (configured for this user)' in text
                     for _, _, _, text in mygo_nodes(atspi, executable)),
                     'Tools did not distinguish the saved user link from the current PATH')
            print('Verified Add to PATH links the actual sibling CLI in the isolated home')
        opened = Path(os.environ['HOME'])/'opened-guide'
        for guide, label in [('dzc-cli', 'CLI workflows'), ('dezirclip-theme', 'Theme authoring')]:
            button = wait_for(lambda: action(f'Open Skill folder: {label}'), f'{label} folder button unavailable')
            assert atspi.Action.do_action(button, 0)
            expected = executable.parent/'skills'/guide
            wait_for(lambda: opened.is_file() and opened.read_text().strip() == str(expected),
                     f'{label} button did not open the matching packaged guide')
        print('Verified both guide buttons passed the packaged folders to the desktop opener')
        wait_for(lambda: any('MyGo Native' in text for _, _, _, text in mygo_nodes(atspi, executable)),
                 'Tools did not identify the resident Quick Paste renderer')
        print('Verified the real tray Settings action in both a new and an existing MyGo WebView')
        def saved_panel(key):
            connection = sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH'])
            try:
                return connection.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
            finally:
                connection.close()
        for label, key in [('Screenshot', 'app.screenshot_enabled'), ('Quick Paste', 'app.quick_paste_enabled')]:
            for enabled in (False, True):
                control = wait_for(lambda: find_item(label), f'{label} switch unavailable')
                wait_for(lambda: control.get_state_set().contains(atspi.StateType.SENSITIVE), f'{label} switch busy')
                assert atspi.Action.do_action(control, 0)
                wait_for(lambda: saved_panel(key) == ('true' if enabled else 'false',), f'{label} switch did not persist')
                wait_for(lambda: control.get_state_set().contains(atspi.StateType.CHECKED) == enabled,
                         f'{label} switch did not acknowledge the saved state')
        full = wait_for(lambda: action('Full Screen Capture'), 'Screenshot controls did not return after enabling')
        print('Verified native panel switches persist and acknowledge changes in the shared MyGo WebView')
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


def check(desktop, pinned, audio, pin_preview=False):
    import gi
    gi.require_version('Atspi', '2.0')
    gi.require_version('Gtk', '3.0')
    from gi.repository import Atspi, Gtk

    cli = desktop.with_name('dzc')
    marker = 'MyGo desktop fixture ' + secrets.token_hex(12)
    subprocess.run([cli, 'add', marker], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.compact_mode','true')")
        db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.arrow_key_selection','true')")
        if not pinned:
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.preview_keyboard_enabled','true')")
        db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.silent_start','false')")
        if audio is not None:
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.sound_enabled','true')")
        if not pinned:
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.preview_opacity','0')")
        if pinned:
            db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                           [('app.window_width','660'),('app.window_height','620')])
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.custom_background_opacity','100')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.emoji_panel_enabled','true')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.window_pinned','true')")
            db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.hotkey','Ctrl+Shift+F8')")
    if pinned:
        installed = json.loads(subprocess.check_output([cli, 'desktop-tools', 'installed-apps'], timeout=12))
        assert any(app['path'] == '/linked-fixture' for app in installed)
        assert not any(app['path'] == '/oversized-fixture' for app in installed)
        (Path(os.environ['XDG_DATA_HOME'])/'applications/blocked.desktop').unlink()
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
    launch = [desktop]
    if pinned:
        from gi.repository import Gio, GLib
        # A second installation records the arguments handed to it by the real desktop-entry launcher.
        second = Path(os.environ['HOME'])/'second 配置 "%u$'
        second.mkdir()
        second_cli = second/'dzc'
        os.link(cli, second_cli)
        second_desktop = second/'dzc-desktop'
        second_desktop.write_text('#!/bin/sh\nprintf "%s\\n" "$0" "$@" > "$HOME/login-command"\n')
        second_desktop.chmod(0o700)
        def login_tool(binary, action):
            return json.loads(subprocess.check_output([binary, 'desktop-tools', action]))
        assert json.loads(subprocess.check_output([cli, 'desktop-tools', 'enable-login']))
        entry = Path(os.environ['XDG_CONFIG_HOME'])/'autostart/dezirclip.desktop'
        assert login_tool(second_cli, 'enable-login')
        assert not login_tool(cli, 'disable-login') and login_tool(second_cli, 'login-status')
        assert Gio.DesktopAppInfo.new_from_filename(str(entry)).launch([], None)
        receipt = Path(os.environ['HOME'])/'login-command'
        deadline = time.monotonic() + 5
        while not receipt.exists() or receipt.read_text().splitlines() != [str(second_desktop), '--minimized']:
            assert time.monotonic() < deadline, 'Desktop entry did not preserve the selected executable and arguments'
            time.sleep(.05)
        assert login_tool(cli, 'enable-login') and not login_tool(second_cli, 'disable-login')
        info = Gio.DesktopAppInfo.new_from_filename(str(entry))
        ok, launch = GLib.shell_parse_argv(info.get_commandline())
        assert ok and launch == [str(desktop), '--minimized'], 'Desktop login entry does not start this resident'
    from gi.repository import Gdk, GdkX11
    screen = Gdk.Screen.get_default()
    pointer = screen.get_display().get_default_seat().get_pointer()
    work = screen.get_display().get_primary_monitor().get_workarea()
    def move_pointer():
        pointer.warp(screen, work.x+work.width-14, work.y+64)
        screen.get_display().sync()

    def check_placement(follows):
        deadline = time.monotonic() + 12
        geometry = None
        while time.monotonic() < deadline:
            xid = visible_main_window(process.pid, desktop)
            if xid:
                frame = GdkX11.X11Window.foreign_new_for_display(screen.get_display(), xid)
                if frame:
                    geometry = frame.get_frame_extents()
                    right = geometry.x+geometry.width
                    aligned = abs(right-(work.x+work.width-5)) <= 5 and abs(geometry.y-(work.y+76)) <= 5
                    if follows and aligned or not follows and abs(right-(work.x+work.width)) > 30:
                        return
            time.sleep(.05)
        raise AssertionError(f'Expected follow={follows}, work area={work}, window={geometry}')
    move_pointer()
    process = subprocess.Popen(launch, stdout=subprocess.DEVNULL)
    try:
        if not pinned:
            check_placement(True)
        if pinned:
            deadline = time.monotonic() + 15
            while not tray_item(process.pid, '打开剪贴板', activate=False):
                assert process.poll() is None and time.monotonic() < deadline, 'Minimized resident did not provide a tray'
                time.sleep(.1)
            members = memory_sample('login before opening main window', [process.pid])
            assert all(Path(f'/proc/{pid}/exe').resolve() != desktop for pid in members if pid != process.pid)
            assert not any('WebKit' in Path(f'/proc/{pid}/exe').resolve().name for pid in members)
            assert tray_item(process.pid, '打开剪贴板')
            print('Verified minimized login keeps the resident alive without a main WebView until the tray is clicked')
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
                    check_pinned(row, Atspi, Gtk, marker, target, editor, desktop, process.pid)
                    assert tray_item(process.pid, '退出')
                    assert process.wait(timeout=15) == 0
                    # Launch normally from the same profile after changing the setting in the real UI.
                    process = subprocess.Popen([desktop], stdout=subprocess.DEVNULL)
                    deadline = time.monotonic() + 15
                    while not tray_item(process.pid, '打开剪贴板', activate=False):
                        assert process.poll() is None and time.monotonic() < deadline, 'Silent Start lost its tray'
                        time.sleep(.1)
                    members = memory_sample('ordinary startup with Silent Start enabled', [process.pid])
                    assert all(Path(f'/proc/{pid}/exe').resolve() != desktop for pid in members if pid != process.pid)
                    assert not any('WebKit' in Path(f'/proc/{pid}/exe').resolve().name for pid in members)
                    assert tray_item(process.pid, '打开剪贴板')
                    deadline = time.monotonic() + 15
                    while not any(name == 'Settings' for _, name, _, _ in mygo_nodes(Atspi, desktop)):
                        assert process.poll() is None and time.monotonic() < deadline, 'Tray did not reopen the silent desktop'
                        time.sleep(.1)
                    print('Verified ordinary Silent Start retains only the native resident, and its tray opens the main window')
                    # Xvfb exports AppIndicator objects but runs no notification-area host.
                    watcher = Gio.bus_get_sync(Gio.BusType.SESSION, None).call_sync(
                        'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner',
                        GLib.Variant('(s)', ('org.kde.StatusNotifierWatcher',)), None, Gio.DBusCallFlags.NONE, 2000, None)
                    assert watcher.unpack() == (False,)
                    for args, key in [([], ''), (['--show'], 'Ctrl+Shift+F8')]:
                        assert tray_item(process.pid, '退出')
                        assert process.wait(timeout=15) == 0
                        with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
                            db.execute("UPDATE settings SET value=? WHERE key='app.hotkey'", (key,))
                        move_pointer()
                        process = subprocess.Popen([desktop, *args], stdout=subprocess.DEVNULL)
                        deadline = time.monotonic() + 15
                        while not visible_main_window(process.pid, desktop):
                            assert process.poll() is None and time.monotonic() < deadline, f'No visible startup recovery for {args}'
                            time.sleep(.1)
                        check_placement(False)
                    print('Verified saved Follow Mouse off leaves reopened windows away from the pointer')
                    print('Verified ordinary startup without a tray host or shortcut and explicit --show with a registered shortcut')
                    return
                # The Host closes idle desktop streams after 30 seconds.
                # Exercise copy and both resident shortcuts after that deadline.
                if pin_preview:
                    check_preview(row, Atspi, (desktop, process.pid, marker))
                    return
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
        check_native_panels(marker, process, endpoint['pid'], target, editor, paste_row, audio)
        print('Verified independent MyGo WebView, visible synthetic history, native Copy action and clipboard text')
    finally:
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)
        subprocess.run([cli, 'service', 'stop'], check=True)
    check_storage_limit(cli)


def check_storage_limit(cli):
    rows = [{'id': 10000+i, 'content_type': 'text', 'content': f'Old test row {i}',
             'created_at': i+1, 'updated_at': i+1, 'is_pinned': i == 0,
             'tags': ['keep'] if i == 1 else []} for i in range(57)]
    backup = Path(os.environ['HOME'])/'retention.json'
    backup.write_text(json.dumps({'version':'dezirclip-export-v2','exported_at':1,'entries':rows}))
    subprocess.run([cli, 'import', backup, '--mode', 'merge'], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', [
            ('app.persistent', 'true'), ('app.persistent_limit', '50'), ('app.persistent_limit_enabled', 'false')])
        db.commit()
        subprocess.run([cli, 'service', 'start', '--json'], check=True, stdout=subprocess.DEVNULL)
        try:
            verify_storage_captures(db)
        finally:
            subprocess.run([cli, 'service', 'stop'], check=True)


def verify_storage_captures(db):
    def count():
        return db.execute("SELECT count(*) FROM clipboard_history WHERE is_pinned=0 AND COALESCE(tags,'[]')='[]'").fetchone()[0]
    def exists(content):
        return db.execute('SELECT 1 FROM clipboard_history WHERE content=?', (content,)).fetchone()
    for enabled in (False, True):
        if enabled:
            db.execute("UPDATE settings SET value='true' WHERE key='app.persistent_limit_enabled'")
            db.commit()
        marker = 'Retention capture ' + secrets.token_hex(8)
        subprocess.run(['xclip','-selection','clipboard','-i'], input=marker.encode(), check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if exists(marker) and (not enabled or count() == 50): break
            time.sleep(.05)
        assert exists(marker), 'Private X11 selection was not saved'
        assert (count() == 50 if enabled else count() > 50), 'Unexpected evictable history size'
    assert exists('Old test row 0') and exists('Old test row 1') and not exists('Old test row 2')
    print('Verified real captures enforce the enabled limit and preserve pinned and tagged rows')


def check_preview(button, atspi, pin=None):
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk, GdkX11
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
    def preview_bounds():
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            tree = subprocess.check_output(['xwininfo','-root','-tree'], text=True)
            for line in tree.splitlines():
                if '"DezirClip 悬浮预览"' in line:
                    xid = int(line.strip().split()[0], 16)
                    frame = GdkX11.X11Window.foreign_new_for_display(screen.get_display(), xid)
                    info = subprocess.run(['xwininfo', '-id', str(xid)], capture_output=True, text=True)
                    if frame and 'Map State: IsViewable' in info.stdout: return xid, frame.get_frame_extents()
            time.sleep(.05)
        raise AssertionError('No visible native preview for the selected history row')
    preview_id, hover = preview_bounds()
    assert (hover.x+hover.width <= rect.x or hover.x >= rect.x+rect.width or
            hover.y+hover.height <= rect.y or hover.y >= rect.y+rect.height), 'Native preview covered its trigger'
    frame = GdkX11.X11Window.foreign_new_for_display(screen.get_display(), preview_id)
    def clear_background_and_visible_ink():
        pixels = Gdk.pixbuf_get_from_window(frame, 0, 0, frame.get_width(), frame.get_height())
        if not pixels or not pixels.get_has_alpha(): return False
        data = pixels.get_pixels()
        stride = pixels.get_rowstride()
        width, height = pixels.get_width(), pixels.get_height()
        return (data[(height-4)*stride+(width-4)*4+3] == 0
                and any(data[y*stride+x*4+3] >= 220 for y in range(5,height-5) for x in range(5,width-5)))
    deadline = time.monotonic()+4
    while not clear_background_and_visible_ink():
        assert time.monotonic() < deadline, 'Zero preview opacity must clear only the background and leave opaque content'
        time.sleep(.05)
    assert active() == focus, 'Hover preview took keyboard focus'
    if pin:
        desktop, resident, marker = pin
        def wait(check, message):
            deadline = time.monotonic()+7
            while time.monotonic()<deadline:
                value=check()
                if value: return value
                time.sleep(.05)
            raise AssertionError((message, [(n,t[:80]) for _,n,_,t in mygo_nodes(atspi,desktop) if n or t]))
        def pinned_window():
            tree=subprocess.check_output(['xwininfo','-root','-tree'],text=True)
            return next((int(line.split()[0],16) for line in tree.splitlines() if '"DezirClip 固定预览"' in line),None)
        def click(x,y):
            assert atspi.generate_mouse_event(x,y,'b1c')
        def hover_button(names):
            for item,name,interfaces,_ in mygo_nodes(atspi,desktop):
                if name in names and 'Action' in interfaces:
                    box=atspi.Component.get_extents(item,atspi.CoordType.SCREEN)
                    if box.width>0 and box.height>0:
                        return box
        def opacity_slider():
            for item,name,interfaces,_ in mygo_nodes(atspi,desktop):
                if name in ('Background opacity','背景不透明度') and 'Value' in interfaces:
                    return atspi.Component.get_extents(item,atspi.CoordType.SCREEN)
        def set_opacity(fraction):
            slider=wait(opacity_slider,'No native opacity slider')
            x,y=slider.x+int(slider.width*fraction),slider.y+slider.height//2
            pointer.warp(screen,x,y)
            screen.get_display().sync()
            assert atspi.generate_mouse_event(x,y,'b1p')
            try:
                time.sleep(.15)
            finally:
                assert atspi.generate_mouse_event(x,y,'b1r')
            def saved():
                with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
                    value=int(db.execute("SELECT value FROM settings WHERE key='app.preview_opacity'").fetchone()[0])
                return value if abs(value-100*fraction)<12 else None
            return wait(saved,'Native opacity control did not persist through the Rust Host')
        def matches_opacity(xid,percent):
            win=GdkX11.X11Window.foreign_new_for_display(screen.get_display(),xid)
            pixels=Gdk.pixbuf_get_from_window(win,win.get_width()-4,win.get_height()-4,1,1)
            return pixels and pixels.get_has_alpha() and abs(pixels.get_pixels()[3]-round(percent*2.55))<=2
        saved=set_opacity(.4)
        wait(lambda:matches_opacity(preview_id,saved),'Hover background did not follow the native slider')
        close=wait(lambda:hover_button(('Close preview','关闭预览','關閉預覽')), 'Hover preview has no close button')
        click(close.x+close.width//2,close.y+close.height//2)
        wait(lambda:not visible(),'Hover close button did not dismiss the preview')
        assert active()==focus,'Closing the hover preview took focus'
        pointer.warp(screen,rect.x+rect.width//2,rect.y+rect.height//2)
        screen.get_display().sync()
        preview_id,hover=preview_bounds()
        wait(lambda:matches_opacity(preview_id,saved),'Reopened hover preview did not use committed opacity')
        pin_box=wait(lambda:hover_button(('Pin this preview','固定此预览','固定此預覽')), 'Hover preview has no pin button')
        x,y=pin_box.x+pin_box.width//2,pin_box.y+pin_box.height//2
        pointer.warp(screen,x,y)
        screen.get_display().sync()
        time.sleep(.9)
        assert visible(), 'Hover preview closed before its pin button could be clicked'
        click(x,y)
        xid=wait(pinned_window,'Pin button did not create a resident preview')
        assert str(resident) in subprocess.check_output(['xprop','-id',str(xid),'_NET_WM_PID'],text=True)
        wait(lambda: '_NET_WM_STATE_ABOVE' in subprocess.check_output(['xprop','-id',str(xid),'_NET_WM_STATE'],text=True), 'Window manager did not pin the preview above other windows')
        main_xid=int(focus.split()[-1],16)
        main_pid=int(subprocess.check_output(['xprop','-id',str(main_xid),'_NET_WM_PID'],text=True).split()[-1])
        def close_window(window_id):
            window=GdkX11.X11Window.foreign_new_for_display(screen.get_display(),window_id)
            window.focus(Gdk.CURRENT_TIME)
            screen.get_display().sync()
            wait(lambda:int(active().split()[-1],16)==window_id,'Window to close did not gain focus')
            keymap=Gdk.Keymap.get_for_display(screen.get_display())
            for pressed in (True,False):
                for key in ((Gdk.KEY_Alt_L,Gdk.KEY_F4) if pressed else (Gdk.KEY_F4,Gdk.KEY_Alt_L)):
                    ok,codes=keymap.get_entries_for_keyval(key)
                    assert ok and atspi.generate_keyboard_event(codes[0].keycode,None,
                        atspi.KeySynthType.PRESS if pressed else atspi.KeySynthType.RELEASE)
        close_window(main_xid)
        wait(lambda: not Path(f'/proc/{main_pid}').exists(), 'Main WebView process did not exit')
        assert pinned_window()==xid
        wait(lambda:matches_opacity(xid,saved),'Pin did not inherit the hover opacity')
        saved=set_opacity(.75)
        wait(lambda:matches_opacity(xid,saved),'Pinned opacity failed after main WebView exit')
        def native_text(value):
            return next((i for i,n,_,t in mygo_nodes(atspi,desktop) if value in (n,t) or value in t),None)
        wait(lambda:native_text(marker),'Pinned native window did not retain the selected history')
        cli=desktop.with_name('dzc')
        subprocess.run([cli,'privacy','on'],check=True)
        wait(lambda: not native_text(marker),'Pinned native window retained private content')
        subprocess.run([cli,'privacy','off'],check=True)
        assert pinned_window()==xid and not native_text(marker)
        def native_button(names):
            return next((i for i,n,interfaces,_ in mygo_nodes(atspi,desktop) if n in names and 'Action' in interfaces),None)
        retry=wait(lambda:native_button(('Refresh state','重新读取','重新讀取')), 'Concealed preview has no restore control')
        retry_box=atspi.Component.get_extents(retry,atspi.CoordType.SCREEN)
        click(retry_box.x+retry_box.width//2,retry_box.y+retry_box.height//2)
        wait(lambda:native_text(marker),'An explicit retry did not restore the original row')
        close=wait(lambda:native_button(('Close preview','关闭预览','關閉預覽')), 'Pinned preview has no close control')
        close_box=atspi.Component.get_extents(close,atspi.CoordType.SCREEN)
        click(close_box.x+close_box.width//2,close_box.y+close_box.height//2)
        wait(lambda: not pinned_window(), 'Pinned preview did not close')
        print('Verified resident native pin, main WebView closure, original content, privacy concealment and explicit restore')
        return
    pointer.warp(screen, 0, 0)
    screen.get_display().sync()
    deadline = time.monotonic() + 5
    while visible():
        assert time.monotonic() < deadline, 'Hover preview did not close after leaving the row'
        time.sleep(.05)
    print('Verified native hover preview has transparent background and opaque content at 0%, keeps focus and dismisses on leave')
    keymap = Gdk.Keymap.get_for_display(screen.get_display())
    mapped, entries = keymap.get_entries_for_keyval(Gdk.KEY_Down)
    assert mapped and atspi.generate_keyboard_event(entries[0].keycode, None, atspi.KeySynthType.PRESSRELEASE)
    _, keyboard = preview_bounds()
    assert active() == focus
    assert abs(keyboard.x-hover.x) <= 16 and abs(keyboard.y-hover.y) <= rect.height + 16, 'Keyboard preview followed the distant pointer'
    assert not (keyboard.x <= rect.x+rect.width//2 < keyboard.x+keyboard.width and
                keyboard.y <= rect.y+rect.height//2 < keyboard.y+keyboard.height), 'Preview covered the selected row'
    print('Verified arrow-key preview stayed beside the selected history while the pointer remained at the screen corner')


def check_idle(executable):
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi
    cli = executable.with_name('dzc')
    subprocess.run([cli, 'add', 'Idle settings fixture'], check=True)
    db = sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH'])
    with db:
        db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                       [('app.language','en'), ('app.idle_destroy_enabled','false'), ('app.idle_destroy_seconds','5')])
    subprocess.run([cli, 'service', 'start', '--no-capture', '--json'], check=True)
    process = subprocess.Popen([executable, '--show'])
    def wait(probe, message):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            result = probe()
            if result: return result
            time.sleep(.06)
        raise AssertionError(message)
    def visible(): return visible_main_window(process.pid, executable)
    def hide():
        control = wait(lambda: next((item for item,name,interfaces,_ in mygo_nodes(Atspi,executable)
                                     if name == 'Hide' and 'Action' in interfaces), None), 'Missing Hide button')
        assert Atspi.Action.do_action(control, 0)
        wait(lambda: not visible(), 'Manual hide did not hide the main WebView')
    def show():
        wait(lambda: tray_item(process.pid, '打开剪贴板'), 'Resident tray could not reopen history')
        return wait(visible, 'Main window did not return')
    try:
        xid = wait(visible, 'Initial WebView missing')
        pid = int(subprocess.check_output(['xprop','-id',str(xid),'_NET_WM_PID'],text=True).split()[-1])
        hide()
        time.sleep(5.5)
        assert Path(f'/proc/{pid}').exists() and show() == xid, 'Disabled idle release destroyed the WebView'
        with db:
            db.execute("UPDATE settings SET value='true' WHERE key='app.idle_destroy_enabled'")
        hide()
        time.sleep(1)
        assert show() == xid, 'Reopening before the deadline replaced the WebView'
        time.sleep(5)
        assert visible() == xid, 'An expired idle timer closed a reopened WebView'
        hide()
        wait(lambda: not Path(f'/proc/{pid}').exists(), 'Hidden WebView process survived the idle deadline')
        replacement = show()
        next_pid = int(subprocess.check_output(['xprop','-id',str(replacement),'_NET_WM_PID'],text=True).split()[-1])
        assert process.poll() is None and next_pid != pid, 'Resident did not recreate the main WebView'
        print('Verified idle release off, cancel on reopen, expiration and recreation with the same resident')
    finally:
        db.close()
        process.terminate()
        process.wait(timeout=10)
        subprocess.run([cli,'service','stop'],check=True)


def check_profile_move(executable, home):
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi
    assert sys.platform.startswith('linux') and home == Path.home() and Path(os.environ['XDG_DATA_HOME']) == home/'data'
    os.environ.pop('DEZIRCLIP_DB_PATH', None)
    os.environ.pop('TIEZ_DB_PATH', None)
    os.environ['PULSE_SERVER'] = f'unix:{home}/no-audio'
    package = home/'MyGo profile fixture'
    package.mkdir()
    for name in ('dzc', 'dzc-service', executable.name):
        shutil.copy2(executable.with_name(name), package/name)
    executable = package/executable.name
    cli = package/'dzc'
    def run(*args):
        return subprocess.check_output([cli, *args], text=True, timeout=15)
    marker = 'Resident relocation ' + secrets.token_hex(8)
    run('add', marker)
    config = home/'data/io.github.qiyuanhuakai.dezirclip'
    with sqlite3.connect(config/'clipboard.db') as db:
        db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)", [
            ('app.language', 'en'), ('app.silent_start', 'false'), ('app.window_pinned', 'true'),
            ('app.window_width','740'), ('app.window_height','720')])
    parent = home/'profiles'
    target = parent/'destination'
    target.mkdir(parents=True)
    bookmarks = home/'config/gtk-3.0/bookmarks'
    bookmarks.parent.mkdir(exist_ok=True)
    bookmarks.write_text(parent.as_uri()+' Test profiles\n')
    obstruction = target/'clipboard.db-journal'
    obstruction.write_bytes(b'fixture')
    original = json.loads(run('service', 'start', '--no-capture', '--json'))['state']['service_instance_id']
    process = subprocess.Popen([executable, '--show'])
    def nodes(): return mygo_nodes(Atspi, executable)
    def wait(probe, message):
        deadline = time.monotonic() + 9
        while time.monotonic() < deadline:
            result = probe()
            if result: return result
            time.sleep(.06)
        raise AssertionError((message, [(n,t[:60]) for _,n,_,t in nodes() if n or t]))
    def button(name):
        return next((i for i,n,f,_ in nodes() if n.casefold() == name.casefold() and 'Action' in f), None)
    def click(name):
        assert Atspi.Action.do_action(wait(lambda: button(name), name), 0)
    def labeled(name):
        return next((i for i,n,_,t in nodes() if n == name or t == name), None)
    def select_target():
        click('Change')
        def native_open():
            return next((i for i,n,f,_ in nodes() if n == 'Open' and i.get_role() == Atspi.Role.PUSH_BUTTON
                         and 'Action' in f and i.get_state_set().contains(Atspi.StateType.SENSITIVE)), None)
        wait(lambda: labeled('Change Data Path'), 'Directory chooser')
        def click_row(label):
            def visible_row():
                node = labeled(label)
                if node:
                    Atspi.Component.scroll_to(node, Atspi.ScrollType.ANYWHERE)
                    box = Atspi.Component.get_extents(node, Atspi.CoordType.SCREEN)
                    if box.width > 0 and box.height > 0:
                        return box
            box = wait(visible_row, f'{label} directory row did not finish layout')
            assert Atspi.generate_mouse_event(box.x+box.width//2, box.y+box.height//2, 'b1c')
        click_row('Test profiles')
        click_row('destination')
        assert Atspi.Action.do_action(wait(native_open, 'Open directory'), 0)
        click('Confirm')
    def residents():
        for proc in Path('/proc').glob('[0-9]*'):
            try:
                if (proc/'exe').resolve(strict=True) == executable and b'--main-window' not in (proc/'cmdline').read_bytes():
                    yield int(proc.name)
            except (ProcessLookupError, FileNotFoundError, PermissionError):
                continue
    try:
        wait(lambda: labeled(marker), 'Original history')
        wait(lambda: tray_item(process.pid, '设置'), 'Settings tray item')
        heading = wait(lambda: labeled('Data Management'), 'Data settings')
        if not button('Change'):
            Atspi.Component.scroll_to(heading, Atspi.ScrollType.ANYWHERE)
            box = Atspi.Component.get_extents(heading, Atspi.CoordType.SCREEN)
            assert Atspi.generate_mouse_event(box.x+box.width//2, box.y+box.height//2, 'b1c')
        select_target()
        click('Ok')
        assert json.loads(run('service','status','--json'))['state']['service_instance_id'] == original
        assert obstruction.read_bytes() == b'fixture' and not (config/'datapath.txt').exists()
        obstruction.unlink()
        service = package/'dzc-service'
        unavailable = package/'dzc-service.unavailable'
        service.rename(unavailable)
        select_target()
        wait(lambda: labeled('Data moved. Reconnect to the new Service to continue.'), 'Committed relocation dialog')
        assert Path((config/'datapath.txt').read_text()) == target
        sentinel = 'Destination only ' + secrets.token_hex(8)
        run('add', sentinel)
        click('Ok')
        wait(lambda: any('retry the connection' in (n+t) for _,n,_,t in nodes()), 'Reconnection failure dialog')
        click('Ok')
        # Close only this fixture's WebView. The surviving tray must recover without the old child.
        windows = {int(pid) for thread in Path(f'/proc/{process.pid}/task').glob('*/children')
                   for pid in thread.read_text().split()
                   if Path(f'/proc/{pid}/exe').resolve() == executable
                   and b'--main-window' in Path(f'/proc/{pid}/cmdline').read_bytes()}
        assert len(windows) == 1
        child = windows.pop()
        os.kill(child, 15)
        wait(lambda: not Path(f'/proc/{child}').exists(), 'Old WebView exited')
        unavailable.rename(service)
        wait(lambda: tray_item(process.pid, '设置'), 'Retry from resident tray')
        process.wait(timeout=12)
        replacement = wait(lambda: next((p for p in residents() if visible_main_window(p, executable)), None), 'Replacement main window')
        assert replacement != process.pid
        wait(lambda: labeled(marker), 'Migrated history in new WebView')
        wait(lambda: labeled(sentinel), 'Destination-only history survived reconnection')
        status = json.loads(run('service','status','--json'))
        assert status['state']['service_instance_id'] != original and not status['capture_owner']
        assert Path((config/'datapath.txt').read_text()) == target and (config/'clipboard.db').is_file()
        print('Verified MyGo Settings folder selection, pre-commit rollback, post-commit restart failure, tray recovery after WebView exit, destination-only history and preserved capture mode')
    finally:
        for pid in residents():
            try: os.kill(pid, 15)
            except ProcessLookupError: pass
        process.wait(timeout=10)
        run('service', 'stop')


if __name__ == '__main__':
    desktop = Path(sys.argv[1]).resolve(strict=True)
    if len(sys.argv) == 2 or sys.argv[2] in ('--pinned', '--move-profile', '--pin-preview', '--idle'):
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
            if '--pinned' in sys.argv[2:]:
                env.update(HTTPS_PROXY='http://127.0.0.1:0', NO_PROXY='', no_proxy='')
                applications = home/'data/applications'
                applications.mkdir()
                os.mkfifo(applications/'blocked.desktop')
                with (applications/'oversized.desktop').open('wb') as oversized:
                    oversized.write(b'[Desktop Entry]\nType=Application\nName=Oversized Fixture\nExec=/oversized-fixture\n')
                    oversized.truncate(2 * 1024 * 1024)
                descriptor = home/'installed-viewer'
                descriptor.write_text('[Desktop Entry]\nType=Application\nName=Linked Fixture\nExec=/linked-fixture\n')
                (applications/'linked.desktop').symlink_to(descriptor)
                fixture_editor = home/'fixture-editor'
                fixture_editor.write_text("""#!/usr/bin/python3
from pathlib import Path
import sys, time
with open(sys.argv[1], 'r+') as document:
    document.seek(0, 2)
    document.write(' externally edited')
    document.flush()
    deadline = time.monotonic() + 25
    while not (Path.home() / 'finish-edit').exists() and time.monotonic() < deadline:
        time.sleep(.04)
    document.write(' private edit')
    document.flush()
    (Path.home() / 'late-edit-written').touch()
""")
                fixture_editor.chmod(0o700)
                for name, label, program in [('dzc-system-fixture', 'System text fixture', str(fixture_editor)),
                                             ('dzc-viewer-fixture', 'DzcFixture Viewer', '/bin/cat')]:
                    (applications/(name + '.desktop')).write_text(
                        f'[Desktop Entry]\nType=Application\nName={label}\nExec={program} %f\nMimeType=text/plain;\n')
                (home/'config/mimeapps.list').write_text('[Default Applications]\ntext/plain=dzc-system-fixture.desktop;\n')
                package = home/'MyGo test package'
                package.mkdir()
                for binary in (desktop, desktop.with_name('dzc'), desktop.with_name('dzc-service')):
                    try:
                        os.link(binary, package/binary.name)
                    except OSError as error:
                        if error.errno != errno.EXDEV: raise
                        shutil.copy2(binary, package/binary.name)
                desktop = package/desktop.name
                root = Path(__file__).resolve().parents[2]
                for relative, source in runpy.run_path(str(root/'scripts/package-dzc.py'))['skill_files'](root).items():
                    destination = package/relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(source, destination)
                recorder = home/'bin/xdg-open'
                recorder.parent.mkdir()
                recorder.write_text('#!/bin/sh\nprintf "%s\\n" "$1" > "$HOME/opened-guide"\n')
                recorder.chmod(0o700)
                env['PATH'] = str(recorder.parent) + os.pathsep + env['PATH']
            display_options = ['-s','-screen 0 800x600x24'] if '--pin-preview' in sys.argv else []
            subprocess.run(['xvfb-run', '-a', *display_options, 'dbus-run-session', '--', sys.executable, __file__, desktop, path, *sys.argv[2:]],
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
            pinned = '--pinned' in sys.argv[3:]
            if '--idle' in sys.argv[3:]:
                check_idle(desktop)
            elif '--move-profile' in sys.argv[3:]:
                check_profile_move(desktop, home)
            else:
                with (contextlib.nullcontext() if pinned or '--pin-preview' in sys.argv else private_audio(home)) as audio:
                    check(desktop, pinned, audio, '--pin-preview' in sys.argv)
        finally:
            wm.terminate()
            wm.wait(timeout=5)
