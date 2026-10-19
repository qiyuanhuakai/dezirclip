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
import sqlite3
import statistics
import subprocess
import tempfile
import time


def sample(binary, output, rows, iterations):
    version = subprocess.check_output([str(binary), "--version"], text=True).strip()
    results = {}
    with tempfile.TemporaryDirectory(prefix="dzc-history-baseline-") as temporary:
        database = Path(temporary) / "history.db"
        environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(database)}
        subprocess.run([str(binary), "add", "synthetic baseline needle 中文🙂\r\n"],
                       env=environment, check=True, capture_output=True)
        with closing(sqlite3.connect(database)) as connection, connection:
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
    output.write_text(json.dumps({
        "schema_version": 1, "measured_at": datetime.now(timezone.utc).isoformat(),
        "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "version": version, "host": platform.platform(), "rows": rows, "iterations": iterations,
        "scope": "CLI process startup + synthetic history query + JSON output", "results": results,
        "limits": ["Use identical build profiles/targets for comparison", "No GUI, capture, memory, or native clipboard coverage"],
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rows", type=int, default=1000)
    parser.add_argument("--iterations", type=int, default=30)
    args = parser.parse_args()
    if args.rows < 50 or args.iterations < 10:
        parser.error("At least 50 rows and 10 measured iterations are required")
    sample(args.binary.resolve(strict=True), args.output.resolve(), args.rows, args.iterations)
