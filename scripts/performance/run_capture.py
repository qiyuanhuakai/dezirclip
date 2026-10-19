"""Run a frozen capture adapter in a quiet sampling window."""
import argparse
from datetime import datetime, timezone
import json
import hashlib
from pathlib import Path
import platform
import subprocess

from prepare_capture import sha256


def run(args):
    preparation = args.preparation.resolve(strict=True)
    proof = json.loads(preparation.read_text(encoding="utf-8"))
    if not proof["eligible_for_timing"] or proof["candidate"]["dirty"]:
        raise ValueError("Timing requires a frozen clean candidate build, not a dirty functional check")
    if hashlib.sha256(f"{platform.node()}|{platform.machine()}".encode()).hexdigest() != proof["machine_id"]:
        raise ValueError("Sampling machine differs from the prepared build host")
    adapter = proof["adapters"][args.variant]
    binary = Path(adapter["binary"]).resolve(strict=True)
    if sha256(binary) != adapter["binary_sha256"]:
        raise ValueError("Prepared binary no longer matches its build proof")
    directory = preparation.parent
    output = args.output.resolve()
    if not output.is_relative_to(directory) or output.exists():
        raise ValueError("Use a new report inside the prepared artifact directory")
    raw = directory / f"raw-{args.variant}-{args.rows}-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%f')}.json"
    command = [str(binary), str(directory / "cases" / args.variant), str(args.rows), str(args.iterations), str(raw)]
    subprocess.run(command, check=True)
    report = json.loads(raw.read_text(encoding="utf-8"))
    if report["verification_only"] or report["iterations"] != args.iterations:
        raise ValueError("Adapter returned a functional check instead of the requested timing run")
    report["build"] = {"variant": args.variant, "binary_sha256": adapter["binary_sha256"],
                       "source": proof["baseline"] if args.variant == "legacy" else proof["candidate"],
                       "profile": proof["profile"], "target": proof["target"], "rustc": proof["rustc"],
                       "cargo": proof["cargo"], "environment": proof["build_environment"],
                       "template_sha256": proof["template_sha256"], "machine_id": proof["machine_id"]}
    report["run"] = {"measured_at": datetime.now(timezone.utc).isoformat(), "host": platform.platform(),
                     "round": args.round, "command": command, "preparation_sha256": sha256(preparation),
                     "raw_report_sha256": sha256(raw), "raw_report": raw.name}
    report["run"]["runner_sha256"] = sha256(Path(__file__).resolve())
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--preparation", type=Path, required=True)
    parser.add_argument("--variant", choices=["legacy", "core", "engine"], required=True)
    parser.add_argument("--rows", type=int, choices=[1000, 10000], required=True)
    parser.add_argument("--iterations", type=int, default=50)
    parser.add_argument("--round", required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    if arguments.iterations < 30:
        parser.error("Use at least 30 measured iterations")
    try:
        run(arguments)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Cannot sample capture adapter: {error}\n")
