"""Opt-in native selectors and generated Shell widgets; no Python runtime adapter."""

from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from sqlite_history_fixture import connect_fixture_database

BIN = os.environ.get("DZC_TEST_BIN")
NU = os.environ.get("DZC_TEST_NU") or shutil.which("nu")
PAYLOAD = "中文🙂 DZC_NATIVE_LITERAL\r\n$(not-a-command)\r\n\n"
FIXTURE = r'''
use std::{env, fs, io::{self, Read, Write}, process};
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if let Ok(path) = env::var("DZC_FAKE_ARGUMENTS") { fs::write(path, args.join("\0")).unwrap(); }
    if env::var("DZC_FAKE_MODE").as_deref() == Ok("widget") {
        io::stdout().write_all(env::var("DZC_FAKE_PAYLOAD").unwrap_or_default().as_bytes()).unwrap();
        process::exit(env::var("DZC_FAKE_EXIT").unwrap_or_else(|_| "0".into()).parse().unwrap());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    if let Ok(path) = env::var("DZC_FAKE_INPUT") { fs::write(path, input.as_bytes()).unwrap(); }
    if let Ok(path) = env::var("DZC_FAKE_ENV") {
        let inherited: Vec<_> = env::vars().filter(|(key, _)| key.starts_with("FZF_DEFAULT_")).collect();
        fs::write(path, format!("{inherited:?}")).unwrap();
    }
    match env::var("DZC_FAKE_MODE").unwrap_or_default().as_str() {
        "cancel" => { print!("partial"); process::exit(130); },
        "error" => { print!("partial"); process::exit(2); },
        "forged" => print!("999999\t  [text] forged\n"),
        "invalid" => io::stdout().write_all(&[255]).unwrap(),
        "huge" => io::stdout().write_all(&vec![b'x'; 100_000]).unwrap(),
        _ => println!("{}", input.lines().next().unwrap_or_default()),
    }
}
'''


@unittest.skipUnless(BIN, "Set DZC_TEST_BIN to test native dzc frontends")
class NativePickShellTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("rustc"):
            raise unittest.SkipTest("Native subprocess fixture requires rustc")
        cls.fixture_directory = tempfile.TemporaryDirectory(prefix="dzc-native-fixture-")
        fixture = Path(cls.fixture_directory.name)
        source = fixture / "fake.rs"
        source.write_text(FIXTURE, encoding="utf-8")
        cls.fake = fixture / ("fake.exe" if sys.platform == "win32" else "fake")
        subprocess.run(["rustc", "--edition=2021", str(source), "-o", str(cls.fake)],
                       check=True, capture_output=True, timeout=120)
        for backend in ("fzf", "dmenu", "rofi"):
            shutil.copy2(cls.fake, fixture / (backend + (".exe" if sys.platform == "win32" else "")))

    @classmethod
    def tearDownClass(cls):
        cls.fixture_directory.cleanup()

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="dzc-native-widget-")
        self.root = Path(self.directory.name)
        self.environment = {**os.environ,
                            "DEZIRCLIP_DB_PATH": str(self.root / "history.db"),
                            "PATH": self.fixture_directory.name + os.pathsep + os.environ["PATH"],
                            "DZC_FAKE_ARGUMENTS": str(self.root / "arguments.bin"),
                            "DZC_FAKE_INPUT": str(self.root / "input.txt"),
                            "DZC_FAKE_ENV": str(self.root / "environment.txt")}
        for key in ("DZC_BIN", "DZC_PICKER", "DZC_FAKE_MODE", "DZC_FAKE_EXIT"):
            self.environment.pop(key, None)

    def tearDown(self):
        self.directory.cleanup()

    def dzc(self, *arguments, payload=None, check=True, environment=None):
        return subprocess.run([BIN, *arguments], input=payload, capture_output=True,
                              env=environment or self.environment, timeout=20, check=check)

    def assert_arguments(self, expected):
        self.assertEqual((self.root / "arguments.bin").read_bytes().decode("utf-8").split("\0"), expected)

    def test_external_picker_receives_safe_rows_and_restores_exact_text_for_each_backend(self):
        self.dzc("add", "-", payload=PAYLOAD.encode("utf-8"))
        expected_arguments = {
            "fzf": ["--no-multi", "--delimiter=\t", "--with-nth=2..", "--prompt=clipboard> "],
            "dmenu": ["-i", "-p", "clipboard"],
            "rofi": ["-dmenu", "-i", "-p", "clipboard"],
        }
        environment = {**self.environment, "FZF_DEFAULT_OPTS": "--bind enter:execute(untrusted)",
                       "FZF_DEFAULT_OPTS_FILE": "untrusted", "FZF_DEFAULT_COMMAND": "untrusted"}
        for backend, arguments in expected_arguments.items():
            with self.subTest(backend=backend):
                result = self.dzc("pick", "--backend", backend, "--action", "insert", environment=environment)
                self.assertEqual(result.stdout, PAYLOAD.encode("utf-8"))
                self.assert_arguments(arguments)
                rows = (self.root / "input.txt").read_text(encoding="utf-8").splitlines()
                self.assertEqual(len(rows), 1)
                self.assertEqual(rows[0].count("\t"), 1)
                self.assertEqual((self.root / "environment.txt").read_text(), "[]")

    def test_external_picker_cancellation_errors_and_forged_output_never_return_payload(self):
        self.dzc("add", "private payload")
        for mode, code in (("cancel", 1), ("error", 1), ("forged", 1), ("invalid", 1), ("huge", 1)):
            with self.subTest(mode=mode):
                result = self.dzc("pick", "--backend", "fzf", "--action", "print", check=False,
                                  environment={**self.environment, "DZC_FAKE_MODE": mode})
                self.assertEqual(result.returncode, code)
                self.assertEqual(result.stdout, b"")
                self.assertNotIn(b"private payload", result.stderr)

    def test_picker_type_filter_is_applied_before_pagination(self):
        ids = []
        for content, kind in (("needle older", "code"), ("needle middle", "code"), ("needle newer", "text")):
            result = self.dzc("add", content, "--type", kind)
            ids.append(int(result.stdout.decode().strip().rsplit("=", 1)[1]))
        with closing(connect_fixture_database(self.root / "history.db")) as database:
            database.execute("UPDATE clipboard_history SET timestamp=id")
            database.commit()
        result = self.dzc("pick", "--backend", "fzf", "--query", "needle", "--kind", "code",
                          "--limit", "1", "--offset", "1", "--action", "id")
        self.assertEqual(result.stdout, f"{ids[0]}\n".encode())

    def test_all_generated_widgets_work_without_opening_history_or_python(self):
        for shell in ("bash", "zsh", "fish", "nu", "pwsh"):
            with self.subTest(shell=shell):
                script = self.dzc(shell).stdout.decode("utf-8")
                self.assertNotIn("python", script.lower())
                self.assertNotIn("__DZC_EXECUTABLE__", script)
                self.assertIn("DZC_BIN", script)
                self.assertIn("tui", script)
        self.assertFalse((self.root / "history.db").exists())

    def test_nushell_install_is_idempotent_and_refuses_conflicting_files(self):
        target = self.root / "autoload" / "dzc.nu"
        self.dzc("nu", "--install", str(target))
        self.assertEqual(target.read_bytes(), self.dzc("nu").stdout)
        self.dzc("nu", "--install", str(target))
        target.write_bytes(b"user configuration\n")
        self.assertNotEqual(self.dzc("nu", "--install", str(target), check=False).returncode, 0)
        self.assertEqual(target.read_bytes(), b"user configuration\n")
        if sys.platform != "win32":
            link = self.root / "link.nu"
            link.symlink_to(target)
            self.assertNotEqual(self.dzc("nu", "--install", str(link), check=False).returncode, 0)
            self.assertEqual(target.read_bytes(), b"user configuration\n")

    def test_generated_defaults_quote_spaces_unicode_and_shell_metacharacters(self):
        directory = self.root / "中文 dir's $()"
        directory.mkdir()
        executable = directory / ("dzc.exe" if sys.platform == "win32" else "dzc")
        shutil.copy2(BIN, executable)
        for shell in ("bash", "zsh", "fish", "nu", "pwsh"):
            command = NU if shell == "nu" else shutil.which(shell)
            if not command:
                continue
            if sys.platform == "win32" and shell in ("bash", "zsh", "fish"):
                continue
            with self.subTest(shell=shell):
                widget = self.root / ("quoted." + ("ps1" if shell == "pwsh" else shell))
                widget.write_bytes(subprocess.run([str(executable), shell], env=self.environment,
                                                  capture_output=True, check=True).stdout)
                if shell in ("bash", "zsh"):
                    arguments = [command, "-c", 'source "$1"; printf "%s" "$DZC_BIN"',
                                 "dzc-path-test", str(widget)]
                elif shell == "fish":
                    arguments = [command, "-c", 'source "$argv[1]"; printf "%s" "$DZC_BIN"', str(widget)]
                elif shell == "nu":
                    arguments = [command, "-n", "-c", f"source {json.dumps(str(widget))}; $env.DZC_BIN"]
                else:
                    setup = self.root / "quoted-test.ps1"
                    setup.write_text(". '" + str(widget).replace("'", "''") +
                                     "'\n[Console]::Out.Write($env:DZC_BIN)\n", encoding="utf-8")
                    arguments = [command, "-NoProfile", "-File", str(setup)]
                result = subprocess.run(arguments, env=self.environment, capture_output=True,
                                        timeout=10, check=True)
                actual = result.stdout.decode("utf-8").rstrip("\r\n")
                self.assertEqual(Path(actual), executable)

    @unittest.skipIf(sys.platform == "win32", "POSIX Shell widgets run on Linux/WSL")
    def test_bash_zsh_and_fish_preserve_exact_literal_text_and_cancel_partial_output(self):
        commands = {
            "bash": 'source "$1"; prefix="前pre:"; READLINE_LINE="${prefix}tail"; READLINE_POINT=${#prefix}; '
                    '_dzc_insert; printf "%s" "$READLINE_LINE"',
            "zsh": 'source "$1"; zle() { :; }; LBUFFER="前pre:"; RBUFFER="tail"; '
                   '_dzc_insert; printf "%s" "$LBUFFER$RBUFFER"',
            "fish": 'source "$argv[1]"; function commandline; if test "$argv[1]" = --insert; '
                    'printf "前pre:%stail" "$argv[-1]"; end; end; __dzc_insert',
        }
        for shell, program in commands.items():
            if not shutil.which(shell):
                continue
            script = self.root / ("widget." + shell)
            script.write_bytes(self.dzc(shell).stdout)
            for picker in ("tui", "fzf"):
                for exit_code in (0, 1, 130, 2):
                    with self.subTest(shell=shell, picker=picker, exit_code=exit_code):
                        environment = {**self.environment, "DZC_BIN": str(self.fake), "DZC_PICKER": picker,
                                       "DZC_FAKE_MODE": "widget", "DZC_FAKE_PAYLOAD": PAYLOAD,
                                       "DZC_FAKE_EXIT": str(exit_code)}
                        arguments = [shell, "-c", program]
                        if shell != "fish":
                            arguments.append("dzc-widget-test")
                        result = subprocess.run([*arguments, str(script)], env=environment,
                                                capture_output=True, timeout=10, check=True)
                        expected = "前pre:" + (PAYLOAD if exit_code == 0 else "") + "tail"
                        if shell == "fish" and exit_code != 0:
                            expected = ""
                        self.assertEqual(result.stdout, expected.encode("utf-8"))
                        self.assert_arguments(["tui", "--action", "insert"] if picker == "tui" else
                                              ["pick", "--backend", "fzf", "--action", "insert"])

    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty") and shutil.which("pwsh"),
                         "PowerShell editor interaction requires Windows, pwsh and optional pywinpty")
    def test_powershell_real_editor_inserts_literal_text_and_ignores_partial_error_output(self):
        from winpty import PtyProcess
        script = self.root / "widget.ps1"
        script.write_bytes(self.dzc("pwsh").stdout)
        for exit_code in (0, 2):
            with self.subTest(exit_code=exit_code):
                captured = self.root / f"powershell-buffer-{exit_code}.txt"
                setup = self.root / "setup.ps1"
                quote = lambda value: "'" + str(value).replace("'", "''") + "'"
                setup.write_text(
                    "Import-Module PSReadLine\n. " + quote(script) + "\n"
                    "Set-PSReadLineOption -BellStyle None\n"
                    "Set-PSReadLineKeyHandler -Chord Ctrl+x -ScriptBlock {\n"
                    " $taskLine = ''; $taskCursor = 0\n"
                    " [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$taskLine, [ref]$taskCursor)\n"
                    " [IO.File]::WriteAllText(" + quote(captured) + ", $taskLine, [Text.UTF8Encoding]::new($false))\n"
                    " [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()\n}\n"
                    "function prompt { 'DZC_NATIVE_READY> ' }\n", encoding="utf-8")
                process = PtyProcess.spawn([shutil.which("pwsh"), "-NoLogo", "-NoProfile", "-NoExit", "-File", str(setup)],
                                           env={**self.environment, "DZC_BIN": str(self.fake), "DZC_FAKE_MODE": "widget",
                                                "DZC_FAKE_PAYLOAD": PAYLOAD, "DZC_FAKE_EXIT": str(exit_code)},
                                           dimensions=(30, 110), backend="0")
                process.fileobj.settimeout(0.2)
                screen = ""
                try:
                    screen = self.read_until(process, screen, lambda current: "DZC_NATIVE_READY>" in current)
                    self.assertIn("DZC_NATIVE_READY>", screen)
                    process.write("prefix:\x07")
                    screen = self.read_until(process, screen, lambda _: (self.root / "arguments.bin").exists())
                    process.write("\x18")
                    screen = self.read_until(process, screen, lambda _: captured.exists())
                    self.assertTrue(captured.exists(), screen)
                    expected = "prefix:" + (PAYLOAD if exit_code == 0 else "")
                    self.assertEqual(captured.read_bytes(), expected.encode("utf-8"))
                    self.assert_arguments(["tui", "--action", "insert"])
                finally:
                    if process.isalive():
                        process.write("\x03")
                    process.close(force=True)
                (self.root / "arguments.bin").unlink(missing_ok=True)

    @unittest.skipUnless(sys.platform == "win32" and NU and importlib.util.find_spec("winpty"),
                         "Nushell editor interaction requires Windows, Nushell and optional pywinpty")
    def test_nushell_real_editor_modes_cancellation_and_missing_executable(self):
        from winpty import PtyProcess
        script = self.root / "widget.nu"
        script.write_bytes(self.dzc("nu").stdout)
        cases = [("emacs", 0, False), ("vi_insert", 0, False), ("vi_normal", 0, False),
                 ("emacs", 1, False), ("emacs", 130, False), ("emacs", 2, False), ("emacs", 0, True)]
        for index, (mode, exit_code, missing) in enumerate(cases):
            with self.subTest(mode=mode, exit_code=exit_code, missing=missing):
                captured = self.root / f"nu-buffer-{index}.txt"
                setup = self.root / "setup.nu"
                quote = lambda value: json.dumps(str(value), ensure_ascii=False)
                setup.write_text(
                    f"source {quote(script)}\n$env.config.show_banner = false\n$env.config.show_hints = false\n"
                    f"$env.config.edit_mode = '{'emacs' if mode == 'emacs' else 'vi'}'\n"
                    "$env.PROMPT_COMMAND = {|| 'DZC_NATIVE_READY> '}\n$env.PROMPT_COMMAND_RIGHT = {|| ''}\n"
                    "$env.PROMPT_INDICATOR = ''\n$env.PROMPT_INDICATOR_VI_INSERT = 'DZC_INSERT> '\n"
                    "$env.PROMPT_INDICATOR_VI_NORMAL = 'DZC_NORMAL> '\n"
                    "def dzc-test-probe [] {\n"
                    f" commandline | save --force --raw {quote(captured)}\n commandline edit --replace ''\n}}\n"
                    "$env.config.keybindings ++= [{name: dzc_test_probe, modifier: control, keycode: char_x, "
                    "mode: [emacs vi_insert vi_normal], event: {send: executehostcommand, cmd: 'dzc-test-probe'}}]\n",
                    encoding="utf-8")
                process = PtyProcess.spawn(
                    [NU, "-n", "--no-history", "--config-home", str(self.root), "--execute", f"source {quote(setup)}"],
                    env={**self.environment, "DZC_BIN": str(self.root / "missing.exe") if missing else str(self.fake),
                         "DZC_FAKE_MODE": "widget", "DZC_FAKE_PAYLOAD": PAYLOAD, "DZC_FAKE_EXIT": str(exit_code)},
                    dimensions=(30, 110), backend="0")
                process.fileobj.settimeout(0.2)
                screen = ""
                try:
                    screen = self.read_until(process, screen, lambda current: "DZC_NATIVE_READY>" in current)
                    self.assertIn("DZC_NATIVE_READY>", screen)
                    process.write("prefix:")
                    screen = self.read_until(process, screen, lambda current: "prefix:" in current)
                    self.assertIn("prefix:", screen)
                    if mode == "vi_normal":
                        process.write("\x1b")
                        screen = self.read_until(process, screen, lambda current: "DZC_NORMAL>" in current)
                        self.assertIn("DZC_NORMAL>", screen)
                    completed = screen.count("\x1b]133;D")
                    process.write("\x07")
                    screen = self.read_until(process, screen, lambda current: current.count("\x1b]133;D") > completed)
                    self.assertGreater(screen.count("\x1b]133;D"), completed, screen)
                    process.write("\x18")
                    screen = self.read_until(process, screen, lambda _: captured.exists())
                    self.assertTrue(captured.exists(), screen)
                    expected = "prefix:"
                    if exit_code == 0 and not missing:
                        expected = "prefix" + PAYLOAD + ":" if mode == "vi_normal" else "prefix:" + PAYLOAD
                    self.assertEqual(captured.read_bytes(), expected.encode("utf-8"))
                    if not missing:
                        self.assert_arguments(["tui", "--action", "insert"])
                finally:
                    if process.isalive():
                        process.write("\x03")
                    process.close(force=True)

    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty") and shutil.which("pwsh"),
                         "Nested builtin TUI requires Windows, pwsh and optional pywinpty")
    def test_shell_widgets_open_builtin_tui_and_insert_without_submitting(self):
        from winpty import PtyProcess
        self.dzc("add", "-", payload=PAYLOAD.encode("utf-8"))
        for shell in ("pwsh", "nu"):
            if shell == "nu" and not NU:
                continue
            widget = self.root / ("nested.ps1" if shell == "pwsh" else "nested.nu")
            widget.write_bytes(self.dzc(shell).stdout)
            for cancel in (False, True):
                with self.subTest(shell=shell, cancel=cancel):
                    captured = self.root / f"nested-{shell}-{cancel}.txt"
                    if shell == "pwsh":
                        quote = lambda value: "'" + str(value).replace("'", "''") + "'"
                        setup = self.root / "nested-setup.ps1"
                        setup.write_text(
                            "Import-Module PSReadLine\n. " + quote(widget) + "\n"
                            "Set-PSReadLineOption -BellStyle None\n"
                            "Set-PSReadLineKeyHandler -Chord Ctrl+x -ScriptBlock {\n"
                            " $taskLine = ''; $taskCursor = 0\n"
                            " [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$taskLine, [ref]$taskCursor)\n"
                            " [IO.File]::WriteAllText(" + quote(captured) + ", $taskLine, [Text.UTF8Encoding]::new($false))\n"
                            " [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()\n}\n"
                            "function prompt { 'DZC_NESTED_READY> ' }\n", encoding="utf-8")
                        arguments = [shutil.which("pwsh"), "-NoLogo", "-NoProfile", "-NoExit", "-File", str(setup)]
                    else:
                        quote = lambda value: json.dumps(str(value), ensure_ascii=False)
                        setup = self.root / "nested-setup.nu"
                        setup.write_text(
                            f"source {quote(widget)}\n$env.config.show_banner = false\n$env.config.show_hints = false\n"
                            "$env.PROMPT_COMMAND = {|| 'DZC_NESTED_READY> '}\n$env.PROMPT_COMMAND_RIGHT = {|| ''}\n"
                            "$env.PROMPT_INDICATOR = ''\n"
                            "def dzc-test-probe [] {\n"
                            f" commandline | save --force --raw {quote(captured)}\n commandline edit --replace ''\n}}\n"
                            "$env.config.keybindings ++= [{name: dzc_test_probe, modifier: control, keycode: char_x, "
                            "mode: [emacs vi_insert vi_normal], event: {send: executehostcommand, cmd: 'dzc-test-probe'}}]\n",
                            encoding="utf-8")
                        arguments = [NU, "-n", "--no-history", "--config-home", str(self.root), "--execute", f"source {quote(setup)}"]
                    process = PtyProcess.spawn(arguments, env=self.environment,
                                               dimensions=(30, 110), backend="0")
                    process.fileobj.settimeout(0.2)
                    screen = ""
                    try:
                        screen = self.read_until(process, screen, lambda current: "DZC_NESTED_READY>" in current)
                        self.assertIn("DZC_NESTED_READY>", screen)
                        process.write("prefix:")
                        if shell == "nu":
                            screen = self.read_until(process, screen, lambda current: "prefix:" in current)
                            self.assertIn("prefix:", screen)
                        completed = screen.count("\x1b]133;D")
                        process.write("\x07")
                        screen = self.read_until(process, screen, lambda current: "Enter insert" in current)
                        self.assertIn("Enter insert", screen)
                        process.write("q" if cancel else "\r")
                        screen = self.read_until(process, screen, lambda current: "\x1b[?1049l" in current)
                        self.assertIn("\x1b[?1049l", screen)
                        if shell == "nu":
                            screen = self.read_until(process, screen, lambda current: current.count("\x1b]133;D") > completed)
                            self.assertGreater(screen.count("\x1b]133;D"), completed, screen)
                        process.write("\x18")
                        screen = self.read_until(process, screen, lambda _: captured.exists())
                        self.assertTrue(captured.exists(), screen)
                        expected = "prefix:" + ("" if cancel else PAYLOAD)
                        self.assertEqual(captured.read_bytes(), expected.encode("utf-8"))
                        self.assertTrue(process.isalive())
                    finally:
                        if process.isalive():
                            process.write("\x03")
                        process.close(force=True)

    def read_until(self, process, screen, predicate, timeout=10):
        deadline = time.monotonic() + timeout
        while not predicate(screen) and time.monotonic() < deadline:
            try:
                chunk = process.read(65536)
            except socket.timeout:
                continue
            except EOFError:
                break
            screen += chunk
            if "\x1b[6n" in chunk:
                process.write("\x1b[1;1R")
        return screen


if __name__ == "__main__":
    unittest.main()
