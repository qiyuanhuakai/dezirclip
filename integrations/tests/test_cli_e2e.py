"""Opt-in adapter checks against a real dzc binary and a disposable database."""

import json
import importlib.util
import os
from contextlib import closing
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from dzc_integrations.client import AdapterError, DzcClient
from sqlite_history_fixture import (LINUX_ENVELOPE_FIXTURE, connect_fixture_database,
                                    mark_legacy_plaintext_sensitive_fixture)


@unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN to run checks against a built dzc")
class RealCliTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="dzc-adapter-e2e-")
        self.database = Path(self.directory.name) / "history.db"
        self.environment = patch.dict(os.environ, {
            "DEZIRCLIP_DB_PATH": str(self.database), "DZC_BIN": os.environ["DZC_TEST_BIN"],
        })
        self.environment.start()
        self.client = DzcClient()

    def tearDown(self):
        self.environment.stop()
        self.directory.cleanup()

    def add(self, content, kind="text"):
        result = self.client.run(["add", "-", "--type", kind], payload=content.encode("utf-8"))
        return int(result.decode("utf-8").strip().rsplit("=", 1)[1])

    def test_raw_mime_and_json_preserve_unicode_crlf_and_trailing_newlines(self):
        content = "中文🙂\r\nsecond line\r\n\n\n"
        entry_id = self.add(content)
        self.assertEqual(self.client.raw(entry_id), content.encode("utf-8"))
        self.assertEqual(self.client.run(["get", str(entry_id), "--mime", "text/plain"]), content.encode("utf-8"))
        self.assertEqual(self.client.entry(entry_id)["content"], content)
        metadata = self.client.entries()[0]
        self.assertEqual(metadata["id"], entry_id)
        self.assertTrue({"id", "type", "preview", "tags", "pinned", "timestamp", "mime", "sensitive"}
                        <= metadata.keys())
        self.assertNotIn("content", metadata)
        self.assertIsInstance(metadata["id"], int)
        self.assertFalse(metadata["sensitive"])

    def test_pagination_and_tag_filters_apply_before_the_result_limit(self):
        ids = [self.add(f"record {number}") for number in range(7)]
        self.client.run(["tag", "add", str(ids[0]), "work"])
        self.client.run(["pin", str(ids[1]), "--quiet"])
        first = self.client.entries(limit=3)
        second = self.client.entries(limit=3, offset=3)
        last = self.client.entries(limit=3, offset=6)
        all_ids = [row["id"] for row in first + second + last]
        self.assertEqual(set(all_ids), set(ids))
        self.assertEqual(len(all_ids), len(set(all_ids)))
        self.assertEqual(first[0]["id"], ids[1])
        tagged = self.client.json(["list", "1", "--tag", "work", "--metadata", "--json"])
        self.assertEqual([row["id"] for row in tagged], [ids[0]])
        code_id = self.add("fn example() {}", kind="code")
        self.assertEqual([row["id"] for row in self.client.entries(kind="code", limit=1)], [code_id])

    def test_search_modes_match_full_payload_beyond_two_thousand_characters(self):
        entry_id = self.add("x" * 2500 + "needle-after-long-prefix")
        for mode, query in (("contains", "needle-after"), ("regex", "needle-after-.*-prefix"),
                            ("fuzzy", "nalp"), ("fts5", "needle")):
            with self.subTest(mode=mode):
                rows = self.client.json(["search", query, "--mode", mode, "--metadata", "--json"])
                self.assertEqual([row["id"] for row in rows], [entry_id])
                self.assertNotIn("content", rows[0])
                self.assertLessEqual(len(rows[0]["preview"]), 500)
        self.assertEqual([row["id"] for row in self.client.entries(query="needle-after")], [entry_id])

    def test_export_import_preserves_full_long_text_and_html_payloads(self):
        text = "Text start\r\n" + "中文🙂 line\r\n" * 400 + "tail\n\n"
        html = "<article>" + "<p>中文 long HTML payload</p>" * 400 + "</article>\n"
        self.assertGreater(len(text), 2500)
        self.assertGreater(len(html), 6000)
        text_id = self.add(text)
        html_id = self.add(html, kind="html")
        backup = Path(self.directory.name) / "backup.json"
        restored = Path(self.directory.name) / "restored.db"
        self.client.run(["export", str(backup), "--quiet"])
        exported = json.loads(backup.read_text(encoding="utf-8"))
        self.assertEqual({row["content"] for row in exported["entries"]}, {text, html})
        with patch.dict(os.environ, {"DEZIRCLIP_DB_PATH": str(restored)}):
            self.client.run(["import", str(backup), "--quiet"])
            self.assertEqual(self.client.json(["count", "--json"])["count"], 2)
            self.assertEqual(self.client.raw(text_id), text.encode("utf-8"))
            self.assertEqual(self.client.raw(html_id), html.encode("utf-8"))
            self.assertEqual(self.client.entry(html_id)["type"], "html")

    def test_replace_import_removes_protected_leftovers_and_preserves_sparse_ids(self):
        self.add("sparse text")
        self.add("sparse code", kind="code")
        backup = Path(self.directory.name) / "sparse.json"
        self.client.run(["export", str(backup), "--quiet"])
        exported = json.loads(backup.read_text(encoding="utf-8"))
        for row in exported["entries"]:
            row["id"] = 7 if row["content_type"] == "text" else 42
            row.update(use_count=17, is_pinned=True, pinned_order=9,
                       ocr_text="preserved OCR text", kinds=["code", "url"], tags=["imported"])
        backup.write_text(json.dumps(exported, ensure_ascii=False), encoding="utf-8")
        replaced = Path(self.directory.name) / "replace.db"
        with patch.dict(os.environ, {"DEZIRCLIP_DB_PATH": str(replaced)}):
            pinned_id = self.add("pinned leftover")
            tagged_id = self.add("tagged leftover")
            self.add("unprotected leftover")
            self.client.run(["pin", str(pinned_id), "--quiet"])
            self.client.run(["tag", "add", str(tagged_id), "keep"])
            self.client.run(["import", str(backup), "--mode", "replace", "--quiet"])
            self.assertEqual({row["id"] for row in self.client.entries()}, {7, 42})
            self.assertEqual(self.client.raw(7), b"sparse text")
            self.assertEqual(self.client.raw(42), b"sparse code")
            self.assertEqual(self.client.json(["count", "--json"])["count"], 2)
            with closing(sqlite3.connect(replaced)) as database:
                orphan_count = database.execute(
                    "SELECT count(*) FROM entry_tags et LEFT JOIN clipboard_history ch "
                    "ON et.entry_id=ch.id WHERE ch.id IS NULL"
                ).fetchone()[0]
                self.assertEqual(orphan_count, 0)
                metadata = database.execute(
                    "SELECT id,use_count,is_pinned,pinned_order,ocr_text,ocr_status,content_kinds,tags "
                    "FROM clipboard_history ORDER BY id"
                ).fetchall()
            self.assertEqual([row[0] for row in metadata], [7, 42])
            for row in metadata:
                self.assertEqual(row[1:6], (17, 1, 9, "preserved OCR text", "done"))
                self.assertEqual(json.loads(row[6]), ["code", "url"])
                self.assertEqual(json.loads(row[7]), ["imported"])

    def test_replace_rejects_unknown_backup_version_without_removing_originals(self):
        original_id = self.add("original before invalid backup")
        self.client.run(["pin", str(original_id), "--quiet"])
        backup = Path(self.directory.name) / "unknown-version.json"
        self.client.run(["export", str(backup), "--quiet"])
        exported = json.loads(backup.read_text(encoding="utf-8"))
        exported["version"] = "unsupported-export-v999"
        backup.write_text(json.dumps(exported), encoding="utf-8")
        with self.assertRaises(AdapterError):
            self.client.run(["import", str(backup), "--mode", "replace", "--quiet"])
        self.assertEqual(self.client.json(["count", "--json"])["count"], 1)
        self.assertEqual(self.client.raw(original_id), b"original before invalid backup")
        self.assertTrue(self.client.entry(original_id)["pinned"])

    def test_sensitive_tags_and_untagged_encrypted_fields_are_not_exposed(self):
        public_id = self.add("public marker")
        secret_id = self.add("secret marker")
        encrypted_id = self.add("encrypted marker")
        with closing(connect_fixture_database(self.database)) as database:
            mark_legacy_plaintext_sensitive_fixture(database, secret_id, "密码")
            database.execute("UPDATE clipboard_history SET content = ?, preview = ? WHERE id = ?",
                             (LINUX_ENVELOPE_FIXTURE, LINUX_ENVELOPE_FIXTURE, encrypted_id))
            database.commit()
        rows = self.client.entries(exclude_sensitive=True)
        self.assertEqual([row["id"] for row in rows], [public_id])
        rows = self.client.entries(query="marker", exclude_sensitive=True)
        self.assertEqual([row["id"] for row in rows], [public_id])
        for entry_id in (secret_id, encrypted_id):
            with self.assertRaises(AdapterError):
                self.client.entry(entry_id, exclude_sensitive=True)

    def test_actual_stdio_mcp_is_readonly_bounded_and_excludes_sensitive_data(self):
        public_id = self.add("public\r\n多行\n\n")
        secret_id = self.add("private-do-not-return")
        with closing(connect_fixture_database(self.database)) as database:
            mark_legacy_plaintext_sensitive_fixture(database, secret_id)
            database.commit()
        messages = [
            {"id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25",
                                                           "capabilities": {}, "clientInfo": {"name": "e2e", "version": "1"}}},
            {"method": "notifications/initialized"},
            {"id": 2, "method": "tools/list"},
            {"id": 3, "method": "tools/call", "params": {"name": "clipboard_list", "arguments": {"limit": 1}}},
            {"id": 4, "method": "tools/call", "params": {"name": "clipboard_get", "arguments": {"id": public_id, "max_chars": 3}}},
            {"id": 5, "method": "tools/call", "params": {"name": "clipboard_get", "arguments": {"id": secret_id}}},
            {"id": 6, "method": "tools/call", "params": {"name": "clipboard_search", "arguments": {"query": "public"}}},
            {"id": 7, "method": "tools/call", "params": {"name": "clipboard_delete", "arguments": {"id": public_id}}},
        ]
        payload = b"\n".join(json.dumps({"jsonrpc": "2.0", **message}).encode("utf-8") for message in messages) + b"\n"
        launcher = Path(__file__).resolve().parents[1] / "dzc-integrations.py"
        process = subprocess.run([sys.executable, str(launcher), "mcp"], input=payload,
                                 capture_output=True, timeout=30, check=True)
        frames = {frame["id"]: frame for frame in map(json.loads, process.stdout.splitlines())}
        self.assertEqual(set(frames), set(range(1, 8)))
        tools = frames[2]["result"]["tools"]
        self.assertTrue(all(tool["annotations"]["readOnlyHint"] for tool in tools))
        self.assertEqual(len(tools), 3)
        listed = json.loads(frames[3]["result"]["content"][0]["text"])
        self.assertEqual([row["id"] for row in listed["entries"]], [public_id])
        self.assertNotIn("content", listed["entries"][0])
        fetched = json.loads(frames[4]["result"]["content"][0]["text"])
        self.assertEqual(fetched["content"], "pub")
        self.assertTrue(fetched["truncated"])
        self.assertTrue(frames[5]["result"]["isError"])
        searched = json.loads(frames[6]["result"]["content"][0]["text"])
        self.assertEqual([row["id"] for row in searched["entries"]], [public_id])
        self.assertEqual(frames[7]["error"]["code"], -32602)
        self.assertNotIn(b"private-do-not-return", process.stdout)
        self.assertEqual(process.stderr, b"")
        self.assertEqual(self.client.json(["count", "--json"])["count"], 2)

    @unittest.skipUnless(sys.platform == "win32" and importlib.util.find_spec("winpty"),
                         "Windows terminal interaction needs optional pywinpty and windows-curses")
    def test_windows_tui_renders_and_returns_selected_content_through_a_redirect(self):
        import socket
        import time
        from winpty import PtyProcess

        content = b"Windows TUI selection\r\nsecond line\r\n\n"
        self.add(content.decode("utf-8"))
        output = Path(self.directory.name) / "selected.txt"
        launcher = Path(__file__).resolve().parents[1] / "dzc-integrations.py"
        program = (
            "import subprocess, sys\n"
            "with open(" + repr(str(output)) + ", 'wb') as output:\n"
            " result = subprocess.run([sys.executable, " + repr(str(launcher)) +
            ", 'tui', '--action', 'print'], stdout=output, timeout=15)\n"
            "print('DZC_TUI_EXIT=' + str(result.returncode), flush=True)\n"
            "raise SystemExit(result.returncode)\n"
        )
        process = PtyProcess.spawn([sys.executable, "-c", program],
                                   env=os.environ.copy(), dimensions=(30, 110), backend="0")
        process.fileobj.settimeout(0.2)
        screen = ""
        try:
            deadline = time.monotonic() + 8
            while "Enter print" not in screen and time.monotonic() < deadline:
                try:
                    screen += process.read(65536)
                except socket.timeout:
                    pass
                except EOFError:
                    break
            self.assertIn("DezirClip", screen)
            self.assertIn("Enter print", screen)
            process.write("\r")
            deadline = time.monotonic() + 8
            while "DZC_TUI_EXIT=" not in screen and time.monotonic() < deadline:
                try:
                    screen += process.read(65536)
                except socket.timeout:
                    pass
                except EOFError:
                    break
            self.assertIn("DZC_TUI_EXIT=0", screen)
            self.assertEqual(output.read_bytes(), content)
        finally:
            if process.isalive():
                process.write("q")
            process.close(force=True)


if __name__ == "__main__":
    unittest.main()
