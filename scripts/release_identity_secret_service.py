"""Disposable private Secret Service QA; never uses keyutils or user credentials."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--helper", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    helper = Path(args.helper).resolve(strict=True)
    helper_sha = digest(helper)
    parent = Path("/q")
    parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(parent, 0o700)
    root = Path(tempfile.mkdtemp(prefix="dezirclip-private-ss-", dir=parent))
    marker = root / ".dzc-private-ss-qa-v1"
    marker.write_text("synthetic-fixture-only\n")
    marker.chmod(0o600)
    environment = os.environ.copy()
    for name in ("DBUS_SESSION_BUS_ADDRESS", "DBUS_SESSION_BUS_PID", "DBUS_SYSTEM_BUS_ADDRESS", "GNOME_KEYRING_CONTROL", "SSH_AUTH_SOCK", "GIT_DIR", "GIT_WORK_TREE"):
        environment.pop(name, None)
    for key, relative in (("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
        target = root / relative
        target.mkdir(mode=0o700)
        environment[key] = str(target)
    runtime = root / "runtime"
    control = runtime / "keyring"
    control.mkdir(mode=0o700)
    address = "unix:path=" + str(runtime / "bus")
    environment["DBUS_SESSION_BUS_ADDRESS"] = address
    environment["GNOME_KEYRING_CONTROL"] = str(control)
    environment["DZC_QA_FIXTURE_ROOT"] = str(root)
    processes = []
    logs = []
    record = {"status": "running", "private_root": str(root), "helper": str(helper), "helper_sha256": helper_sha, "backend": "explicit_secret_service", "keyutils_constructed": False, "system_keyring_isolation_claimed": False, "steps": []}
    failed = None
    try:
        for name, command in (("dbus", ["/usr/bin/dbus-daemon", "--session", "--nofork", "--nopidfile", "--address=" + address]), ("keyring", ["/usr/bin/gnome-keyring-daemon", "--foreground", "--unlock", "--components=secrets", "--control-directory=" + str(control)])):
            if name == "keyring":
                deadline = time.monotonic() + 5
                while not (runtime / "bus").is_socket():
                    if time.monotonic() > deadline or processes[0].poll() is not None:
                        raise RuntimeError("PrivateBusStartupFailed")
                    time.sleep(0.05)
                os.chmod(runtime / "bus", 0o600)
            stdout = (output / (name + ".stdout.log")).open("xb")
            stderr = (output / (name + ".stderr.log")).open("xb")
            logs.extend((stdout, stderr))
            process = subprocess.Popen(command, env=environment, stdin=subprocess.PIPE, stdout=stdout, stderr=stderr, start_new_session=True, umask=0o077)
            processes.append(process)
            if name == "keyring":
                process.stdin.write(b"fixed-disposable-qa-keyring-password\n")
            process.stdin.close()
        deadline = time.monotonic() + 8
        while True:
            probe = subprocess.run(["/usr/bin/gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus", "--method", "org.freedesktop.DBus.NameHasOwner", "org.freedesktop.secrets"], env=environment, capture_output=True, timeout=2)
            if probe.returncode == 0 and probe.stdout.strip() == b"(true,)":
                break
            if time.monotonic() > deadline or processes[-1].poll() is not None:
                raise RuntimeError("PrivateSecretServiceStartupFailed")
            time.sleep(0.05)
        for operation, expected in (("seed", 0), ("check", 0), ("seed", 1)):
            result = subprocess.run([str(helper), operation, str(root)], env=environment, capture_output=True, timeout=15)
            item = {"operation": operation, "exit_code": result.returncode, "stdout": result.stdout.decode(), "stderr": result.stderr.decode()}
            record["steps"].append(item)
            if result.returncode != expected:
                raise RuntimeError("PrivatePreseedAssertionFailed")
            if expected and json.loads(result.stderr)["reason"] != "ExistingCredentialRefused":
                raise RuntimeError("PrivatePreseedNegativeCauseMismatch")
        if args.command:
            command = args.command[1:] if args.command[0] == "--" else args.command
            with (output / "command.stdout.log").open("xb") as stdout, (output / "command.stderr.log").open("xb") as stderr:
                result = subprocess.run(command, env=environment, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, timeout=1200)
            record["command"] = command
            record["command_exit_code"] = result.returncode
            if result.returncode:
                raise RuntimeError("SyntheticCommandFailed")
        record["status"] = "passed"
    except Exception as error:
        record["status"] = "failed"
        record["failure"] = type(error).__name__ + ": " + str(error)
        failed = error
    finally:
        record["processes"] = []
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
            try:
                code = process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                code = process.wait(timeout=5)
            record["processes"].append({"pid": process.pid, "exit_code": code, "stopped": process.poll() is not None})
        for log in logs:
            log.close()
        record["helper_sha256_after"] = digest(helper)
        record["helper_unchanged"] = helper_sha == record["helper_sha256_after"]
        if not record["helper_unchanged"]:
            record["status"] = "failed"
        (output / "proof.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps({"status": record["status"], "proof": str(output / "proof.json")}))
    if failed or record["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
