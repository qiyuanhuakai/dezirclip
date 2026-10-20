"""Process-level coverage for the self-contained Rust MCP command."""
from contextlib import closing
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from sqlite_history_fixture import (LINUX_ENVELOPE_FIXTURE, connect_fixture_database,
                                    mark_legacy_plaintext_sensitive_fixture)


@unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN to test native commands")
class NativeMcpTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="dzc-native-mcp-")
        self.addCleanup(self.directory.cleanup)
        self.database = Path(self.directory.name) / "history.db"
        self.binary = str(Path(os.environ["DZC_TEST_BIN"]).resolve())
        self.environment = dict(os.environ, DEZIRCLIP_DB_PATH=str(self.database),
                                PATH=str(Path(self.directory.name) / "no-external-tools"))

    def cli(self, *args, payload=None):
        return subprocess.run([self.binary, *args], input=payload, capture_output=True,
                              env=self.environment, timeout=30, check=True).stdout

    def request(self, method, params=None, identifier=1):
        value = {"jsonrpc": "2.0", "method": method, "id": identifier}
        if params is not None:
            value["params"] = params
        return value

    def initialize(self):
        return [self.request("initialize", {"protocolVersion": "2025-11-25",
                                            "capabilities": {}, "clientInfo": {}}),
                {"jsonrpc": "2.0", "method": "notifications/initialized"}]

    def mcp(self, requests):
        payload = b"\n".join(json.dumps(item).encode("utf-8") for item in requests) + b"\n"
        result = subprocess.run([self.binary, "mcp"], input=payload, capture_output=True,
                                env=self.environment, timeout=30, check=True)
        self.assertEqual(result.stderr, b"")
        return {frame["id"]: frame for frame in map(json.loads, result.stdout.splitlines())}

    def test_initialization_tools_and_invalid_arguments_leave_database_unopened(self):
        frames = self.mcp([*self.initialize(), self.request("tools/list", identifier=2),
                           self.request("tools/call", {"name": "clipboard_list",
                                                       "arguments": {"limit": 101}}, 3)])
        self.assertFalse(self.database.exists())
        tools = frames[2]["result"]["tools"]
        self.assertEqual({item["name"] for item in tools},
                         {"clipboard_list", "clipboard_search", "clipboard_get"})
        self.assertTrue(all(item["annotations"]["readOnlyHint"] for item in tools))
        self.assertTrue(frames[3]["result"]["isError"])

    def test_real_history_is_bounded_private_and_readonly_through_stdio(self):
        payload = "public\r\n你好🙂\n\n"
        public_id = int(self.cli("add", "-", payload=payload.encode()).decode().strip().rsplit("=", 1)[1])
        secret_id = int(self.cli("add", "private-never-expose").decode().strip().rsplit("=", 1)[1])
        encrypted_id = int(self.cli("add", "encrypted").decode().strip().rsplit("=", 1)[1])
        with closing(connect_fixture_database(self.database)) as database:
            mark_legacy_plaintext_sensitive_fixture(database, secret_id)
            database.execute("UPDATE clipboard_history SET content=?,preview=? WHERE id=?",
                             (LINUX_ENVELOPE_FIXTURE, LINUX_ENVELOPE_FIXTURE, encrypted_id))
            database.commit()
        requests = [*self.initialize(),
                    self.request("tools/call", {"name": "clipboard_list", "arguments": {"limit": 1}}, 2),
                    self.request("tools/call", {"name": "clipboard_get", "arguments": {"id": public_id, "max_chars": 3}}, 3),
                    self.request("tools/call", {"name": "clipboard_get", "arguments": {"id": secret_id}}, 4),
                    self.request("tools/call", {"name": "clipboard_get", "arguments": {"id": encrypted_id}}, 5),
                    self.request("tools/call", {"name": "clipboard_delete", "arguments": {"id": public_id}}, 6)]
        frames = self.mcp(requests)
        listed = json.loads(frames[2]["result"]["content"][0]["text"])
        self.assertEqual([row["id"] for row in listed["entries"]], [public_id])
        self.assertNotIn("content", listed["entries"][0])
        fetched = json.loads(frames[3]["result"]["content"][0]["text"])
        self.assertEqual(fetched["content"], "pub")
        self.assertTrue(fetched["truncated"])
        self.assertTrue(frames[4]["result"]["isError"])
        self.assertTrue(frames[5]["result"]["isError"])
        self.assertIn("error", frames[6])
        self.assertNotIn("private-never-expose", json.dumps(frames))
        self.assertEqual(json.loads(self.cli("count", "--json")), {"count": 3})
        self.assertEqual(self.cli("get", str(public_id), "--raw"), payload.encode())
