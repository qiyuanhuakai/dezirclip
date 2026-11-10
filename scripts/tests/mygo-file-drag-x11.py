"""Actual GTK URI-list transfers in the existing private Xvfb desktop fixture."""
import ctypes
import os
from pathlib import Path
import sqlite3
import subprocess
import time


def check(desktop, helpers):
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('Atspi', '2.0')
    from gi.repository import Gtk, Gdk, Atspi, GdkPixbuf
    cli = desktop.with_name('dzc')
    subprocess.run([cli, 'add', 'native file drag fixture'], check=True, stdout=subprocess.DEVNULL)
    with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
        db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', [
            ('app.window_pinned', 'true'), ('app.silent_start', 'false'), ('app.language', 'en'),
            ('app.window_width', '650'), ('app.window_height', '600')])
    files = [Path.home()/'drag 文件 with spaces.txt', Path.home()/'drag-image.png']
    files[0].write_text('Native copy fixture')
    bitmap = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8, 24, 24)
    bitmap.fill(0x3399ffff)
    bitmap.savev(str(files[1]), 'png', [], [])
    uris = [path.as_uri() for path in files]
    target = Gtk.Window(title='Independent native file transfer')
    target.set_default_size(270, 270)
    panel = Gtk.EventBox()
    panel.add(Gtk.Label(label='Native file source and target'))
    target.add(panel)
    targets = [Gtk.TargetEntry.new('text/uri-list', 0, 0)]
    panel.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, targets, Gdk.DragAction.COPY)
    panel.connect('drag-data-get', lambda widget, context, selection, info, stamp: selection.set_uris(uris))
    panel.drag_dest_set(Gtk.DestDefaults.ALL, targets, Gdk.DragAction.COPY)
    received = []
    panel.connect('drag-data-received', lambda widget, context, x, y, selection, info, stamp: received.append(selection.get_uris()))
    target.show_all()
    target.move(10, 50)
    x11, xtest = ctypes.CDLL('libX11.so.6'), ctypes.CDLL('libXtst.so.6')
    ptr, xid = ctypes.c_void_p, ctypes.c_ulong
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [ctypes.c_char_p], ptr
    x11.XSync.argtypes = [ptr, ctypes.c_int]
    x11.XCloseDisplay.argtypes = [ptr]
    x11.XMoveWindow.argtypes = [ptr, xid, ctypes.c_int, ctypes.c_int]
    x11.XKeysymToKeycode.argtypes = [ptr, xid]
    xtest.XTestFakeMotionEvent.argtypes = [ptr, ctypes.c_int, ctypes.c_int, ctypes.c_int, xid]
    xtest.XTestFakeButtonEvent.argtypes = [ptr, ctypes.c_uint, ctypes.c_int, xid]
    xtest.XTestFakeKeyEvent.argtypes = [ptr, ctypes.c_uint, ctypes.c_int, xid]
    display = x11.XOpenDisplay(None)
    assert display

    def pump(seconds=.05):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            while Gtk.events_pending(): Gtk.main_iteration_do(False)
            time.sleep(.01)

    def wait(predicate, message):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            pump()
            result = predicate()
            if result: return result
        tree = [(name, text[:120]) for _, name, _, text in helpers['mygo_nodes'](Atspi, desktop) if name or text]
        raise AssertionError(f'{message}; accessibility={tree}')

    def point(label):
        for node, name, interfaces, text in helpers['mygo_nodes'](Atspi, desktop):
            if label not in (name, text) or 'Component' not in interfaces: continue
            rect = Atspi.Component.get_extents(node, Atspi.CoordType.SCREEN)
            if rect.width > 0 and rect.height > 0:
                return (rect.x + rect.width//2, rect.y + rect.height//2)
        return None

    def gesture(start, end, cancel=False):
        xtest.XTestFakeMotionEvent(display, -1, *start, 0)
        x11.XSync(display, False)
        pump(.1)
        xtest.XTestFakeButtonEvent(display, 1, True, 0)
        x11.XSync(display, False)
        pump(.1)
        for index in range(1, 31):
            x = start[0] + (end[0]-start[0])*index//30
            y = start[1] + (end[1]-start[1])*index//30
            xtest.XTestFakeMotionEvent(display, -1, x, y, 0)
            x11.XSync(display, False)
            pump(.04)
        pump(.5)
        if cancel:
            key = x11.XKeysymToKeycode(display, 0xff1b)
            xtest.XTestFakeKeyEvent(display, key, True, 0)
            xtest.XTestFakeKeyEvent(display, key, False, 0)
        xtest.XTestFakeButtonEvent(display, 1, False, 0)
        x11.XSync(display, False)
        pump(.3)

    process = subprocess.Popen([desktop])
    try:
        main = wait(lambda: helpers['visible_main_window'](process.pid, desktop), 'Main window missing')
        x11.XMoveWindow(display, main, 350, 20)
        x11.XSync(display, False)
        drop = wait(lambda: point('native file drag fixture'), 'Initial history missing')
        origin = panel.get_window().get_origin()
        outside = (origin[-2] + 100, origin[-1] + 100)
        print(f'Native file fixture positions: source={outside}, destination={drop}', flush=True)
        pump(.3)
        gesture(outside, drop)
        row = wait(lambda: point('2 items'), 'Native file drop did not enter history')
        clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
        selection = clipboard.wait_for_contents(Gdk.Atom.intern('text/uri-list', False))
        assert selection and selection.get_uris() == uris, 'Drop must copy original file URIs'
        panel.drag_source_unset()
        gesture(row, outside)
        wait(lambda: received, 'Native card drag did not reach independent GTK target')
        assert received == [uris], received
        assert files[0].read_text() == 'Native copy fixture' and files[1].is_file()
        received.clear()
        gesture(row, outside, cancel=True)
        pump(.5)
        assert not received, 'Cancelled drag delivered files'
        selection = clipboard.wait_for_contents(Gdk.Atom.intern('text/uri-list', False))
        assert selection and selection.get_uris() == uris, 'Dragging changed the clipboard'
        image = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8, 240, 160)
        image.fill(0x6699ffff)
        clipboard.set_image(image)
        row = wait(lambda: point('Clipboard Image'), 'Captured image card missing')
        gesture(row, outside)
        wait(lambda: received, 'Image card did not export a native file')
        from urllib.parse import urlparse, unquote
        assert len(received) == 1 and len(received[0]) == 1
        export = Path(unquote(urlparse(received[0][0]).path))
        assert export.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        assert clipboard.wait_is_image_available(), 'Dragging must preserve the clipboard'
        print('Native Linux multi-file copy, Unicode paths, image attachment and cancellation passed')
    finally:
        with helpers['observed_service_cleanup'](cli):
            process.terminate()
            process.wait(timeout=20)
        target.destroy()
        x11.XCloseDisplay(display)
