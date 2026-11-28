"""Build the native test companions and verify subsequent Cargo operations."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def build_companions(target):
    target_args = ["--target", target] if target else []
    subprocess.run(["cargo", "build", "--locked", "-p", "dzc-cli", "-p", "dzc-service",
                    "--no-default-features", "--features", "dzc-cli/native-screen,dzc-service/native-screen",
                    "--bin", "dzc", "--bin", "dzc-service", *target_args], cwd=ROOT, check=True)


def executable_digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fingerprints(target_directory, suffix):
    return {name: executable_digest(target_directory / (name + suffix))
            for name in ("dzc", "dzc-service")}


def protect_companions(command, target, stdout=None):
    directory = ROOT / "target" / target / "debug"
    suffix = ".exe" if os.name == "nt" else ""
    before = fingerprints(directory, suffix)
    subprocess.run(command, cwd=ROOT, check=True, stdout=stdout)
    after = fingerprints(directory, suffix)
    changed = [name for name in before if before[name] != after[name]]
    if changed:
        raise RuntimeError(f"Desktop build replaced the tested standalone companions: {', '.join(changed)}")


def binary_from_test(lines, expected, source, feature):
    normal_binary = None
    for line in lines:
        if not line.startswith('{"reason":'):
            print(line, end="")
            continue
        message = json.loads(line)
        if (message["reason"] == "compiler-artifact" and message["target"]["name"] == expected.stem
                and message["target"]["kind"] == ["bin"] and message["profile"]["test"] is False
                and Path(message["target"]["src_path"]).resolve() == source
                and feature in message["features"]
                and message["executable"] and Path(message["executable"]).resolve() == expected):
            normal_binary = expected
    return normal_binary


def test_windows_desktop():
    if os.name != "nt":
        raise RuntimeError("The embedded frontend fixture requires a native Windows test")
    executable = (ROOT / "target/debug/dezirclip.exe").resolve()
    command = ["cargo", "test", "--locked", "--manifest-path", "src-tauri/Cargo.toml", "-p", "dezirclip",
               "--all-targets", "--features", "tauri/custom-protocol", "--message-format=json-render-diagnostics"]
    # Cargo builds the normal binary when preparing the backup integration test.
    # Keep its compiler record to rule out a stale executable or a test harness.
    with tempfile.TemporaryFile(mode="w+t", encoding="utf-8", errors="replace") as log:
        try:
            protect_companions(command, "", stdout=log)
        finally:
            log.seek(0)
            desktop = binary_from_test(log, executable, (ROOT / "src-tauri/src/main.rs").resolve(), "desktop")
    if desktop is None:
        raise RuntimeError("The Tauri test invocation did not produce the embedded desktop")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        print(f"desktop_sha256={executable_digest(desktop)}", file=output)


def test_windows_workspace():
    if os.name != "nt":
        raise RuntimeError("The workspace CLI fixture requires a native Windows test")
    executable = (ROOT / "target/debug/dzc.exe").resolve()
    command = ["cargo", "test", "--locked", "--workspace", "--all-targets", "--no-default-features",
               "--features", "dzc-cli/native-screen,dzc-service/native-screen",
               "--message-format=json-render-diagnostics"]
    with tempfile.TemporaryFile(mode="w+t", encoding="utf-8", errors="replace") as log:
        try:
            subprocess.run(command, cwd=ROOT, stdout=log, check=True)
        finally:
            log.seek(0)
            cli = binary_from_test(log, executable, (ROOT / "crates/dzc-cli/src/main.rs").resolve(), "native-screen")
    if cli is None:
        raise RuntimeError("The workspace test invocation did not produce the normal dzc executable")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        print(f"cli_path={cli}", file=output)
        print(f"cli_sha256={executable_digest(cli)}", file=output)


if __name__ == "__main__":
    target = os.environ.get("DZC_COMPANION_TARGET", "")
    if sys.argv[1:] == ["build"]:
        build_companions(target)
    elif sys.argv[1:] == ["test-windows-desktop"]:
        test_windows_desktop()
    elif sys.argv[1:] == ["test-windows-workspace"]:
        test_windows_workspace()
    elif len(sys.argv) > 2 and sys.argv[1] == "protect":
        protect_companions(sys.argv[2:], target)
    else:
        sys.exit("Expected build, test-windows-desktop, test-windows-workspace, or protect <command>")
