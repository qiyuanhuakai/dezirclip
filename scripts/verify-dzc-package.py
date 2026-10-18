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
