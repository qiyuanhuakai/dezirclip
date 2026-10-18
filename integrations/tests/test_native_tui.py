"""Real Windows ConPTY paste boundaries with disposable history and console state."""

from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest


WRAPPER = r'''
import ctypes, json, pathlib, subprocess, sys, threading, time
from ctypes import wintypes as w
k = ctypes.WinDLL('kernel32', use_last_error=True)
k.CreateFileW.restype = w.HANDLE
k.CreateFileW.argtypes = [w.LPCWSTR,w.DWORD,w.DWORD,ctypes.c_void_p,w.DWORD,w.DWORD,w.HANDLE]
k.GetConsoleMode.argtypes = [w.HANDLE,ctypes.POINTER(w.DWORD)]
k.CloseHandle.argtypes = [w.HANDLE]
def modes():
    values = []
    for name in ('CONIN$', 'CONOUT$'):
        handle = k.CreateFileW(name, 0xC0000000, 3, None, 3, 0, None)
        mode = w.DWORD()
        assert k.GetConsoleMode(handle, ctypes.byref(mode))
        values.append(mode.value)
        k.CloseHandle(handle)
    return values + [k.GetConsoleCP(), k.GetConsoleOutputCP()]
before = modes()
seen_vt = [False]
stop = threading.Event()
def monitor():
    while not stop.wait(.01):
        seen_vt[0] |= bool(modes()[0] & 0x200)
observer = threading.Thread(target=monitor, daemon=True)
observer.start()
with open(sys.argv[1], 'wb') as output, open(sys.argv[2], 'wb') as error:
    result = subprocess.run(sys.argv[4:], stdout=output, stderr=error)
stop.set()
observer.join()
pathlib.Path(sys.argv[3]).write_text(json.dumps({
    'code':result.returncode, 'before':before, 'after':modes(), 'seen_vt':seen_vt[0]
}))
with open('CONOUT$', 'wb', buffering=0) as terminal:
    terminal.write(b'DZC_TUI_TEST_DONE\r\n')
'''


@unittest.skipUnless(sys.platform == "win32" and os.environ.get("DZC_TEST_BIN")
                     and importlib.util.find_spec("winpty"),
                     "Native Windows TUI needs DZC_TEST_BIN and optional pywinpty")
class NativeTuiPasteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dzc-native-tui-paste-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.binary = self.directory / "dzc.exe"
        shutil.copy2(os.environ["DZC_TEST_BIN"], self.binary)
        self.database = self.directory / "history.db"
        self.environment = dict(os.environ, DEZIRCLIP_DB_PATH=str(self.database))
        self.wrapper = self.directory / "wrapper.py"
        self.wrapper.write_text(WRAPPER, encoding="utf-8")
        self.output = self.directory / "stdout.bin"
        self.error = self.directory / "stderr.bin"
        self.result = self.directory / "result.json"
        self.cli("add", "hello 中文🙂")
        with closing(sqlite3.connect(self.database)) as database:
            self.identifier = database.execute("SELECT id FROM clipboard_history").fetchone()[0]
        self.process = None
        self.screen = ""

    def cli(self, *arguments):
        return subprocess.run([str(self.binary), *arguments], env=self.environment,
                              capture_output=True, check=True, timeout=15).stdout

    def row(self):
        with closing(sqlite3.connect(self.database)) as database:
            row = database.execute("SELECT is_pinned,tags FROM clipboard_history WHERE id=?",
                                   [self.identifier]).fetchone()
            return None if row is None else (row[0], json.loads(row[1]))

    def start(self):
        from winpty import PtyProcess
        self.screen = ""
        for artifact in (self.result, self.output, self.error):
            artifact.unlink(missing_ok=True)
        self.process = PtyProcess.spawn(
            [sys.executable, str(self.wrapper), str(self.output), str(self.error),
             str(self.result), str(self.binary), "tui", "--action", "id"],
            env=self.environment, dimensions=(30, 110), backend="0")
        self.process.fileobj.settimeout(.1)
        self.addCleanup(self.close)
        self.wait("Preview | tags:")

    def read(self):
        try:
            chunk = self.process.read(65536)
        except (socket.timeout, EOFError):
            return
        self.screen += chunk
        if "\x1b[6n" in chunk:
            self.process.write("\x1b[1;1R")

    def wait(self, needle, start=0, timeout=15):
        deadline = time.monotonic() + timeout
        while needle not in self.screen[start:] and time.monotonic() < deadline:
            self.read()
        self.assertIn(needle, self.screen[start:], self.screen[-5000:])

    def send_wait(self, text, needle):
        start = len(self.screen)
        self.process.write(text)
        self.wait(needle, start)

    def settle(self, duration=.4):
        deadline = time.monotonic() + duration
        while time.monotonic() < deadline:
            self.read()

    def paste(self, value):
        self.process.write("\x1b[200~" + value + "\x1b[201~")
        self.settle()

    def finish(self, expected_code=1, timeout=15):
        self.wait("DZC_TUI_TEST_DONE", timeout=timeout)
        state = json.loads(self.result.read_text())
        self.assertEqual(state["code"], expected_code, self.error.read_bytes())
        self.assertEqual(state["before"], state["after"], state)
        self.assertTrue(state["seen_vt"], "Windows TUI must use VT input, not key-record paste guessing")
        self.assertIn("\x1b[?2004h", self.screen)
        self.assertIn("\x1b[?2004l", self.screen)
        self.assertIn("\x1b[?1049l", self.screen)
        if expected_code:
            self.assertEqual(self.output.read_bytes(), b"")

    def close(self):
        if self.process is not None:
            if self.process.isalive():
                self.process.write("\x03")
                self.settle(.15)
            if self.process.isalive():
                self.process.write("q")
                self.settle(.15)
            self.process.close(force=True)
            self.process.fileobj.close()
            self.process._server.close()
            self.process = None

    def test_search_paste_cannot_submit_then_delete_the_matching_entry(self):
        self.start()
        self.send_wait("/", "Search (empty = all):")
        self.paste("hello\rdyes\r")
        self.assertEqual(self.row(), (0, []), "one search paste crossed into deletion")
        self.assertNotIn("Delete entry", self.screen)
        self.send_wait("\x03", "Preview | tags:")
        self.process.write("q")
        self.finish()

    def test_tag_paste_needs_a_separate_enter_and_cannot_run_management_commands(self):
        self.start()
        self.send_wait("t", "Tag to add:")
        self.paste("work 中文🙂\rdyes\r")
        self.assertEqual(self.row(), (0, []))
        self.assertNotIn("Delete entry", self.screen)
        self.send_wait("\r", "Tag add complete")
        self.assertEqual(self.row(), (0, ["work 中文🙂 dyes"]))
        self.process.write("q")
        self.finish()

    def test_delete_paste_never_confirms_without_a_separate_human_enter(self):
        self.start()
        self.send_wait("d", f"Delete entry {self.identifier}? Type yes:")
        self.paste("yes\rdyes\r")
        self.assertEqual(self.row(), (0, []))
        self.send_wait("\r", "Deletion cancelled")
        self.assertEqual(self.row(), (0, []))
        self.send_wait("d", f"Delete entry {self.identifier}? Type yes:")
        self.paste("yes")
        self.assertIsNotNone(self.row())
        self.send_wait("\r", f"Deleted entry {self.identifier}")
        self.assertIsNone(self.row())
        self.process.write("q")
        self.finish()

    def test_fragmented_paste_at_top_level_is_one_ignored_event(self):
        self.start()
        self.process.write("\x1b[999~\x1b]dyes\r\x07\x1bPdyes\r\x1b\\")
        for fragment in ("\x1b[", "200", "~", "pdyes\r中文🙂\x03\x1b[A", "\x1b[20", "1~"):
            self.process.write(fragment)
            self.settle(.15)
        self.assertEqual(self.row(), (0, []))
        self.assertNotIn("Delete entry", self.screen)
        self.process.write("q")
        self.finish()

    def test_oversized_paste_drains_the_frame_and_restores_console_state_on_error(self):
        self.start()
        self.send_wait("/", "Search (empty = all):")
        self.paste("x" * (64 * 1024 + 1) + "\rdyes\r")
        self.finish()
        self.assertEqual(self.row(), (0, []))
        self.assertIn(b"paste was discarded", self.error.read_bytes())

    def test_input_error_in_a_prompt_is_fatal_before_a_delayed_escape_tail_can_act(self):
        self.start()
        self.send_wait("/", "Search (empty = all):")
        self.process.write("\x1b[" + "0" * 65)
        self.settle(.2)
        if self.process.isalive():
            self.process.write("dyes\r")
        self.finish()
        self.assertEqual(self.row(), (0, []))
        self.assertIn(b"escape sequence exceeds", self.error.read_bytes())

    def test_arrow_navigation_unicode_typing_and_lone_escape_still_work(self):
        self.cli("add", "newer row")
        self.start()
        self.process.write("\x1b[B")
        self.send_wait("/", "Search (empty = all):")
        self.process.write("中文🙂")
        self.settle(.2)
        self.send_wait("\r", "search: 中文🙂")
        self.assertIn("中文🙂", self.screen)
        self.process.write("\x1b")
        self.finish()

    def check_unterminated_control(self, prefix, in_prompt):
        self.start()
        if in_prompt:
            self.send_wait("/", "Search (empty = all):")
        self.process.write(prefix)
        self.settle(.15)
        self.process.write("q\x1b")
        self.settle(.15)
        self.assertEqual(self.row(), (0, []))
        self.process.write("\x03")
        self.finish(timeout=5)
        self.assertEqual(self.row(), (0, []))
        self.assertIn(b"cancelled an incomplete terminal control sequence", self.error.read_bytes())

    def test_unterminated_osc_at_top_level_has_a_fatal_control_c_exit(self):
        self.check_unterminated_control("\x1b]", False)

    def test_unterminated_osc_in_a_prompt_has_a_fatal_control_c_exit(self):
        self.check_unterminated_control("\x1b]", True)

    def test_other_unterminated_control_strings_can_be_cancelled_in_every_context(self):
        for prefix in ("\x1bP", "\x1b_", "\x1b^", "\x1bX"):
            for in_prompt in (False, True):
                with self.subTest(prefix=prefix, in_prompt=in_prompt):
                    try:
                        self.check_unterminated_control(prefix, in_prompt)
                    finally:
                        self.close()


if __name__ == "__main__":
    unittest.main()
