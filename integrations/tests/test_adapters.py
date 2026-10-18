import io
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from dzc_integrations.client import AdapterError, DzcClient, display_text, row_text, sensitive
from dzc_integrations.mcp import MAX_LINE_BYTES, Server, serve
from dzc_integrations.picker import emit_selection, rofi_script, select
from dzc_integrations.tui import preview_lines


def entry(entry_id=42, **kwargs):
    return {"id": entry_id, "type": "text", "content": "line one\nline two\n",
            "preview": "line one", "tags": [], "pinned": False,
            "timestamp": 1000, "mime": "text/plain", "sensitive": False, **kwargs}


class FakeClient:
    def __init__(self, rows=None):
        self.rows = [entry()] if rows is None else rows
        self.calls = []

    def entries(self, **kwargs):
        self.calls.append(("entries", kwargs))
        return self.rows

    def entry(self, entry_id, **kwargs):
        self.calls.append(("entry", entry_id, kwargs))
        return next(row for row in self.rows if row["id"] == entry_id)

    def raw(self, entry_id):
        self.calls.append(("raw", entry_id))
        return self.entry(entry_id)["content"].encode("utf-8")

    def copy(self, entry_id):
        self.calls.append(("copy", entry_id))


def request(method, params=None, request_id=1):
    result = {"jsonrpc": "2.0", "method": method, "id": request_id}
    if params is not None:
        result["params"] = params
    return result


def ready_server(client=None):
    server = Server(client or FakeClient())
    server.handle(request("initialize", {"protocolVersion": "2025-11-25",
                                        "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}))
    server.handle({"jsonrpc": "2.0", "method": "notifications/initialized"})
    return server


class ClientTests(unittest.TestCase):
    def test_metadata_and_privacy_flags_cross_the_process_boundary(self):
        client = DzcClient("C:/installed path/dzc.exe")
        with patch.object(client, "json", return_value=[entry()]) as run:
            client.entries(limit=7, offset=20, exclude_sensitive=True)
        args = run.call_args.args[0]
        self.assertEqual(args[0], "list")
        self.assertIn("--metadata", args)
        self.assertIn("--exclude-sensitive", args)
        self.assertEqual(args[-2:], ["--", "7"])
        self.assertEqual(args[args.index("--offset") + 1], "20")

    def test_a_query_starting_with_a_dash_remains_data(self):
        client = DzcClient("dzc")
        with patch.object(client, "json", return_value=[]) as run:
            client.entries(query="--delete; $(echo bad)", limit=3)
        args = run.call_args.args[0]
        self.assertEqual(args[-2:], ["--", "--delete; $(echo bad)"])

    def test_sensitive_markers_and_encrypted_fields_are_rejected(self):
        for secret in (entry(tags=["SENSITIVE"]), entry(tags=["密码"]),
                       entry(sensitive=True), entry(encrypted=True), entry(preview="linux:payload")):
            self.assertTrue(sensitive(secret))
            client = DzcClient("dzc")
            with patch.object(client, "json", return_value=secret):
                with self.assertRaises(AdapterError):
                    client.entry(42, exclude_sensitive=True)

    def test_invalid_ids_and_page_sizes_never_run_a_command(self):
        client = DzcClient("dzc")
        with patch.object(client, "run") as run:
            for invalid in (True, 0, -1, "42", 2**64):
                with self.assertRaises(AdapterError):
                    client.raw(invalid)
            with self.assertRaises(AdapterError):
                client.entries(limit=201)
            run.assert_not_called()

    def test_terminal_controls_cannot_create_a_picker_row_or_escape_sequence(self):
        row = row_text(entry(preview="first\n42\tsecond\x1b[31m\x00\u202Esecret"))
        self.assertEqual(row.count("\t"), 1)
        self.assertNotIn("\n", row)
        self.assertNotIn("\x1b", row)
        self.assertNotIn("\u202E", row)
        self.assertIn("second", row)
        self.assertEqual(display_text("字" * 10, 3), "字字字")

    def test_preview_preserves_lines_but_strips_terminal_controls(self):
        self.assertEqual(preview_lines("one\n\x1b[31mtwo\u202E", 80), ["one", "[31mtwo"])

    def test_subprocess_boundary_is_binary_exact_and_bounds_output(self):
        client = DzcClient(sys.executable)
        self.assertEqual(client.run(["-c", "import sys; sys.stdout.buffer.write(b'one\\r\\ntwo\\n\\n')"]),
                         b"one\r\ntwo\n\n")
        with self.assertRaises(AdapterError):
            client.run(["-c", "print('more than five bytes')"], max_bytes=5)
        with self.assertRaises(AdapterError) as failure:
            client.run(["-c", "import sys; print('private content', file=sys.stderr); sys.exit(1)"])
        self.assertNotIn("private content", str(failure.exception))


class PickerTests(unittest.TestCase):
    def test_cancel_does_not_read_or_copy_an_entry(self):
        client = FakeClient()
        with patch("dzc_integrations.picker.shutil.which", return_value="fzf"), \
                patch("dzc_integrations.picker.subprocess.run", return_value=subprocess.CompletedProcess([], 1, b"")):
            chosen = select(client)
        self.assertIsNone(chosen)
        self.assertEqual(emit_selection(client, chosen, "copy"), 1)
        self.assertEqual(len(client.calls), 1)

    def test_forged_picker_output_never_becomes_an_id(self):
        client = FakeClient()
        with patch("dzc_integrations.picker.shutil.which", return_value="fzf"), \
                patch("dzc_integrations.picker.subprocess.run", return_value=subprocess.CompletedProcess([], 0, b"999\tforged\n")):
            with self.assertRaises(AdapterError):
                select(client)

    def test_selection_uses_argument_vector_and_clears_shell_bindings(self):
        client = FakeClient()
        row = row_text(client.rows[0]).encode("utf-8") + b"\n"
        with patch("dzc_integrations.picker.shutil.which", return_value="C:/path with spaces/fzf.exe"), \
                patch.dict("os.environ", {"FZF_DEFAULT_OPTS": "--bind=enter:execute(evil)"}), \
                patch("dzc_integrations.picker.subprocess.run", return_value=subprocess.CompletedProcess([], 0, row)) as run:
            chosen = select(client)
        self.assertEqual(chosen["id"], 42)
        self.assertEqual(run.call_args.args[0][0], "C:/path with spaces/fzf.exe")
        self.assertNotIn("shell", run.call_args.kwargs)
        self.assertNotIn("FZF_DEFAULT_OPTS", run.call_args.kwargs["env"])

    def test_copy_uses_native_cli_not_a_shell(self):
        client = FakeClient()
        self.assertEqual(emit_selection(client, entry(), "copy"), 0)
        self.assertEqual(client.calls, [("copy", 42)])

    def test_shell_insert_refuses_nul_instead_of_silently_corrupting_content(self):
        client = FakeClient([entry(content="before\0after")])
        with self.assertRaises(AdapterError):
            emit_selection(client, entry(), "insert")

    def test_rofi_copies_hidden_info_not_visible_clipboard_text(self):
        client = FakeClient()
        with patch.dict("os.environ", {"ROFI_RETV": "1", "ROFI_INFO": "42"}):
            self.assertEqual(rofi_script(client), 0)
        self.assertEqual(client.calls, [("copy", 42)])
        with patch.dict("os.environ", {"ROFI_RETV": "1", "ROFI_INFO": "42;bad"}):
            with self.assertRaises(AdapterError):
                rofi_script(client)


class McpTests(unittest.TestCase):
    def call(self, server, name, arguments):
        return server.handle(request("tools/call", {"name": name, "arguments": arguments}))

    def test_initialize_negotiates_supported_version_and_requires_ready_notification(self):
        server = Server(FakeClient())
        self.assertIn("error", server.handle(request("tools/list")))
        response = server.handle(request("initialize", {"protocolVersion": "2025-03-26",
                                                         "capabilities": {}, "clientInfo": {}}))
        self.assertEqual(response["result"]["protocolVersion"], "2025-03-26")
        self.assertIn("error", server.handle(request("tools/list")))
        self.assertIsNone(server.handle({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        self.assertEqual(len(server.handle(request("tools/list"))["result"]["tools"]), 3)

    def test_only_readonly_history_tools_are_exposed(self):
        server = ready_server()
        tools = server.handle(request("tools/list"))["result"]["tools"]
        self.assertEqual({tool["name"] for tool in tools}, {"clipboard_list", "clipboard_search", "clipboard_get"})
        self.assertTrue(all(tool["annotations"]["readOnlyHint"] for tool in tools))
        for name in ("clipboard_delete", "clipboard_copy", "clipboard_add", "exec"):
            response = self.call(server, name, {})
            self.assertEqual(response["error"]["code"], -32602)
        for name in ([], {}, 42, None):
            response = self.call(server, name, {})
            self.assertEqual(response["error"]["code"], -32602)

    def test_list_is_metadata_only_and_filters_sensitive_rows_defensively(self):
        client = FakeClient([entry(), entry(99, tags=["密码"], content="secret-value")])
        response = self.call(ready_server(client), "clipboard_list", {"limit": 2, "offset": 4})
        text = response["result"]["content"][0]["text"]
        data = json.loads(text)
        self.assertEqual(len(data["entries"]), 1)
        self.assertNotIn("secret-value", text)
        self.assertNotIn("content", data["entries"][0])
        self.assertEqual(client.calls[0][1]["exclude_sensitive"], True)
        self.assertEqual(client.calls[0][1]["offset"], 4)

    def test_get_bounds_content_and_rejects_sensitive_payload(self):
        client = FakeClient([entry(content="你好abcd", extra_private_field="not exposed")])
        response = self.call(ready_server(client), "clipboard_get", {"id": 42, "max_chars": 3})
        data = json.loads(response["result"]["content"][0]["text"])
        self.assertEqual(data["content"], "你好a")
        self.assertTrue(data["truncated"])
        self.assertNotIn("extra_private_field", data)
        self.assertTrue(client.calls[0][2]["exclude_sensitive"])
        response = self.call(ready_server(FakeClient([entry(sensitive=True)])), "clipboard_get", {"id": 42})
        self.assertTrue(response["result"]["isError"])
        self.assertNotIn("line one", response["result"]["content"][0]["text"])

    def test_arguments_cannot_disable_privacy_or_set_unbounded_limits(self):
        client = FakeClient()
        server = ready_server(client)
        for name, arguments in (("clipboard_list", {"limit": 101}),
                                ("clipboard_list", {"limit": True}),
                                ("clipboard_search", {"query": ""}),
                                ("clipboard_get", {"id": 42, "include_sensitive": True}),
                                ("clipboard_get", {"id": 42, "max_chars": 16385})):
            response = self.call(server, name, arguments)
            self.assertTrue(response["result"]["isError"])
        self.assertEqual(client.calls, [])

    def test_stdio_is_utf8_ndjson_and_notifications_have_no_response(self):
        messages = [request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {}}),
                    {"jsonrpc": "2.0", "method": "notifications/initialized"},
                    request("tools/call", {"name": "clipboard_search", "arguments": {"query": "你好"}}, 2)]
        source = io.BytesIO(b"not json\n" + b"\n".join(json.dumps(item).encode("utf-8") for item in messages) + b"\n")
        destination = io.BytesIO()
        self.assertEqual(serve(FakeClient(), source, destination), 0)
        frames = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertEqual(len(frames), 3)
        self.assertEqual(frames[0]["error"]["code"], -32700)
        self.assertEqual(frames[-1]["id"], 2)

    def test_oversized_frame_terminates_without_buffering_the_remaining_input(self):
        source = io.BytesIO(b"x" * (MAX_LINE_BYTES * 2))
        destination = io.BytesIO()
        self.assertEqual(serve(FakeClient(), source, destination), 1)
        self.assertEqual(source.tell(), MAX_LINE_BYTES + 1)
        self.assertEqual(json.loads(destination.getvalue())["error"]["code"], -32600)

    def test_protocol_errors_never_crash_or_echo_request_payloads(self):
        server = ready_server()
        for invalid in ([], None, 42, {"jsonrpc": "1.0", "id": 1},
                        request("tools/call", []), request("ping", request_id=True)):
            self.assertIn("error", server.handle(invalid))
        self.assertIsNone(server.handle({"jsonrpc": "2.0", "method": "notifications/cancelled"}))
        self.assertIsNone(server.handle({"jsonrpc": "2.0", "id": "unsolicited", "result": {}}))
        self.assertIsNone(server.handle(request("ping", request_id=True))["id"])
        self.assertEqual(server.handle(request("ping"))["result"], {})

    def test_deeply_nested_json_is_rejected_and_does_not_end_the_session(self):
        source = io.BytesIO(b"[" * 2000 + b"]" * 2000 + b"\n" +
                            json.dumps(request("ping", request_id=2)).encode("utf-8") + b"\n")
        destination = io.BytesIO()
        self.assertEqual(serve(FakeClient(), source, destination), 0)
        responses = [json.loads(line) for line in destination.getvalue().splitlines()]
        self.assertIn(responses[0]["error"]["code"], (-32700, -32600))
        self.assertEqual(responses[1]["result"], {})


class ShellTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty"),
                         "Windows widget interaction needs optional pywinpty")
    def test_powershell_widget_inserts_literal_text_without_submitting_it(self):
        import os
        import shutil
        import socket
        import time
        from winpty import PtyProcess

        powershell = shutil.which("pwsh")
        if not powershell:
            self.skipTest("PowerShell 7 is unavailable")
        payload = "中文 DZC_WIDGET_LITERAL\n$(not-a-command)\n\n"
        widget = Path(__file__).resolve().parents[1] / "shell" / "dzc.ps1"
        with tempfile.TemporaryDirectory(prefix="dzc-powershell-widget-") as directory:
            adapter = Path(directory) / "fake adapter.py"
            buffer = Path(directory) / "buffer.txt"
            setup = Path(directory) / "setup.ps1"
            adapter.write_text("import sys\nsys.stdout.buffer.write(" + repr(payload.encode("utf-8")) + ")\n", encoding="utf-8")
            quote = lambda path: "'" + str(path).replace("'", "''") + "'"
            setup.write_text(
                "Import-Module PSReadLine\n"
                ". " + quote(widget) + "\n"
                "Set-PSReadLineOption -BellStyle None\n"
                "Set-PSReadLineKeyHandler -Chord Ctrl+x -ScriptBlock {\n"
                " $taskLine = ''; $taskCursor = 0\n"
                " [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$taskLine, [ref]$taskCursor)\n"
                " [System.IO.File]::WriteAllText(" + quote(buffer) + ", $taskLine, [Text.UTF8Encoding]::new($false))\n"
                " [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()\n"
                "}\n"
                "function prompt { 'DZC_WIDGET_READY> ' }\n", encoding="utf-8")
            environment = {**os.environ, "DZC_ADAPTER": str(adapter), "DZC_PYTHON": sys.executable}
            process = PtyProcess.spawn([powershell, "-NoLogo", "-NoProfile", "-NoExit", "-File", str(setup)],
                                       env=environment, dimensions=(30, 110), backend="0")
            process.fileobj.settimeout(0.2)
            screen = ""
            try:
                deadline = time.monotonic() + 10
                while "DZC_WIDGET_READY>" not in screen and time.monotonic() < deadline:
                    try:
                        screen += process.read(65536)
                    except socket.timeout:
                        pass
                self.assertIn("DZC_WIDGET_READY>", screen)
                process.write("prefix:\x07")
                deadline = time.monotonic() + 8
                while "DZC_WIDGET_LITERAL" not in screen and time.monotonic() < deadline:
                    try:
                        screen += process.read(65536)
                    except socket.timeout:
                        pass
                self.assertIn("DZC_WIDGET_LITERAL", screen)
                process.write("\x18")
                deadline = time.monotonic() + 5
                while not buffer.exists() and time.monotonic() < deadline:
                    try:
                        screen += process.read(65536)
                    except socket.timeout:
                        pass
                self.assertTrue(buffer.exists(), screen)
                self.assertEqual(buffer.read_bytes(), ("prefix:" + payload).encode("utf-8"))
            finally:
                if process.isalive():
                    process.write("\x03exit\r")
                process.close(force=True)

    @unittest.skipIf(sys.platform == "win32", "POSIX shell behavior is checked on Linux/WSL")
    def test_shell_widgets_insert_unicode_multiline_and_trailing_newlines_without_execution(self):
        import shutil
        payload = "中文\n$(printf untrusted)\n\n"
        shell_root = Path(__file__).resolve().parents[1] / "shell"
        with tempfile.TemporaryDirectory() as directory:
            adapter = Path(directory) / "fake adapter.py"
            adapter.write_text("import sys\nsys.stdout.buffer.write(" + repr(payload.encode("utf-8")) + ")\n", encoding="utf-8")
            commands = {
                "bash": 'source "$1"; DZC_PYTHON="$2"; DZC_ADAPTER="$3"; READLINE_LINE="pre:tail"; '
                        'READLINE_POINT=4; _dzc_insert; printf "%s" "$READLINE_LINE"',
                "zsh": 'source "$1"; DZC_PYTHON="$2"; DZC_ADAPTER="$3"; '
                       'zle() { :; }; LBUFFER="pre:"; RBUFFER="tail"; _dzc_insert; printf "%s" "$LBUFFER$RBUFFER"',
                "fish": 'source "$argv[1]"; set -g DZC_PYTHON "$argv[2]"; set -g DZC_ADAPTER "$argv[3]"; '
                        'function commandline; if test "$argv[1]" = --insert; printf "pre:%stail" "$argv[-1]"; end; end; __dzc_insert',
            }
            for shell, command in commands.items():
                if not shutil.which(shell):
                    continue
                with self.subTest(shell=shell):
                    script = shell_root / ("dzc." + shell)
                    arguments = [shell, "-c", command]
                    if shell != "fish":
                        arguments += ["widget-test"]
                    result = subprocess.run([*arguments, str(script), sys.executable, str(adapter)],
                                            capture_output=True, check=True)
                    self.assertEqual(result.stdout.decode("utf-8"), "pre:" + payload + "tail")

    @unittest.skipUnless(sys.platform.startswith("linux"), "Requires a POSIX controlling terminal")
    def test_tui_controls_stay_on_terminal_when_selected_text_is_captured(self):
        import fcntl
        import os
        import pty
        import select as select_io
        import struct
        import termios
        import time
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        root = str(Path(__file__).resolve().parents[1])
        program = (
            "import sys; sys.path.insert(0, " + repr(root) + ")\n"
            "from dzc_integrations.tui import run_tui\n"
            "from dzc_integrations.picker import emit_selection\n"
            "class Client:\n"
            " def entries(self, *args, **kwargs): return [{'id': 1, 'type': 'text', 'preview': 'preview', 'tags': []}]\n"
            " def entry(self, entry_id): return {'content': 'one\\ntwo\\n\\n'}\n"
            " def raw(self, entry_id): return b'one\\ntwo\\n\\n'\n"
            "client = Client()\n"
            "emit_selection(client, run_tui(client), 'print')\n"
        )

        def own_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        process = subprocess.Popen([sys.executable, "-c", program], stdin=slave,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env={**os.environ, "TERM": "xterm"}, preexec_fn=own_terminal)
        os.close(slave)
        screen = b""
        try:
            deadline = time.monotonic() + 5
            while b"Enter print" not in screen and time.monotonic() < deadline:
                ready, _, _ = select_io.select([master], [], [], 0.1)
                if ready:
                    screen += os.read(master, 65536)
            self.assertIn(b"Enter print", screen)
            os.write(master, b"\n")
            output, errors = process.communicate(timeout=5)
            self.assertEqual(process.returncode, 0, errors)
            self.assertEqual(output, b"one\ntwo\n\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()
            os.close(master)


if __name__ == "__main__":
    unittest.main()
