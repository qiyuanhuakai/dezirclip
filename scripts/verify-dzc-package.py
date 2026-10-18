#!/usr/bin/env python3
"""Extract a dzc archive safely and verify real persistent database behavior."""

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sqlite3
import stat
import subprocess
import sys
import tarfile
import tempfile
import uuid
import zipfile

ARCHIVE_NAME = re.compile(r"^dzc-(.+)-(windows|linux)-(x86_64|aarch64)\.(zip|tar\.gz)$")


def verify_checksum(archive):
    archive = Path(archive)
    sidecar = Path(str(archive) + ".sha256")
    line = sidecar.read_text(encoding="ascii").rstrip("\r\n")
    match = re.fullmatch(r"([0-9a-fA-F]{64})  (.+)", line)
    if not match or match.group(2) != archive.name:
        raise ValueError("SHA-256 sidecar must name exactly this archive")
    with archive.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != match.group(1).lower():
        raise ValueError("archive SHA-256 checksum mismatch")


def destination(root, name):
    if "\\" in name or "\0" in name or ":" in name or name.startswith("/"):
        raise ValueError(f"unsafe archive member: {name!r}")
    parts = PurePosixPath(name).parts
    if not parts or any(part in (".", "..") for part in name.split("/")):
        raise ValueError(f"unsafe archive member: {name!r}")
    path = root.joinpath(*parts)
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"archive member escapes extraction directory: {name!r}")
    return path


def extract_archive(archive, root):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    if str(archive).endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            for member in source.infolist():
                path = destination(root, member.filename)
                mode = member.external_attr >> 16
                if stat.S_ISLNK(mode):
                    raise ValueError("archive symlinks are not allowed")
                if member.is_dir():
                    path.mkdir(parents=True, exist_ok=True)
                    continue
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.open(member) as data, path.open("wb") as output:
                    shutil.copyfileobj(data, output)
                path.chmod(mode & 0o777 or 0o644)
    else:
        with tarfile.open(archive, "r:gz") as source:
            for member in source:
                path = destination(root, member.name)
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                    continue
                if not member.isfile():
                    raise ValueError("archive links and special files are not allowed")
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as data, path.open("wb") as output:
                    shutil.copyfileobj(data, output)
                path.chmod(member.mode & 0o777 or 0o644)


def verify(archive, expected_version=None):
    archive = Path(archive).resolve()
    match = ARCHIVE_NAME.fullmatch(archive.name)
    if not match:
        raise ValueError("archive name must be dzc-VERSION-{windows,linux}-{x86_64,aarch64}.zip or .tar.gz")
    version, platform, _, _ = match.groups()
    if expected_version and expected_version.removeprefix("v") != version:
        raise ValueError("requested version does not match archive name")
    verify_checksum(archive)
    with tempfile.TemporaryDirectory(prefix="dzc-package-verify-") as temporary:
        base = Path(temporary)
        extracted = base / "distribution"
        extract_archive(archive, extracted)
        binary = extracted / ("dzc.exe" if platform == "windows" else "dzc")
        for required in (binary, extracted / "LICENSE", extracted / "README.md",
                         extracted / "docs" / "cli.md", extracted / "integrations" / "dzc-integrations.py",
                         extracted / "skills" / "dzc-cli" / "SKILL.md"):
            if not required.is_file():
                raise ValueError(f"missing packaged resource: {required.relative_to(extracted)}")
        unrelated = base / "other-working-directory"
        unrelated.mkdir()
        environment = os.environ.copy()
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "TIEZ_DB_PATH"):
            environment.pop(key, None)
        environment["DEZIRCLIP_DB_PATH"] = str(base / "original.db")

        def run(arguments, payload=None, env=environment, expect_success=True):
            result = subprocess.run([str(binary), *arguments], input=payload, capture_output=True,
                                    cwd=unrelated, env=env, timeout=30, check=False)
            if expect_success and result.returncode:
                raise ValueError(f"dzc {' '.join(arguments[:2])} failed: {result.stderr.decode('utf-8', errors='replace')}")
            if not expect_success and result.returncode == 0:
                raise ValueError(f"dzc unexpectedly accepted {' '.join(arguments[:2])}")
            return result.stdout

        if run(["--version"]).decode().strip() != f"dzc {version}":
            raise ValueError("binary version does not match the archive version")

        # Native entrypoints must run from the extracted binary without finding
        # Python, an adapter checkout, or any other executable through PATH.
        empty_path = base / "no-external-tools"
        empty_path.mkdir()
        native_environment = {key: value for key, value in environment.items()
                              if key.casefold() != "path"}
        native_environment["PATH"] = str(empty_path)
        lazy_database = base / "native-handshake-must-not-open.db"
        handshake_environment = dict(native_environment, DEZIRCLIP_DB_PATH=str(lazy_database))
        for shell in ("bash", "zsh", "fish", "pwsh", "nu"):
            script = run([shell], env=handshake_environment).decode("utf-8")
            if not script.strip() or "__DZC_EXECUTABLE__" in script \
                    or "dzc-integrations.py" in script or "DZC_PYTHON" in script:
                raise ValueError(f"{shell} did not emit a self-contained native widget")

        def mcp(requests, env):
            frames = [
                {"jsonrpc": "2.0", "id": "initialize", "method": "initialize", "params": {
                    "protocolVersion": "2025-11-25", "capabilities": {},
                    "clientInfo": {"name": "package-verifier", "version": "1"}}},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                *requests,
            ]
            payload = b"\n".join(json.dumps(frame).encode("utf-8") for frame in frames) + b"\n"
            response = [json.loads(line) for line in run(["mcp"], payload, env=env).splitlines()]
            if len(response) != len(requests) + 1 or any(
                    frame.get("jsonrpc") != "2.0" or "error" in frame or "result" not in frame
                    for frame in response):
                raise ValueError("native MCP did not emit valid request responses")
            expected_ids = ["initialize", *(frame["id"] for frame in requests)]
            if [frame.get("id") for frame in response] != expected_ids:
                raise ValueError("native MCP returned unexpected response IDs")
            if response[0]["result"].get("serverInfo", {}).get("version") != version:
                raise ValueError("native MCP server version does not match the archive")
            return [frame["result"] for frame in response[1:]]

        result = mcp([{"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}],
                     handshake_environment)[0]
        tools = result.get("tools", [])
        if {tool.get("name") for tool in tools} != {"clipboard_list", "clipboard_search", "clipboard_get"} \
                or len(tools) != 3 or any(
                    tool.get("annotations", {}).get("readOnlyHint") is not True
                    or tool.get("inputSchema", {}).get("additionalProperties") is not False
                    for tool in tools):
            raise ValueError("native MCP did not expose exactly three read-only tools")
        if lazy_database.exists():
            raise ValueError("Shell generation or MCP handshake unexpectedly opened history")

        if json.loads(run(["count", "--json"])) != {"count": 0}:
            raise ValueError("isolated package verification must start with an empty history")
        marker = f"  package-{uuid.uuid4().hex} 你好\r\nsecond line\r\n".encode("utf-8")
        added = run(["add", "-"], marker).decode().strip()
        identifier = re.fullmatch(r"Added: id=(\d+)", added)
        if not identifier:
            raise ValueError("add did not return a persistent entry ID")
        entry_id = identifier.group(1)
        if run(["get", entry_id, "--raw"]) != marker:
            raise ValueError("separate-process get did not preserve original UTF-8/CRLF bytes")
        run(["pin", entry_id, "--quiet"])
        run(["tag", "add", entry_id, "package-verified"])
        edited = marker + b"edited\n\n"
        run(["edit", entry_id, "-", "--quiet"], edited)
        rows = json.loads(run(["list", "--json", "--", "-1"]))

        def assert_entry(rows):
            if len(rows) != 1 or rows[0].get("id") != int(entry_id) \
                    or rows[0].get("content") != edited.decode("utf-8") \
                    or not rows[0].get("pinned") or rows[0].get("tags") != ["package-verified"]:
                raise ValueError("history did not persist the edited content, ID, pin and tag")

        assert_entry(rows)
        calls = [
            {"jsonrpc": "2.0", "id": "list", "method": "tools/call", "params": {
                "name": "clipboard_list", "arguments": {"limit": 1}}},
            {"jsonrpc": "2.0", "id": "get", "method": "tools/call", "params": {
                "name": "clipboard_get", "arguments": {"id": int(entry_id), "max_chars": 16384}}},
        ]
        results = mcp(calls, native_environment)
        if any(result.get("isError") is not False for result in results):
            raise ValueError("native MCP could not query persisted history without external tools")
        metadata = json.loads(results[0]["content"][0]["text"])["entries"]
        content = json.loads(results[1]["content"][0]["text"])
        if len(metadata) != 1 or metadata[0].get("id") != int(entry_id) or "content" in metadata[0] \
                or content.get("content") != edited.decode("utf-8") or content.get("truncated") is not False:
            raise ValueError("native MCP lost persisted content or leaked payload into metadata")
        backup = base / "backup.json"
        run(["export", str(backup), "--quiet"])
        imported_environment = dict(environment, DEZIRCLIP_DB_PATH=str(base / "imported.db"))
        run(["import", str(backup), "--mode", "replace", "--quiet"], env=imported_environment)
        assert_entry(json.loads(run(["list", "--json", "--", "-1"], env=imported_environment)))
        if run(["get", entry_id, "--raw"], env=imported_environment) != edited:
            raise ValueError("backup roundtrip changed raw content")
        future = json.loads(backup.read_bytes())
        future["version"] = "dezirclip-unsupported-future-version"
        backup.write_text(json.dumps(future), encoding="utf-8")
        run(["import", str(backup), "--mode", "replace", "--quiet"], env=imported_environment,
            expect_success=False)
        assert_entry(json.loads(run(["list", "--json", "--", "-1"], env=imported_environment)))
        future_database = base / "future-schema.db"
        with closing(sqlite3.connect(future_database)) as connection:
            connection.executescript("""
                CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
                INSERT INTO schema_migrations VALUES(99);
                CREATE TABLE preserved(content TEXT);
                INSERT INTO preserved VALUES('preserve future history');
            """)
            connection.commit()
        before = future_database.read_bytes()
        future_environment = dict(environment, DEZIRCLIP_DB_PATH=str(future_database))
        run(["count", "--json"], env=future_environment, expect_success=False)
        if future_database.read_bytes() != before:
            raise ValueError("rejecting a future database schema modified the database file")
        with closing(sqlite3.connect(future_database)) as connection:
            if connection.execute("SELECT MAX(version) FROM schema_migrations").fetchone() != (99,) \
                    or connection.execute("SELECT content FROM preserved").fetchone() != ("preserve future history",):
                raise ValueError("rejecting a future database schema changed existing rows")
    return archive


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--version")
    args = parser.parse_args(argv)
    try:
        print(f"Verified {verify(args.archive, args.version).name}")
        return 0
    except (OSError, ValueError, subprocess.TimeoutExpired, zipfile.BadZipFile, tarfile.TarError) as error:
        print(f"dzc package verification: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
