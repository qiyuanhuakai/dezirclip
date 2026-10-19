"""Time real CLI add/tag requests on cloned, disposable synthetic history.

This includes startup and commit, excludes fixture copy/verification, and does
not measure GUI capture. Old tag-only behavior is reported separately from
the stronger protected-conversion postconditions of newer binaries.
"""
import argparse
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import time

SPEC = importlib.util.spec_from_file_location("history_latency", Path(__file__).with_name("sample-history-latency.py"))
HISTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HISTORY)


def synthetic_payload(size):
    prefix = "合成性能样本🙂\r\n".encode()
    unit = b"synthetic clipboard input text\r\n"
    length = size - len(prefix)
    if length < 0:
        raise ValueError("payload is smaller than its UTF-8 prefix")
    return prefix + (unit * (length // len(unit) + 1))[:length]


def fixture(binary, directory, rows):
    database = directory / "template.db"
    environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(database)}
    subprocess.run([str(binary), "add", "synthetic history seed"], env=environment,
                   check=True, capture_output=True)
    digest = hashlib.sha256()
    with closing(sqlite3.connect(database)) as connection, connection:
        cursor = connection.execute("SELECT * FROM clipboard_history LIMIT 1")
        seed = dict(zip([column[0] for column in cursor.description], cursor.fetchone()))
        seed.pop("id")
        columns = list(seed)
        statement = "INSERT INTO clipboard_history (" + ",".join(columns) + ") VALUES (" + ",".join("?" for _ in columns) + ")"
        for index in range(1, rows):
            item = dict(seed)
            item["content"] = f"synthetic neutral history row {index} needle " + "sample " * 40
            item["preview"] = f"synthetic neutral history row {index}"
            item["timestamp"] = index + 1
            connection.execute(statement, [item[column] for column in columns])
            digest.update(item["content"].encode())
    with closing(sqlite3.connect(database)) as connection:
        connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        assert connection.execute("SELECT COUNT(*) FROM clipboard_history").fetchone()[0] == rows
    return database, digest.hexdigest()


def verify(database, operation, rows, payload):
    with closing(sqlite3.connect(database)) as connection:
        if operation.startswith("add_"):
            stored = connection.execute("SELECT content FROM clipboard_history ORDER BY id DESC LIMIT 1").fetchone()[0]
            assert stored.encode() == payload, "add must preserve the complete UTF-8/CRLF payload"
            assert connection.execute("SELECT COUNT(*) FROM clipboard_history").fetchone()[0] == rows + 1
            return {"complete_payload": True, "row_count": rows + 1}
        content, preview, tags = connection.execute(
            "SELECT content,preview,tags FROM clipboard_history WHERE id=?", [rows // 2]).fetchone()
        assert "sensitive" in json.loads(tags), "tag request did not apply"
        protected = lambda value: value.startswith(("dpapi:", "linux:"))
        in_fts = connection.execute(
            "SELECT COUNT(*) FROM clipboard_fts WHERE rowid=? AND clipboard_fts MATCH 'needle'",
            [rows // 2]).fetchone()[0] > 0
        return {"tag_written": True, "content_encrypted": protected(content),
                "preview_encrypted": protected(preview),
                "sensitive_payload_in_fts": in_fts,
                "security_note": "Tag latency alone does not prove protected conversion; inspect encrypted fields"}


def sample(binary, output, rows, iterations, payload_bytes, revision, build_profile, target, baseline=None, require_protected=False):
    if output.exists():
        raise ValueError(f"Preserve existing sample artifact: {output}")
    results = {}
    payload = synthetic_payload(payload_bytes)
    with tempfile.TemporaryDirectory(prefix="dzc-write-baseline-") as temporary:
        directory = Path(temporary)
        template, fixture_hash = fixture(binary, directory, rows)
        payload_file = directory / "payload.txt"
        payload_file.write_bytes(payload)
        for operation, arguments, standard_input in [
            ("add_stdin", ["add", "-"], payload),
            ("add_file", ["add", "@" + str(payload_file)], None),
            ("tag_sensitive_request", ["tag", "add", str(rows // 2), "sensitive"], None),
        ]:
            samples = []
            observed = []
            for index in range(iterations + 5):
                case = directory / f"{operation}-{index}"
                case.mkdir()
                database = case / "history.db"
                shutil.copyfile(template, database)
                environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(database)}
                start = time.perf_counter_ns()
                completed = subprocess.run([str(binary), *arguments], input=standard_input,
                                           env=environment, check=True, capture_output=True)
                elapsed = (time.perf_counter_ns() - start) / 1_000_000
                if operation.startswith("add_"):
                    assert re.search(rb"Added: id=\d+", completed.stdout), "unexpected add response"
                postcondition = verify(database, operation, rows, payload)
                if index >= 5:
                    samples.append(elapsed)
                    if postcondition not in observed:
                        observed.append(postcondition)
            results[operation] = {"p50_ms": statistics.median(samples),
                                  "p95_ms": sorted(samples)[math.ceil(len(samples) * .95) - 1],
                                  "samples_ms": samples, "postconditions": observed}
    report = {"schema_version": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
              "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
              "host": platform.platform(),
              "machine_id": hashlib.sha256(f"{platform.node()}|{platform.machine()}".encode()).hexdigest(),
              "rows": rows, "iterations": iterations, "fixture_version": 1,
              "fixture_sha256": fixture_hash, "payload_bytes": len(payload),
              "payload_sha256": hashlib.sha256(payload).hexdigest(), "source_revision": revision,
              "build_profile": build_profile, "target": target,
              "scope": "CLI process startup + add/tag request + database commit; no GUI capture",
              "results": results,
              "limits": ["Fixture copy and postcondition checks are outside the measured interval",
                         "Old CLI tag request may only write tags; this is not equivalent protected-conversion completion",
                         "No reader, capture-engine, event, queue-wait, or GUI timing"]}
    if baseline:
        report["comparison"] = HISTORY.compare_reports(json.loads(baseline.read_text(encoding="utf-8")), report)
    if require_protected:
        report["protected_conversion_passed"] = all(
            item["content_encrypted"] and item["preview_encrypted"] and not item["sensitive_payload_in_fts"]
            for item in results["tag_sensitive_request"]["postconditions"])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rows", type=int, choices=[1000, 10000], required=True)
    parser.add_argument("--iterations", type=int, default=50)
    parser.add_argument("--payload-bytes", type=int, default=1024 * 1024)
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--build-profile", choices=["debug", "release"], required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--require-protected-conversion", action="store_true",
                        help="Fail if tag completion leaves plaintext fields or searchable sensitive payload")
    args = parser.parse_args()
    if args.iterations < 30 or args.payload_bytes < 1024:
        parser.error("Use at least 30 iterations and 1024 payload bytes")
    result = sample(args.binary.resolve(strict=True), args.output.resolve(), args.rows, args.iterations,
                    args.payload_bytes, args.source_revision, args.build_profile, args.target, args.baseline,
                    args.require_protected_conversion)
    if result.get("comparison", {}).get("passed") is False or result.get("protected_conversion_passed") is False:
        raise SystemExit(1)
