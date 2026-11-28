"""Build the native test companions and verify subsequent Cargo operations."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def build_companions(target):
    target_args = ["--target", target] if target else []
    subprocess.run(["cargo", "build", "--locked", "-p", "dzc-cli", "-p", "dzc-service",
                    "--no-default-features", "--features", "dzc-cli/native-screen,dzc-service/native-screen",
                    "--bin", "dzc", "--bin", "dzc-service", *target_args], cwd=ROOT, check=True)


def fingerprints(target_directory, suffix):
    return {name: hashlib.sha256((target_directory / (name + suffix)).read_bytes()).digest()
            for name in ("dzc", "dzc-service")}


def protect_companions(command, target):
    directory = ROOT / "target" / target / "debug"
    suffix = ".exe" if os.name == "nt" else ""
    before = fingerprints(directory, suffix)
    subprocess.run(command, cwd=ROOT, check=True)
    after = fingerprints(directory, suffix)
    changed = [name for name in before if before[name] != after[name]]
    if changed:
        raise RuntimeError(f"Desktop build replaced the tested standalone companions: {', '.join(changed)}")


if __name__ == "__main__":
    target = os.environ.get("DZC_COMPANION_TARGET", "")
    if sys.argv[1:] == ["build"]:
        build_companions(target)
    elif len(sys.argv) > 2 and sys.argv[1] == "protect":
        protect_companions(sys.argv[2:], target)
    else:
        sys.exit("Expected build or protect <command>")
