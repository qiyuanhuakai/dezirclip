#!/usr/bin/env python3
"""Build the test-only failure server from installed official protocol XML."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import sys


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--xml", type=Path, default=Path("/usr/share/wayland-protocols/staging/ext-data-control/ext-data-control-v1.xml"))
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    source = Path(__file__).resolve().parent / "native-wayland-fixture/main.c"
    xml = args.xml.resolve(strict=True)
    inputs = {"source": {"path": str(source), "sha256": sha(source)}, "xml": {"path": str(xml), "sha256": sha(xml)}}
    if not args.execute:
        print(json.dumps({"status": "prepared_read_only", "inputs": inputs}, indent=2))
        return
    output = args.output.absolute()
    if sys.platform != "linux" or output.parent.resolve() != Path("/tmp") or not output.name.startswith("dezirclip-") or output.exists():
        raise ValueError("execution needs a new private /tmp/dezirclip-* output")
    output.mkdir(mode=0o700)
    report = {"inputs": inputs, "commands": [], "functional_only": True, "status": "running"}
    def run(command):
        result = subprocess.run(command, capture_output=True, check=False)
        index = len(report["commands"])
        (output / (str(index) + ".stdout.log")).write_bytes(result.stdout)
        (output / (str(index) + ".stderr.log")).write_bytes(result.stderr)
        report["commands"].append({"args": command, "exit_code": result.returncode})
        result.check_returncode()
        return result.stdout.decode()
    try:
        report["compiler_version"] = run(["/usr/bin/cc", "--version"])
        report["scanner_version"] = run(["/usr/bin/wayland-scanner", "--version"])
        report["server_version"] = run(["/usr/bin/pkg-config", "--modversion", "wayland-server"]).strip()
        flags = shlex.split(run(["/usr/bin/pkg-config", "--cflags", "--libs", "wayland-server"]))
        run(["/usr/bin/wayland-scanner", "server-header", str(xml), str(output / "ext-data-control-server.h")])
        run(["/usr/bin/wayland-scanner", "private-code", str(xml), str(output / "ext-data-control-protocol.c")])
        run(["/usr/bin/cc", "-Wall", "-Wextra", "-Werror", "-I" + str(output), str(source), str(output / "ext-data-control-protocol.c"), "-o", str(output / "protocol-fixture"), *flags])
        report["binary_sha256"] = sha(output / "protocol-fixture")
        report["status"] = "passed"
    finally:
        report["inputs_unchanged"] = all(sha(row["path"]) == row["sha256"] for row in inputs.values())
        if not report["inputs_unchanged"]:
            report["status"] = "failed"
        (output / "build.json").write_text(json.dumps(report, indent=2) + "\n")
    if report["status"] != "passed":
        raise SystemExit(1)
    print(json.dumps({"output": str(output), "status": report["status"], "proof_sha256": sha(output / "build.json")}, indent=2))


if __name__ == "__main__":
    main()
