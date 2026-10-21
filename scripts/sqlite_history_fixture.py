"""SQLite connections for disposable tests and synthetic render benchmarks only.

Schemas 19–23 need a pure visibility UDF on every fixture writer. Schemas 20–23
require SQLite >= 3.50.4, persistent FTS5 secure-delete, and connection-level
secure_delete. These trusted synthetic fixtures do not validate the product's
pinned SQLite engine or safely accept untrusted/corrupt database input.
SQLite 3.42 recognizes format 5; the higher fixture floor excludes older
external-content writers observed to corrupt these trusted cross-engine cases.
This mirrors that schema contract without decrypting data or opening a keyring.
Runtime integrations must continue using dzc/Core rather than this fixture helper.
Python's standard sqlite3 API exposes DETERMINISTIC but not INNOCUOUS; these
trusted, disposable fixture connections retain SQLite's default trusted_schema.
The production Rust connection separately registers its INNOCUOUS function.
"""

import base64
import binascii
import json
import sqlite3
import struct


_ASCII_LOWER = str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")
# Rust str::trim follows Unicode White_Space; Python's default strip also
# removes U+001C..U+001F, so pass the exact property characters explicitly.
_RUST_WHITE_SPACE = "\t\n\v\f\r \x85\xa0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000"
_DPAPI_PROVIDER = bytes.fromhex("d08c9ddf0115d1118c7a00c04fc297eb")
LINUX_ENVELOPE_FIXTURE = "linux:" + base64.b64encode(bytes(range(28))).decode("ascii")


def _valid_dpapi_envelope(data):
    position = 0

    def take(count):
        nonlocal position
        if count > len(data) - position:
            raise ValueError("truncated envelope")
        value = data[position:position + count]
        position += count
        return value

    def word():
        return struct.unpack("<I", take(4))[0]

    def blob():
        return take(word())

    try:
        if word() != 1 or take(16) != _DPAPI_PROVIDER or word() != 1:
            return False
        take(16)
        word()
        if len(blob()) % 2:
            return False
        cipher_algorithm, cipher_bits = word(), word()
        if cipher_algorithm & 0xE000 != 0x6000 or not 0 < cipher_bits <= 4096 or cipher_bits % 8:
            return False
        if not blob():
            return False
        blob()
        hash_algorithm, hash_bits = word(), word()
        if hash_algorithm & 0xE000 != 0x8000 or not 0 < hash_bits <= 4096 or hash_bits % 8:
            return False
        blob()
        return bool(blob()) and bool(blob()) and position == len(data)
    except (ValueError, struct.error):
        return False


def has_ciphertext_envelope(value):
    if value.startswith("linux:"):
        linux, payload = True, value[6:]
    elif value.startswith("dpapi:"):
        linux, payload = False, value[6:]
    else:
        return False
    try:
        data = base64.b64decode(payload, validate=True)
        if base64.b64encode(data).decode("ascii") != payload:
            return False
    except (ValueError, binascii.Error):
        return False
    return len(data) >= 28 if linux else _valid_dpapi_envelope(data)


def search_visible(tags, content, preview, html_content, ocr_text):
    if tags is None:
        tags = "[]"
    if not isinstance(tags, str):
        return False
    try:
        parsed = json.loads(tags)
        if not isinstance(parsed, list) or not all(isinstance(tag, str) for tag in parsed):
            return False
        for tag in parsed:
            tag.encode("utf-8")
            if tag.strip(_RUST_WHITE_SPACE).translate(_ASCII_LOWER) in ("sensitive", "密码"):
                return False
    except (ValueError, UnicodeError, RecursionError):
        return False
    for value in (content, preview, html_content, ocr_text):
        if value is not None:
            if not isinstance(value, str) or has_ciphertext_envelope(value):
                return False
    return True


def require_secure_delete_runtime():
    """Reject unsupported engines; only trusted disposable fixture data is accepted."""
    if sqlite3.sqlite_version_info < (3, 50, 4):
        raise RuntimeError("Schemas 20–23 synthetic fixtures require SQLite >= 3.50.4; found " + sqlite3.sqlite_version)


def connect_fixture_database(*args, **kwargs):
    connection = sqlite3.connect(*args, **kwargs)
    try:
        connection.create_function("dc_search_visible", 5, search_visible, deterministic=True)
        schema = connection.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations'").fetchone()
        version = connection.execute("SELECT COALESCE(MAX(version),0) FROM schema_migrations").fetchone()[0] if schema else 0
        if version > 23:
            raise RuntimeError("Synthetic fixture schema is newer than supported schema 23")
        if version >= 20:
            require_secure_delete_runtime()
            option = connection.execute("SELECT v FROM clipboard_fts_config WHERE k='secure-delete'").fetchone()
            if option != (1,):
                raise RuntimeError(f"Schema {version} fixture requires persistent FTS5 secure-delete=1")
            connection.execute("PRAGMA secure_delete=ON")
            if connection.execute("PRAGMA secure_delete").fetchone() != (1,):
                raise RuntimeError(f"Schema {version} fixture requires connection secure_delete=ON")
        if version >= 22:
            state = connection.execute("SELECT state_id,history_revision,privacy_epoch,session_enabled,session_expires_at_ms FROM privacy_runtime_state WHERE singleton=1").fetchone()
            if (state is None or len(state[0]) != 36 or not isinstance(state[1], int) or state[1] < 0
                    or not isinstance(state[2], int) or state[2] < 0 or state[3] not in (0, 1)
                    or (state[4] is not None and (not isinstance(state[4], int) or state[4] < 0))):
                raise RuntimeError("Schema 22 fixture requires valid persistent privacy state")
            expected = {f"revision_{table}_{operation}" for table in ("clipboard_history", "entry_tags", "saved_tags")
                        for operation in ("INSERT", "UPDATE", "DELETE")}
            expected.update(f"privacy_settings_{operation}" for operation in ("INSERT", "UPDATE", "DELETE"))
            if version >= 23:
                expected.add("privacy_record_visibility")
            triggers = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='trigger'")}
            if not expected.issubset(triggers):
                raise RuntimeError("Schema 22 fixture requires history revision and privacy epoch triggers")
    except Exception:
        connection.close()
        raise
    return connection


def mark_legacy_plaintext_sensitive_fixture(connection, entry_id, tag="sensitive"):
    """Model a pre-protection database row solely to test pre-decryption filtering."""
    if tag.strip(_RUST_WHITE_SPACE).translate(_ASCII_LOWER) not in ("sensitive", "密码"):
        raise ValueError("Expected a sensitive fixture tag")
    connection.execute("UPDATE clipboard_history SET tags=? WHERE id=?",
                       (json.dumps([tag], ensure_ascii=False), entry_id))
    connection.execute("INSERT OR IGNORE INTO entry_tags(entry_id,tag) VALUES(?,?)", (entry_id, tag))
