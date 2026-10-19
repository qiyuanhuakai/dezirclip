"""Search type filters must reach the database before either pagination bound."""

from contextlib import closing
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from dzc_integrations.client import DzcClient
from sqlite_history_fixture import connect_fixture_database


class SearchKindBoundaryTests(unittest.TestCase):
    def test_type_and_pagination_are_forwarded_without_reinterpreting_query(self):
        client = DzcClient("dzc-test")
        with patch.object(client, "json", return_value=[]) as invoke:
            self.assertEqual(client.entries(query="--needle", kind="code", limit=1, offset=2,
                                            exclude_sensitive=True), [])
        invoke.assert_called_once_with([
            "search", "--json", "--metadata", "--limit", "1", "--offset", "2",
            "--kind", "code", "--exclude-sensitive", "--", "--needle",
        ])


@unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN to run real CLI search checks")
class RealSearchKindTests(unittest.TestCase):
    def test_mixed_types_filter_before_limit_and_nonzero_offset_in_all_modes(self):
        with tempfile.TemporaryDirectory(prefix="dzc-search-kind-") as directory:
            database = Path(directory) / "history.db"
            with patch.dict(os.environ, {"DEZIRCLIP_DB_PATH": str(database)}):
                client = DzcClient(os.environ["DZC_TEST_BIN"])
                ids = []
                for number, kind in enumerate(("code", "text", "code", "url", "text")):
                    result = client.run(["add", "-", "--type", kind],
                                        payload=f"needle record {number}".encode("utf-8"))
                    ids.append(int(result.decode("utf-8").strip().rsplit("=", 1)[1]))
                with closing(connect_fixture_database(database)) as connection:
                    connection.execute("UPDATE clipboard_history SET timestamp=id")
                    connection.commit()
                self.assertEqual([row["id"] for row in client.entries(query="needle", limit=1)],
                                 [ids[4]])
                for offset, expected in ((0, [ids[2]]), (1, [ids[0]]), (2, [])):
                    with self.subTest(adapter_offset=offset):
                        rows = client.entries(query="needle", kind="code", limit=1, offset=offset)
                        self.assertEqual([row["id"] for row in rows], expected)
                for mode, query in (("contains", "needle"), ("fuzzy", "ndl"),
                                    ("regex", "n.*dle"), ("fts5", "needle")):
                    for offset, expected in ((0, [ids[2]]), (1, [ids[0]]), (2, [])):
                        with self.subTest(mode=mode, offset=offset):
                            rows = client.json([
                                "search", "--kind", "code", "--mode", mode,
                                "--limit", "1", "--offset", str(offset),
                                "--metadata", "--json", "--", query,
                            ])
                            self.assertEqual([row["id"] for row in rows], expected)
                            self.assertTrue(all(row["type"] == "code" for row in rows))
                    with self.subTest(mode=mode, kind="missing"):
                        self.assertEqual(client.json([
                            "search", "--kind", "missing", "--mode", mode,
                            "--limit", "1", "--metadata", "--json", "--", query,
                        ]), [])


if __name__ == "__main__":
    unittest.main()
