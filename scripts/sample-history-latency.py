"""Sample real CLI history operations against a disposable synthetic database.

Run the same command with pre/post binaries built using the same target/profile.
This measures process startup plus database/query/output; it is not a capture,
GUI, memory, native clipboard, or key-store benchmark.
"""
import argparse
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
from sqlite_history_fixture import connect_fixture_database
import statistics
import subprocess
import tempfile
import time


def compare_reports(baseline, current):
    for field in ["host", "machine_id", "rows", "iterations", "scope", "build_profile", "target", "fixture_version"]:
        if field not in baseline or baseline[field] != current.get(field):
            raise ValueError(f"Cannot compare different or missing {field}")
    for field in ["payload_bytes", "fixture_sha256"]:
        if field in baseline or field in current:
            if field not in baseline or baseline[field] != current.get(field):
                raise ValueError(f"Cannot compare different or missing {field}")
    if baseline["build_profile"] == "unspecified" or not baseline["target"]:
        raise ValueError("Comparison requires an explicit build profile and target")
    if baseline["iterations"] < 30:
        raise ValueError("Comparison requires at least 30 measured iterations")
    if not baseline.get("source_revision") or not current.get("source_revision"):
        raise ValueError("Comparison requires source revisions for both binaries")
    if baseline["results"].keys() != current["results"].keys():
        raise ValueError("Cannot compare different operation sets")
    comparisons = {}
    for operation, reference in baseline["results"].items():
        limit = reference["p95_ms"] + max(reference["p95_ms"] * .10, 20.0)
        observed = current["results"][operation]["p95_ms"]
        comparisons[operation] = {"baseline_p95_ms": reference["p95_ms"],
                                  "current_p95_ms": observed, "allowed_p95_ms": limit,
                                  "regression_ms": observed - reference["p95_ms"],
                                  "passed": observed <= limit}
    return {"rule": "p95 <= baseline p95 + max(baseline p95 * 10%, 20ms)",
            "scope": current["scope"], "operations": comparisons,
            "passed": all(item["passed"] for item in comparisons.values())}


def sample(binary, output, rows, iterations, source_revision=None, build_profile="unspecified", target="", baseline=None):
    version = subprocess.check_output([str(binary), "--version"], text=True).strip()
    results = {}
    with tempfile.TemporaryDirectory(prefix="dzc-history-baseline-") as temporary:
        database = Path(temporary) / "history.db"
        environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(database)}
        subprocess.run([str(binary), "add", "synthetic baseline needle 中文🙂\r\n"],
                       env=environment, check=True, capture_output=True)
        with closing(connect_fixture_database(database)) as connection, connection:
            cursor = connection.execute("SELECT * FROM clipboard_history LIMIT 1")
            first = dict(zip([column[0] for column in cursor.description], cursor.fetchone()))
            first.pop("id")
            columns = list(first)
            sql = "INSERT INTO clipboard_history (" + ",".join(columns) + ") VALUES (" + ",".join("?" for _ in columns) + ")"
            for index in range(1, rows):
                item = dict(first)
                item["content"] = f"synthetic baseline needle {index} 中文🙂\r\n" + "sample " * 80
                item["preview"] = f"synthetic baseline needle {index}"
                item["timestamp"] = index + 1
                connection.execute(sql, [item[column] for column in columns])
        for name, arguments in {
            "list_metadata": ["list", "50", "--json", "--metadata"],
            "search": ["search", "needle", "--json", "--limit", "50"],
        }.items():
            elapsed = []
            for index in range(iterations + 5):
                start = time.perf_counter_ns()
                result = subprocess.run([str(binary), *arguments], env=environment, check=True, capture_output=True)
                duration = (time.perf_counter_ns() - start) / 1_000_000
                json.loads(result.stdout)
                if index >= 5:
                    elapsed.append(duration)
            ordered = sorted(elapsed)
            results[name] = {"p50_ms": statistics.median(elapsed),
                             "p95_ms": ordered[math.ceil(len(ordered) * .95) - 1],
                             "samples_ms": elapsed}
    output.parent.mkdir(parents=True, exist_ok=True)
    report = {
        "schema_version": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
        "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "version": version, "host": platform.platform(), "rows": rows, "iterations": iterations,
        "machine_id": hashlib.sha256(f"{platform.node()}|{platform.machine()}".encode()).hexdigest(),
        "fixture_version": 1, "source_revision": source_revision,
        "build_profile": build_profile, "target": target,
        "scope": "CLI process startup + synthetic history query + JSON output", "results": results,
        "limits": ["Use identical build profiles/targets for comparison", "No GUI, capture, memory, or native clipboard coverage"],
    }
    if baseline:
        report["comparison"] = compare_reports(json.loads(baseline.read_text(encoding="utf-8")), report)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rows", type=int, default=1000)
    parser.add_argument("--iterations", type=int, default=30)
    parser.add_argument("--source-revision")
    parser.add_argument("--build-profile", choices=["debug", "release"], default="unspecified")
    parser.add_argument("--target", default="")
    parser.add_argument("--baseline", type=Path, help="Comparable JSON sample; requires >=30 measured iterations")
    args = parser.parse_args()
    if args.rows < 50 or args.iterations < 10:
        parser.error("At least 50 rows and 10 measured iterations are required")
    report = sample(args.binary.resolve(strict=True), args.output.resolve(), args.rows, args.iterations,
                    args.source_revision, args.build_profile, args.target, args.baseline)
    if report.get("comparison", {}).get("passed") is False:
        raise SystemExit(1)
