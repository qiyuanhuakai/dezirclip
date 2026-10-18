"""Optional Nushell tests use disposable config and actual line-editor events."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

NUSHELL = os.environ.get("DZC_TEST_NU") or shutil.which("nu")
WIDGET = Path(__file__).resolve().parents[1] / "shell" / "dzc.nu"


def nu_string(value):
    return json.dumps(str(value), ensure_ascii=False)


@unittest.skipUnless(NUSHELL, "Set DZC_TEST_NU or install Nushell to test its integration")
class NushellTests(unittest.TestCase):
    def test_source_defaults_modes_and_reloading_without_duplicate_bindings(self):
        environment = {key: value for key, value in os.environ.items()
                       if key not in ("DZC_ADAPTER", "DZC_PYTHON", "DZC_PICKER")}
        program = (
            f"source {nu_string(WIDGET)}; source {nu_string(WIDGET)}; "
            "{adapter: $env.DZC_ADAPTER, python: $env.DZC_PYTHON, picker: $env.DZC_PICKER, "
            "bindings: ($env.config.keybindings | where name == dzc_insert)} | to json -r"
        )
        result = subprocess.run([NUSHELL, "-n", "--no-history", "-c", program],
                                env=environment, capture_output=True, timeout=10, check=True)
        config = json.loads(result.stdout)
        self.assertEqual(Path(config["adapter"]), WIDGET.parents[1] / "dzc-integrations.py")
        self.assertEqual(config["python"], "python" if sys.platform == "win32" else "python3")
        self.assertEqual(config["picker"], "fzf")
        self.assertEqual(len(config["bindings"]), 1)
        binding = config["bindings"][0]
        self.assertEqual(binding["keycode"], "char_g")
        self.assertEqual(set(binding["mode"]), {"emacs", "vi_insert", "vi_normal"})
        self.assertEqual(binding["event"], {"send": "executehostcommand", "cmd": "dzc-insert"})

    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty"),
                         "Windows Nushell interaction needs optional pywinpty")
    def test_real_line_editor_inserts_literal_unicode_and_newlines_in_emacs_and_vi_modes(self):
        payload = "中文🙂 DZC_NU_LITERAL\r\n$(not-a-command)\r\n\n"
        for mode in ("emacs", "vi_insert", "vi_normal"):
            with self.subTest(mode=mode):
                self.run_interaction(mode, payload, 0)

    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty"),
                         "Windows Nushell interaction needs optional pywinpty")
    def test_cancellation_nonzero_exit_and_missing_executable_keep_the_existing_buffer(self):
        for exit_code in (1, 130, 2):
            with self.subTest(exit_code=exit_code):
                self.run_interaction("emacs", "partial output must be ignored\n", exit_code)
        self.run_interaction("emacs", "", 0, missing_python=True)

    def run_interaction(self, mode, payload, exit_code, missing_python=False):
        import socket
        import time
        from winpty import PtyProcess

        with tempfile.TemporaryDirectory(prefix="dzc-nushell-widget-") as directory:
            directory = Path(directory)
            adapter = directory / "fake adapter.py"
            captured = directory / "buffer.txt"
            arguments = directory / "arguments.json"
            setup = directory / "setup.nu"
            adapter.write_text(
                "import json, pathlib, sys\n"
                "pathlib.Path(" + repr(str(arguments)) + ").write_text(json.dumps(sys.argv[1:]))\n"
                "sys.stdout.buffer.write(" + repr(payload.encode("utf-8")) + ")\n"
                "sys.exit(" + repr(exit_code) + ")\n", encoding="utf-8")
            setup.write_text(
                f"source {nu_string(WIDGET)}\n"
                "$env.config.show_banner = false\n"
                "$env.config.show_hints = false\n"
                f"$env.config.edit_mode = '{'emacs' if mode == 'emacs' else 'vi'}'\n"
                "$env.PROMPT_COMMAND = {|| 'DZC_NU_READY> '}\n"
                "$env.PROMPT_COMMAND_RIGHT = {|| ''}\n"
                "$env.PROMPT_INDICATOR = ''\n"
                "$env.PROMPT_INDICATOR_VI_INSERT = 'DZC_INSERT> '\n"
                "$env.PROMPT_INDICATOR_VI_NORMAL = 'DZC_NORMAL> '\n"
                "def dzc-test-probe [] {\n"
                f" commandline | save --force --raw {nu_string(captured)}\n"
                " commandline edit --replace ''\n"
                "}\n"
                "$env.config.keybindings ++= [{ name: dzc_test_probe, modifier: control, keycode: char_x, "
                "mode: [emacs vi_insert vi_normal], event: {send: executehostcommand, cmd: 'dzc-test-probe'} }]\n",
                encoding="utf-8")
            environment = {**os.environ, "DZC_ADAPTER": str(adapter),
                           "DZC_PYTHON": str(directory / "missing-python.exe") if missing_python else sys.executable,
                           "DZC_PICKER": "dmenu"}
            process = PtyProcess.spawn(
                [NUSHELL, "-n", "--no-history", "--config-home", str(directory),
                 "--execute", f"source {nu_string(setup)}"],
                env=environment, dimensions=(30, 110), backend="0")
            process.fileobj.settimeout(0.2)
            screen = ""

            def read_screen():
                nonlocal screen
                try:
                    chunk = process.read(65536)
                except socket.timeout:
                    return
                screen += chunk
                if "\x1b[6n" in chunk:
                    process.write("\x1b[1;1R")

            try:
                deadline = time.monotonic() + 10
                while "DZC_NU_READY>" not in screen and time.monotonic() < deadline:
                    read_screen()
                self.assertIn("DZC_NU_READY>", screen)
                process.write("prefix:")
                deadline = time.monotonic() + 5
                while "prefix:" not in screen and time.monotonic() < deadline:
                    read_screen()
                self.assertIn("prefix:", screen)
                if mode == "vi_normal":
                    process.write("\x1b")
                    deadline = time.monotonic() + 5
                    while "DZC_NORMAL>" not in screen and time.monotonic() < deadline:
                        read_screen()
                    self.assertIn("DZC_NORMAL>", screen)
                completed_commands = screen.count("\x1b]133;D")
                process.write("\x07")
                deadline = time.monotonic() + 8
                while screen.count("\x1b]133;D") == completed_commands and time.monotonic() < deadline:
                    read_screen()
                self.assertGreater(screen.count("\x1b]133;D"), completed_commands, screen)
                process.write("\x18")
                deadline = time.monotonic() + 8
                while not captured.exists() and time.monotonic() < deadline:
                    read_screen()
                self.assertTrue(captured.exists(), screen)
                expected = "prefix:"
                if exit_code == 0 and not missing_python:
                    expected = "prefix" + payload + ":" if mode == "vi_normal" else "prefix:" + payload
                self.assertEqual(captured.read_bytes(), expected.encode("utf-8"))
                if not missing_python:
                    self.assertEqual(json.loads(arguments.read_text()),
                                     ["pick", "--backend", "dmenu", "--action", "insert"])
            finally:
                if process.isalive():
                    process.write("\x03")
                process.close(force=True)


if __name__ == "__main__":
    unittest.main()
