"""Disposable fixture writers must preserve schemas 20/21 search privacy contract."""

import base64
from contextlib import closing, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import sqlite3
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPT_ROOT))
from sqlite_history_fixture import (LINUX_ENVELOPE_FIXTURE, connect_fixture_database,
                                    has_ciphertext_envelope, search_visible, require_secure_delete_runtime)


def dpapi_blob():
    word = lambda value: struct.pack("<I", value)
    blob = lambda value: word(len(value)) + value
    return (word(1) + bytes.fromhex("d08c9ddf0115d1118c7a00c04fc297eb") + word(1)
            + bytes([42]) * 16 + word(0) + blob(b"\0\0") + word(0x6610) + word(256)
            + blob(bytes([1]) * 32) + blob(b"") + word(0x800E) + word(512)
            + blob(bytes([2]) * 32) + blob(bytes([3]) * 16) + blob(bytes([4]) * 64))


class FixtureVisibilityTests(unittest.TestCase):
    def test_tags_are_typed_json_with_ascii_sensitive_markers_and_null_default(self):
        for tags in (None, "[]", '["work"]', '["ſensitive"]'):
            self.assertTrue(search_visible(tags, "body", None, None, None))
        for tags in ('["sensitive"]', '["SENSITIVE"]', '["\\u0073ensitive"]', '["密码"]',
                     "not JSON", "null", "{}", '[1]', '[null]', '[true]', '["\\ud800"]', b"[]", 1):
            with self.subTest(tags=tags):
                self.assertFalse(search_visible(tags, "body", None, None, None))
        for whitespace in ("\t\n\v\f\r \x85\xa0\u1680"
                           "\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a"
                           "\u2028\u2029\u202f\u205f\u3000"):
            for marker in ("sensitive", "SENSITIVE", "密码"):
                with self.subTest(whitespace=repr(whitespace), marker=marker):
                    tags = json.dumps([whitespace + marker + whitespace])
                    self.assertFalse(search_visible(tags, "body", None, None, None))
        for non_whitespace in ("\x1c", "\x1d", "\x1e", "\x1f", "\u180e", "\u200b", "\ufeff"):
            for marker in ("sensitive", "密码"):
                with self.subTest(non_whitespace=repr(non_whitespace), marker=marker):
                    tags = json.dumps([non_whitespace + marker + non_whitespace])
                    self.assertTrue(search_visible(tags, "body", None, None, None))

    def test_every_text_field_hides_complete_envelopes_without_decryption(self):
        for value in (LINUX_ENVELOPE_FIXTURE, "dpapi:" + base64.b64encode(dpapi_blob()).decode()):
            self.assertTrue(has_ciphertext_envelope(value))
            for field in range(4):
                fields = ["ordinary", None, None, None]
                fields[field] = value
                self.assertFalse(search_visible("[]", *fields))
        # A structurally complete envelope stays excluded even when its payload is damaged.
        damaged = "linux:" + base64.b64encode(bytes([255]) * 28).decode()
        self.assertFalse(search_visible("[]", damaged, None, None, None))
        self.assertFalse(search_visible("[]", b"body", None, None, None))

    def test_plain_prefixes_and_incomplete_envelopes_remain_literal_text(self):
        values = ["linux: kernel notes", "dpapi: configuration", "linux:unreadable", "dpapi:",
                  "dpapi:" + base64.b64encode(bytes([42]) * 200).decode()]
        values.extend("linux:" + base64.b64encode(bytes(length)).decode() for length in range(28))
        payload = base64.b64encode(bytes(range(28))).decode()
        # Python's decoder accepts nonzero padding bits; Rust STANDARD rejects them.
        values.append("linux:" + payload[:-3] + "x==")
        values.append(LINUX_ENVELOPE_FIXTURE + "\n")
        for value in values:
            with self.subTest(value=value):
                self.assertFalse(has_ciphertext_envelope(value))
                self.assertTrue(search_visible("[]", value, None, None, None))

    def test_dpapi_requires_provider_lengths_algorithms_and_complete_consumption(self):
        complete = dpapi_blob()
        invalid = [complete[:length] for length in range(len(complete))]
        invalid.append(complete + b"trailing")
        wrong_provider = bytearray(complete)
        wrong_provider[4] ^= 1
        invalid.append(wrong_provider)
        oversized = bytearray(complete)
        oversized[44:48] = struct.pack("<I", 2**32 - 1)
        invalid.append(oversized)
        for value in invalid:
            self.assertFalse(has_ciphertext_envelope("dpapi:" + base64.b64encode(value).decode()))

    def test_fixture_opener_registers_five_argument_deterministic_udf(self):
        with closing(connect_fixture_database(":memory:")) as database:
            self.assertEqual(database.execute("SELECT dc_search_visible(NULL,'body',NULL,NULL,NULL)").fetchone(), (1,))
            self.assertEqual(database.execute("SELECT dc_search_visible(?,NULL,NULL,NULL,NULL)", ('["SENSITIVE"]',)).fetchone(), (0,))
            function = next(row for row in database.execute("PRAGMA function_list") if row[0] == "dc_search_visible")
            self.assertEqual(function[4], 5)
            self.assertTrue(function[5] & 0x800)


class SchemaTwentyFixtureTests(unittest.TestCase):
    """Historical schema20 remains writable without automatic schema migration."""
    schema_version = 20

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix=f"dzc-schema{self.schema_version}-contract-")
        self.path = Path(self.directory.name) / "history.db"
        with closing(sqlite3.connect(self.path)) as connection, connection:
            connection.executescript("""
                CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
                CREATE VIRTUAL TABLE clipboard_fts USING fts5(content);
                INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('secure-delete',1);
                CREATE TABLE reject_failure(value INTEGER CHECK(value>0));
            """)
            connection.execute("INSERT INTO schema_migrations VALUES(?)", (self.schema_version,))
            if self.schema_version == 21:
                connection.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('automerge',8)")

    def tearDown(self):
        self.directory.cleanup()

    def test_real_format5_writer_reopens_and_preserves_search_and_rollback(self):
        require_secure_delete_runtime()
        with closing(connect_fixture_database(self.path)) as connection, connection:
            self.assertEqual(connection.execute("PRAGMA secure_delete").fetchone(), (1,))
            connection.executemany("INSERT INTO clipboard_fts(rowid,content) VALUES(?,?)",
                                   [(1, "uniqueprivatemarker sharedmarker"), (2, "publicmarker sharedmarker")]
                                   + [(index, f"fixturefillermarker{index:05d}") for index in range(3, 1003)])
        with closing(connect_fixture_database(self.path)) as connection, connection:
            connection.execute("UPDATE clipboard_fts SET content='replacementmarker' WHERE rowid=1")
            self.assertEqual(connection.execute("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'uniqueprivatemarker'").fetchone(), (0,))
            self.assertEqual(connection.execute("SELECT rowid FROM clipboard_fts WHERE clipboard_fts MATCH 'sharedmarker'").fetchall(), [(2,)])
            connection.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)")
        with closing(connect_fixture_database(self.path)) as connection:
            self.assertEqual(connection.execute("SELECT v FROM clipboard_fts_config WHERE k='version'").fetchone(), (5,))
            with self.assertRaises(sqlite3.IntegrityError), connection:
                connection.execute("UPDATE clipboard_fts SET content='failedmarker' WHERE rowid=2")
                connection.execute("INSERT INTO reject_failure VALUES(-1)")
            self.assertEqual(connection.execute("SELECT content FROM clipboard_fts WHERE rowid=2").fetchone(), ("publicmarker sharedmarker",))
            self.assertEqual(connection.execute("SELECT rowid FROM clipboard_fts WHERE clipboard_fts MATCH 'sharedmarker'").fetchall(), [(2,)])
            connection.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)")

    def test_readonly_reader_enforces_connection_protection(self):
        with closing(connect_fixture_database(self.path)) as connection, connection:
            connection.execute("INSERT INTO clipboard_fts(content) VALUES('beforereadonly')")
        with closing(connect_fixture_database(self.path)) as connection, connection:
            connection.execute("UPDATE clipboard_fts SET content='readonlymarker' WHERE rowid=1")
        with closing(connect_fixture_database(self.path.as_uri() + "?mode=ro", uri=True)) as connection:
            self.assertEqual(connection.execute("SELECT max(version) FROM schema_migrations").fetchone(), (self.schema_version,))
            self.assertEqual(connection.execute("PRAGMA secure_delete").fetchone(), (1,))
            self.assertEqual(connection.execute("SELECT v FROM clipboard_fts_config WHERE k='version'").fetchone(), (5,))
            self.assertEqual(connection.execute("SELECT rowid FROM clipboard_fts WHERE clipboard_fts MATCH 'readonlymarker'").fetchall(), [(1,)])
            with self.assertRaisesRegex(sqlite3.OperationalError, "readonly"):
                connection.execute("INSERT INTO clipboard_fts(content) VALUES('refused')")

    def test_legacy_writer_keeps_runtime_default_secure_delete(self):
        with closing(sqlite3.connect(":memory:")) as raw:
            default = raw.execute("PRAGMA secure_delete").fetchone()
        with closing(sqlite3.connect(self.path)) as connection, connection:
            connection.execute("UPDATE schema_migrations SET version=19")
        with closing(connect_fixture_database(self.path)) as connection:
            self.assertEqual(connection.execute("PRAGMA secure_delete").fetchone(), default)

    # Standalone FTS above forces actual format5; product external-content
    # protection/purge is verified by the patched Core tests and real CLI below.

    def test_rejects_disabled_index_protection_without_repairing_it(self):
        with closing(sqlite3.connect(self.path)) as connection, connection:
            connection.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('secure-delete',0)")
        with self.assertRaisesRegex(RuntimeError, "persistent FTS5 secure-delete=1"):
            connect_fixture_database(self.path)
        with closing(sqlite3.connect(self.path)) as connection:
            self.assertEqual(connection.execute("SELECT v FROM clipboard_fts_config WHERE k='secure-delete'").fetchone(), (0,))
            self.assertEqual(connection.execute("SELECT count(*) FROM clipboard_fts").fetchone(), (0,))

    def test_rejects_older_runtime_and_future_schema(self):
        with patch.object(sqlite3, "sqlite_version_info", (3, 50, 3)):
            with self.assertRaisesRegex(RuntimeError, "SQLite >= 3.50.4"):
                connect_fixture_database(self.path)
        with closing(sqlite3.connect(self.path)) as connection, connection:
            connection.execute("INSERT INTO schema_migrations VALUES(22)")
        with self.assertRaisesRegex(RuntimeError, "newer than supported"):
            connect_fixture_database(self.path)


class SchemaTwentyOneFixtureTests(SchemaTwentyFixtureTests):
    """Current scheduling keeps the complete schema20 secure-writer contract."""
    schema_version = 21

    def test_current_schedule_is_not_rewritten_by_the_fixture_opener(self):
        with closing(connect_fixture_database(self.path)) as connection:
            self.assertEqual(connection.execute("SELECT max(version) FROM schema_migrations").fetchone(), (21,))
            self.assertEqual(connection.execute("SELECT v FROM clipboard_fts_config WHERE k='automerge'").fetchone(), (8,))
            self.assertEqual(connection.execute("PRAGMA secure_delete").fetchone(), (1,))


@unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN for real schema21 cross-connection checks")
class SchemaTwentyOneWriterTests(unittest.TestCase):
    def test_cli_and_external_writer_keep_search_view_and_fts_consistent(self):
        with tempfile.TemporaryDirectory(prefix="dzc-schema21-writer-") as directory:
            path = Path(directory) / "history.db"
            environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(path)}
            binary = str(Path(os.environ["DZC_TEST_BIN"]).resolve())

            def cli(*arguments):
                return subprocess.run([binary, *arguments], capture_output=True, env=environment,
                                      timeout=30, check=True).stdout

            cli("add", "nativekeepmarker")
            with closing(sqlite3.connect(path)) as raw:
                self.assertEqual(raw.execute("SELECT max(version) FROM schema_migrations").fetchone(), (21,))
                self.assertEqual(raw.execute("SELECT v FROM clipboard_fts_config WHERE k='automerge'").fetchone(), (8,))
                self.assertEqual(raw.execute("SELECT v FROM clipboard_fts_config WHERE k='secure-delete'").fetchone(), (1,))
                with self.assertRaisesRegex(sqlite3.OperationalError, "no such function: dc_search_visible"):
                    raw.execute("UPDATE clipboard_history SET timestamp=id")
            with closing(connect_fixture_database(path)) as database:
                database.executemany(
                    "INSERT INTO clipboard_history(content_type,content,preview,source_app,timestamp,tags) "
                    "VALUES('text',?,?, 'fixture',2,?)",
                    [("linux: ordinaryprefixmarker", "dpapi: ordinary preview", "[]"),
                     ("privateftsmarker", "private summary", '["SENSITIVE"]'),
                     (LINUX_ENVELOPE_FIXTURE, "encryptedftsmarker", "[]"),
                     ("malformedftsmarker", "invalid tags", "not JSON")])
                database.commit()
                self.assertEqual(database.execute("SELECT count(*) FROM clipboard_search_content").fetchone(), (2,))
                for term in ("privateftsmarker", "encryptedftsmarker", "malformedftsmarker"):
                    self.assertEqual(database.execute("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?", (term,)).fetchone(), (0,))
                database.execute("UPDATE clipboard_history SET content='linux: updatedprefixmarker' WHERE id=2")
                database.execute("DELETE FROM clipboard_history WHERE id=1")
                database.commit()
                database.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)")
                self.assertEqual(database.execute("PRAGMA quick_check").fetchone(), ("ok",))
            result = json.loads(cli("search", "updatedprefixmarker", "--mode", "fts5", "--metadata", "--json"))
            self.assertEqual([entry["id"] for entry in result], [2])
            self.assertEqual(json.loads(cli("search", "privateftsmarker", "--mode", "fts5", "--metadata", "--json")), [])


@unittest.skipUnless(importlib.util.find_spec("PIL") and importlib.util.find_spec("psutil"),
                     "Synthetic image fixture checks need Pillow and psutil")
class SyntheticSeederTests(unittest.TestCase):
    def _assert_seed_cases(self, binary=None):
        spec = importlib.util.spec_from_file_location("seed_performance_fixture", SCRIPT_ROOT / "seed-performance-history.py")
        seed = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(seed)
        with tempfile.TemporaryDirectory(prefix="dzc-synthetic-seed-") as temporary:
            root = Path(temporary)
            seed.WORKSPACE_ROOT = root
            seed.CASES_ROOT = root / "cases"
            seed.CASES_ROOT.mkdir()
            cases = []
            for name in ("legacy-no-cli", "current" if binary else "legacy-fresh"):
                case = seed.CASES_ROOT / name
                (case / "data").mkdir(parents=True)
                executable = case / "synthetic-case.exe"
                executable.write_bytes(b"synthetic benchmark executable placeholder")
                (case / "case.json").write_text(json.dumps({"executable": str(executable)}), encoding="utf-8")
                path = case / "data" / "clipboard.db"
                if name == "current":
                    subprocess.run([binary, "count", "--json"],
                                   env={**os.environ, "DEZIRCLIP_DB_PATH": str(path)},
                                   capture_output=True, check=True, timeout=30)
                    with closing(sqlite3.connect(path)) as database:
                        self.assertEqual(database.execute("SELECT max(version) FROM schema_migrations").fetchone(), (21,))
                        self.assertEqual(database.execute("SELECT v FROM clipboard_fts_config WHERE k='automerge'").fetchone(), (8,))
                else:
                    with closing(sqlite3.connect(path)) as database:
                        database.executescript("CREATE TABLE clipboard_history(id INTEGER PRIMARY KEY,content_type TEXT,content TEXT,source_app TEXT,timestamp INTEGER,preview TEXT,tags TEXT); CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT);")
                cases.append(case)
            digests = []
            for case in [*cases, cases[-1]]:
                with redirect_stdout(io.StringIO()):
                    seed.seed(case, 12, 2, 64, 64)
                manifest = json.loads((case / "history-fixture.json").read_text(encoding="utf-8"))
                self.assertEqual(manifest["counts"], {"text": 12, "image": 2})
                digests.append(manifest["logical_sha256"])
                with closing(connect_fixture_database(case / "data" / "clipboard.db")) as database:
                    self.assertEqual(database.execute("PRAGMA quick_check").fetchone(), ("ok",))
                    if database.execute("SELECT 1 FROM sqlite_master WHERE name='clipboard_fts'").fetchone():
                        database.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)")
                        self.assertEqual(database.execute("SELECT count(*) FROM clipboard_fts_docsize").fetchone(), (14,))
                        self.assertEqual(database.execute("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'benchmark'").fetchone(), (12,))
            self.assertEqual(len(set(digests)), 1)

    def test_direct_sqlite_fixture_stays_deterministic_for_legacy_without_cli(self):
        self._assert_seed_cases()

    @unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN for real schema21 seeder checks")
    def test_direct_sqlite_fixture_stays_deterministic_for_legacy_and_schema21(self):
        self._assert_seed_cases(os.environ["DZC_TEST_BIN"])


if __name__ == "__main__":
    unittest.main()
