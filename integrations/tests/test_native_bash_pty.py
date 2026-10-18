"""Real Bash/Readline cursor checks use typed text and arrow keys in a PTY."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


WIDGET = Path(__file__).resolve().parents[2] / "src-tauri" / "src" / "cli" / "shell" / "bash.sh"


@unittest.skipUnless(sys.platform.startswith("linux") and shutil.which("bash"),
                     "Native Bash Readline interaction requires a Linux PTY")
class NativeBashPtyTests(unittest.TestCase):
    def test_ascii_selection_does_not_split_the_existing_unicode_prefix(self):
        line, cursor = self.interact("前tail", 4, "X")
        self.assertEqual(line, "前Xtail".encode("utf-8"))
        self.assertEqual(cursor, 2)

    def test_unicode_selection_leaves_the_cursor_before_the_original_suffix(self):
        line, cursor = self.interact("pre:tail", 4, "中文🙂", typed_after="X")
        self.assertEqual(line, "pre:中文🙂Xtail".encode("utf-8"))
        self.assertEqual(cursor, 8)

    def test_multiline_crlf_trailing_newlines_and_literal_shell_syntax_are_preserved(self):
        selection = "中文🙂\r\n$(not-a-command)\r\n\n"
        line, cursor = self.interact("前pre:tail", 4, selection, typed_after="X")
        self.assertEqual(line, ("前pre:" + selection + "Xtail").encode("utf-8"))
        self.assertEqual(cursor, len("前pre:" + selection + "X"))

    def test_cancellation_and_errors_keep_the_buffer_and_cursor_unchanged(self):
        for exit_code in (1, 130, 2):
            with self.subTest(exit_code=exit_code):
                line, cursor = self.interact("前tail", 4, "partial中文\n", exit_code=exit_code,
                                             typed_after="X")
                self.assertEqual(line, "前Xtail".encode("utf-8"))
                self.assertEqual(cursor, 2)

    def test_byte_oriented_caller_locale_remains_byte_oriented(self):
        line, cursor = self.interact("pre:tail", 4, "中文🙂", typed_after="X", locale="C")
        self.assertEqual(line, "pre:中文🙂Xtail".encode("utf-8"))
        self.assertEqual(cursor, len("pre:中文🙂X".encode("utf-8")))

    def interact(self, initial, move_left, selection, exit_code=0, typed_after="", locale="C.UTF-8"):
        import fcntl
        import pty
        import select
        import struct
        import termios
        import time

        with tempfile.TemporaryDirectory(prefix="dzc-bash-pty-") as directory:
            directory = Path(directory)
            widget = directory / "widget.bash"
            if os.environ.get("DZC_TEST_BIN"):
                generated = subprocess.run([os.environ["DZC_TEST_BIN"], "bash"],
                                           capture_output=True, check=True, timeout=10)
                widget.write_bytes(generated.stdout)
            else:
                widget.write_text(WIDGET.read_text(encoding="utf-8"), encoding="utf-8")
            selector = directory / "fake-dzc"
            selector.write_text("#!/bin/sh\nprintf '%s' \"$DZC_TEST_SELECTION\"\n"
                                "exit \"$DZC_TEST_EXIT\"\n", encoding="utf-8")
            selector.chmod(0o755)
            setup = directory / "setup.bash"
            setup.write_text(
                'source "$DZC_TEST_WIDGET"\n'
                "PS1='DZC_BASH_READY> '\n"
                "_dzc_test_probe() {\n"
                ' printf "%s" "$READLINE_LINE" > "$DZC_TEST_LINE"\n'
                ' printf "%s" "$READLINE_POINT" > "$DZC_TEST_CURSOR"\n'
                ' printf "%s" "$LC_ALL" > "$DZC_TEST_LOCALE"\n'
                " READLINE_LINE=''\n"
                "}\n"
                "bind -x '\"\\C-x\":_dzc_test_probe'\n", encoding="utf-8")
            environment = {**os.environ, "HOME": str(directory), "HISTFILE": "/dev/null",
                           "INPUTRC": "/dev/null", "TERM": "xterm-256color",
                           "LC_ALL": locale, "LANG": locale, "DZC_BIN": str(selector),
                           "DZC_PICKER": "tui", "DZC_TEST_WIDGET": str(widget),
                           "DZC_TEST_SELECTION": selection, "DZC_TEST_EXIT": str(exit_code),
                           "DZC_TEST_LINE": str(directory / "line.bin"),
                           "DZC_TEST_CURSOR": str(directory / "cursor.txt"),
                           "DZC_TEST_LOCALE": str(directory / "locale.txt")}
            environment.pop("BASH_ENV", None)
            environment.pop("ENV", None)
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 110, 0, 0))

            def own_terminal():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)

            process = subprocess.Popen([shutil.which("bash"), "--noprofile", "--rcfile", str(setup), "-i"],
                                       stdin=slave, stdout=slave, stderr=slave, env=environment,
                                       preexec_fn=own_terminal)
            os.close(slave)
            screen = b""

            def drain():
                nonlocal screen
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        screen += os.read(master, 65536)
                    except OSError:
                        pass

            try:
                deadline = time.monotonic() + 5
                while b"DZC_BASH_READY>" not in screen and time.monotonic() < deadline:
                    drain()
                self.assertIn(b"DZC_BASH_READY>", screen)
                # Readline derives its own cursor from actual typing and cursor keys.
                keys = initial.encode("utf-8") + b"\x1b[D" * move_left + b"\x07"
                keys += typed_after.encode("utf-8") + b"\x18"
                os.write(master, keys)
                captured = directory / "locale.txt"
                deadline = time.monotonic() + 5
                while not captured.exists() and time.monotonic() < deadline:
                    drain()
                self.assertTrue(captured.exists(), screen)
                self.assertEqual(captured.read_text(), locale)
                self.assertTrue(process.poll() is None, screen)
                return (directory / "line.bin").read_bytes(), int((directory / "cursor.txt").read_text())
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
                os.close(master)


if __name__ == "__main__":
    unittest.main()
