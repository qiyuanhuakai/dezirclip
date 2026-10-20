#!/usr/bin/env python3
"""Private native Wayland protocol matrix. Default is read-only preparation."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import struct
import subprocess
import sys
import time
import zlib

MARKER = "private synthetic clipboard only\n"
TESTS = [
    "wayland::tests::isolated_compositor_clipboard_lifecycle",
    "wayland::tests::isolated_uri_offer_falls_back_to_text_unless_local_files_exist",
    "runtime::tests::isolated_wayland_ports_owned_lifecycle_and_core_preflight",
]
NEGATIVE = "wayland::tests::isolated_negative_capabilities_fail_closed_without_x11_fallback"
DISCONNECT = "wayland::tests::isolated_disconnect_joins_owner_and_subscription"


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def save(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def checked_file(value):
    path = Path(value).resolve(strict=True)
    if not path.is_file():
        raise ValueError("expected a regular input file")
    return path


def private_libraries(value):
    prefix = Path(value).resolve(strict=True)
    if (prefix.parent != Path("/tmp") or not prefix.name.startswith("dezirclip-")
            or not (prefix / "lib").is_dir()
            or (prefix / "dezirclip-isolated-compositor-prefix").read_text() != MARKER):
        raise ValueError("only a marked private /tmp/dezirclip-* compositor prefix may be mounted")
    files = {}
    for path in sorted(prefix.rglob("*")):
        if path.is_file():
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(prefix):
                raise ValueError("private runtime symlink escapes its prefix")
            files["runtime/" + str(path.relative_to(prefix))] = {"path": str(resolved), "sha256": digest(resolved)}
    return prefix, files


def stop_owned(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def inside():
    root = Path("/fixture")
    runtime = root / "runtime"
    assert runtime.joinpath("dezirclip-isolated-wayland-fixture").read_text() == MARKER
    assert "DISPLAY" not in os.environ and "DBUS_SESSION_BUS_ADDRESS" not in os.environ
    assert os.environ["HOME"] == "/home/fixture"
    config = json.loads((root / "plan.json").read_text())
    report = {"status": "running", "started_at": datetime.now(timezone.utc).isoformat(), "functional_only": True, "cases": [],
              "scope": "private user/PID/network/IPC/UTS namespaces; no host clipboard or database"}
    compositor = None
    compositor_log = (root / "compositor.log").open("wb")

    def run(label, command, env, payload=None, timeout=35):
        result = subprocess.run(command, env=env, input=payload, capture_output=True, timeout=timeout)
        (root / (label + ".stdout.log")).write_bytes(result.stdout)
        (root / (label + ".stderr.log")).write_bytes(result.stderr)
        report["cases"].append({"label": label, "command": command, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError(label + " failed; raw logs retained")
        return result.stdout

    def test(label, name, env):
        return run(label, ["/fixture/platform-tests", "--exact", name, "--nocapture"], env)

    try:
        report["compositor_version"] = subprocess.check_output(["/fixture/compositor", "--version"], text=True).strip()
        compositor = subprocess.Popen(["/fixture/compositor", "--unsupported-gpu", "-d", "-c", "/fixture/sway.conf"], stdout=compositor_log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            sockets = [path for path in runtime.glob("wayland-*") if path.is_socket()]
            ipc = [path for path in runtime.glob("sway-ipc.*.sock") if path.is_socket()]
            if sockets and ipc:
                ready = subprocess.run(["/fixture/swaymsg", "-s", str(ipc[0]), "-t", "get_version"], capture_output=True, timeout=2)
                if ready.returncode == 0:
                    (root / "compositor-ready.json").write_bytes(ready.stdout)
                    break
            if compositor.poll() is not None:
                raise RuntimeError("private compositor exited before readiness")
            time.sleep(.05)
        else:
            raise RuntimeError("private compositor readiness timed out")
        env = dict(os.environ, WAYLAND_DISPLAY=sockets[0].name)
        lifecycle = test("lifecycle", TESTS[0], env)
        if ('"protocol":"' + config["expected_protocol"] + '"').encode() not in lifecycle:
            raise RuntimeError("actual negotiated protocol differs from requested matrix")
        for label, name in zip(["uri-fallback", "ports-core"], TESTS[1:]):
            test(label, name, env)
        capabilities = json.loads(run("cli-capabilities", ["/fixture/dzc", "capabilities", "--json"], env))
        assert capabilities["native_wayland"] and capabilities["read"]["availability"]["status"] == "available"
        assert capabilities["wayland"]["connection"]["protocol"] == config["expected_protocol"]
        assert capabilities["auto_paste"]["availability"]["status"] == "unsupported"
        assert capabilities["resolve_source"]["availability"]["status"] == "unsupported"
        report["capabilities"] = capabilities

        def png_chunk(kind, body):
            return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
        png = b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0))
        png += png_chunk(b"IDAT", zlib.compress((b"\0" + b"\xff\x00\x00\xff" * 2) * 2)) + png_chunk(b"IEND", b"")
        synthetic_file = root / "synthetic-file.txt"
        synthetic_file.write_text("synthetic file URI fixture\n")
        payloads = [("text/plain", b"synthetic standalone owner text"),
                    ("text/html", b"<b>synthetic standalone owner HTML</b>"),
                    ("image/png", png), ("text/uri-list", b"file:///fixture/synthetic-file.txt\r\n")]
        for index, (mime, body) in enumerate(payloads):
            label = "cli-mime-" + str(index)
            (root / (label + ".payload")).write_bytes(body)
            run(label + "-write", ["/fixture/dzc", "clipboard", "write", "-", "--mime", mime], env, body)
            # The write process has exited: its explicit owner child must still serve.
            actual = run(label + "-read", ["/fixture/dzc", "clipboard", "read", "--mime", mime], env)
            assert actual == body, label + " exact MIME bytes changed"
            formats = json.loads(run(label + "-formats", ["/fixture/dzc", "clipboard", "formats", "--json"], env))
            assert mime in formats
        for label, additions, removals in [
            ("seat_name", {"DEZIRCLIP_WAYLAND_SEAT": "dezirclip-nonexistent-seat"}, []),
            ("disabled", {"DEZIRCLIP_DISABLE_LINUX_SERVICES": "wayland_clipboard"}, []),
            ("connection", {"WAYLAND_DISPLAY": "wayland-does-not-exist", "DISPLAY": ":65530"}, []),
            ("inherited_fd", {"WAYLAND_SOCKET": "99", "DISPLAY": ":65530"}, ["WAYLAND_DISPLAY"]),
        ]:
            negative_env = dict(env, DEZIRCLIP_EXPECT_WAYLAND_FAILURE=label, **additions)
            for key in removals:
                negative_env.pop(key, None)
            test("negative-" + label, NEGATIVE, negative_env)

        disconnect_env = dict(env, DEZIRCLIP_WAYLAND_DISCONNECT_TEST="1")
        with (root / "disconnect.stdout.log").open("wb") as out, (root / "disconnect.stderr.log").open("wb") as err:
            process = subprocess.Popen(["/fixture/platform-tests", "--exact", DISCONNECT, "--nocapture"], env=disconnect_env, stdout=out, stderr=err)
            try:
                deadline = time.monotonic() + 15
                while not (runtime / "disconnect-ready").is_file():
                    if process.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError("disconnect test did not confirm private owner readiness")
                    time.sleep(.01)
                stop_owned(compositor)
                code = process.wait(timeout=15)
                report["cases"].append({"label": "disconnect", "exit_code": code})
                assert code == 0, "disconnect cleanup failed"
            finally:
                stop_owned(process)

        # Controlled protocol server, generated from official XML. These are not
        # claims about real compositor permission UI or its policy defaults.
        for mode in ["protocol", "seat", "permission"]:
            controlled_runtime = root / ("controlled-" + mode)
            controlled_runtime.mkdir(mode=0o700)
            (controlled_runtime / "dezirclip-isolated-wayland-fixture").write_text(MARKER)
            controlled_env = dict(env, XDG_RUNTIME_DIR=str(controlled_runtime), WAYLAND_DISPLAY="wayland-negative", DEZIRCLIP_EXPECT_WAYLAND_FAILURE=mode)
            with (root / ("controlled-" + mode + ".server.log")).open("wb") as err:
                server = subprocess.Popen(["/fixture/protocol-fixture", mode], env=controlled_env, stdout=subprocess.PIPE, stderr=err)
                try:
                    if not select.select([server.stdout], [], [], 5)[0] or server.stdout.readline() != b"ready\n":
                        raise RuntimeError("controlled protocol server did not become ready")
                    test("negative-" + mode, NEGATIVE, controlled_env)
                finally:
                    stop_owned(server)
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = repr(error)
        raise
    finally:
        stop_owned(compositor)
        compositor_log.close()
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save(root / "report.json", report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=checked_file, required=True)
    parser.add_argument("--cli", type=checked_file, required=True)
    parser.add_argument("--compositor", type=checked_file, required=True)
    parser.add_argument("--swaymsg", type=checked_file, required=True)
    parser.add_argument("--protocol-fixture", type=checked_file, required=True)
    parser.add_argument("--library-prefix", type=Path)
    parser.add_argument("--expected-protocol", choices=["ext", "wlr"], required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    inputs = {name: {"path": str(value), "sha256": digest(value)} for name, value in vars(args).items() if name in ["test_binary", "cli", "compositor", "swaymsg", "protocol_fixture"]}
    inputs["controller"] = {"path": str(Path(__file__).resolve()), "sha256": digest(__file__)}
    prefix = None
    if args.library_prefix:
        prefix, runtime_inputs = private_libraries(args.library_prefix)
        inputs.update(runtime_inputs)
    plan = {"functional_only": True, "expected_protocol": args.expected_protocol, "inputs": inputs,
            "boundary": "no host display/session bus/HOME/database; private bwrap namespaces; synthetic payloads only"}
    if not args.execute:
        print(json.dumps({"status": "prepared_read_only", **plan}, indent=2))
        return
    if sys.platform != "linux" or args.output is None:
        raise ValueError("execution requires Linux and a new --output directory")
    output = args.output.absolute()
    if output.parent.resolve() != Path("/tmp") or not output.name.startswith("dezirclip-") or output.exists():
        raise ValueError("output must be a new private /tmp/dezirclip-* directory")
    output.mkdir(mode=0o700)
    runtime = output / "runtime"
    runtime.mkdir(mode=0o700)
    (runtime / "dezirclip-isolated-wayland-fixture").write_text(MARKER)
    (output / "sway.conf").write_text("xwayland disable\noutput * resolution 1280x720\nseat seat0 fallback true\n")
    shutil.copyfile(__file__, output / "controller.py")
    save(output / "plan.json", plan)
    command = ["/usr/bin/bwrap", "--unshare-all", "--die-with-parent", "--new-session", "--clearenv"]
    for path in ["/usr", "/lib", "/lib64", "/bin"]:
        if Path(path).exists():
            command += ["--ro-bind", path, path]
    command += ["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/home", "--dir", "/home/fixture", "--bind", str(output), "/fixture"]
    for name, target in [("test_binary", "platform-tests"), ("cli", "dzc"), ("compositor", "compositor"), ("swaymsg", "swaymsg"), ("protocol_fixture", "protocol-fixture")]:
        command += ["--ro-bind", str(getattr(args, name)), "/fixture/" + target]
    env = {"PATH": "/usr/bin:/bin", "HOME": "/home/fixture", "XDG_RUNTIME_DIR": "/fixture/runtime", "XDG_SESSION_TYPE": "wayland", "WLR_BACKENDS": "headless", "WLR_RENDERER": "pixman", "WLR_LIBINPUT_NO_DEVICES": "1", "LANG": "C.UTF-8", "DEZIRCLIP_RUN_WAYLAND_SMOKE_TESTS": "1"}
    if prefix:
        command += ["--ro-bind", str(prefix), "/test-libs"]
        env["LD_LIBRARY_PATH"] = "/test-libs/lib"
    for key, value in env.items():
        command += ["--setenv", key, value]
    command += ["--chdir", "/fixture", "--", "/usr/bin/python3", "/fixture/controller.py", "--inside"]
    save(output / "command.json", command)
    try:
        result = subprocess.run(command, capture_output=True, timeout=240)
        (output / "outer.stdout.log").write_bytes(result.stdout)
        (output / "outer.stderr.log").write_bytes(result.stderr)
        code = result.returncode
    except BaseException as error:
        save(output / "outer-failure.json", {"error": repr(error)})
        raise
    finally:
        after = {}
        for name, row in inputs.items():
            try:
                after[name] = {"path": row["path"], "sha256": digest(row["path"])}
            except OSError as error:
                after[name] = {"path": row["path"], "error": type(error).__name__}
        if prefix:
            try:
                _, runtime_after = private_libraries(prefix)
                after.update(runtime_after)
            except (OSError, ValueError) as error:
                after["runtime_recheck_failure"] = type(error).__name__
        save(output / "input-recheck.json", {"before": inputs, "after": after, "unchanged": inputs == after})
    report = json.loads((output / "report.json").read_text()) if (output / "report.json").exists() else {}
    passed = code == 0 and report.get("status") == "passed" and inputs == after
    print(json.dumps({"output": str(output), "passed": passed, "report_sha256": digest(output / "report.json") if report else None}, indent=2))
    if not passed:
        raise SystemExit(1)


if __name__ == "__main__":
    if sys.argv[1:] == ["--inside"]:
        inside()
    else:
        main()
