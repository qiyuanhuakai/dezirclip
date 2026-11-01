#!/usr/bin/env python3
"""Exercise bundled OCR and paste helpers against the Platform, without installing."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time


def report_process(label, receipt, primary=None):
    try:
        print(label + " " + json.dumps(receipt, sort_keys=True), file=sys.stderr, flush=True)
    except (OSError, ValueError) as error:
        if primary is None:
            raise
        if hasattr(primary, "add_note"):
            primary.add_note("Process diagnostic output failed: " + str(error))


def wait_xvfb_display(server, read_fd):
    began = time.monotonic()
    deadline = began + 15
    data = bytearray()
    code, primary, status = None, None, "waiting"
    try:
        while True:
            code = server.poll()
            if code is not None:
                raise RuntimeError("Isolated Xvfb exited before displayfd readiness")
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([read_fd], [], [], remaining)[0]:
                code = server.poll()
                raise RuntimeError("Isolated Xvfb did not start within 15 seconds")
            chunk = os.read(read_fd, 32 - len(data))
            data.extend(chunk)
            code = server.poll()
            if code is not None:
                raise RuntimeError("Isolated Xvfb exited while reporting displayfd readiness")
            if time.monotonic() > deadline:
                raise RuntimeError("Isolated Xvfb did not start within 15 seconds")
            if not chunk:
                raise RuntimeError("Isolated Xvfb displayfd closed before a complete ready line")
            if b"\n" in data:
                if not re.fullmatch(rb"[0-9]+\n", data):
                    raise RuntimeError("Isolated Xvfb displayfd ready line is invalid")
                status = "ready"
                return ":" + data[:-1].decode("ascii")
            if len(data) == 32 or not data.isdigit():
                raise RuntimeError("Isolated Xvfb displayfd ready line is invalid or exceeds 32 bytes")
    except BaseException as error:
        primary, status = error, "failed"
        raise
    finally:
        report_process("Xvfb startup", {"pid": server.pid, "poll": code, "status": status,
            "elapsed_seconds": time.monotonic() - began, "ready_budget_seconds": 15,
            "displayfd_bytes": len(data), "displayfd_hex": data.hex(), "stderr_inherited": True}, primary)


@contextmanager
def original_process(process, label):
    primary = None
    try:
        yield process
    except BaseException as error:
        primary = error
        raise
    finally:
        began = time.monotonic()
        errors, forced, code = [], False, None
        try:
            code = process.poll()
        except BaseException as error:
            errors.append("poll: " + repr(error))
        if code is None:
            try:
                process.terminate()
            except BaseException as error:
                errors.append("terminate: " + repr(error))
            try:
                code = process.wait(timeout=10)
            except BaseException as error:
                errors.append("wait after terminate: " + repr(error))
                forced = True
                try:
                    process.kill()
                except BaseException as kill_error:
                    errors.append("kill: " + repr(kill_error))
                try:
                    code = process.wait(timeout=10)
                except BaseException as wait_error:
                    errors.append("wait after kill: " + repr(wait_error))
        for name in ("stdin", "stdout", "stderr"):
            stream = getattr(process, name, None)
            if stream is not None:
                try:
                    stream.close()
                except BaseException as error:
                    errors.append(name + " close: " + repr(error))
        if forced:
            errors.append("Original child required force kill")
        receipt = {"label": label, "pid": process.pid, "exit_code": code,
            "exit_confirmed": code is not None, "forced_kill": forced, "errors": errors,
            "elapsed_seconds": time.monotonic() - began}
        report_process("Process cleanup", receipt, primary)
        if errors and primary is None:
            raise RuntimeError("Original " + label + " cleanup failed: " + "; ".join(errors))


def read_line(process, timeout=15):
    if not select.select([process.stdout], [], [], timeout)[0]:
        raise RuntimeError("X11 test process did not become ready")
    line = process.stdout.readline().strip()
    if not line:
        raise RuntimeError("X11 test process exited before becoming ready")
    return line


def verify(build_dir):
    fixtures = Path(__file__).resolve().parent / "tests" / "fixtures"
    base = [
        "flatpak", "build", "--runtime", "--readonly", "--unshare=network",
        f"--bind-mount=/run/dezirclip-tests={fixtures}", str(build_dir),
    ]

    def run(command, **kwargs):
        return subprocess.run(
            base + command, check=True, text=True, capture_output=True,
            timeout=45, **kwargs,
        ).stdout

    for resource in ("dzc-cli/SKILL.md", "dezirclip-theme/SKILL.md",
                     "dezirclip-theme/assets/forest.theme.json"):
        run(["test", "-s", f"/app/share/dezirclip/skills/{resource}"])
    print("PASS bundled CLI and theme Skills", flush=True)

    # --runtime uses org.gnome.Platform, so SDK-only shared libraries cannot mask gaps.
    run(["/app/bin/xdotool", "version"])
    languages = run(["/app/bin/tesseract", "--list-langs"])
    assert {"eng", "chi_sim", "osd"} <= set(languages.splitlines()), languages
    for language, fixture, expected in (
        ("eng", "ocr-eng.png", "DEZIRCLIP12345"),
        ("chi_sim", "ocr-chi-sim.png", "中文测试"),
    ):
        text = run([
            "/app/bin/tesseract", f"/run/dezirclip-tests/{fixture}",
            "stdout", "-l", language, "--psm", "7",
        ])
        assert expected in "".join(text.split()), (language, text)
        print(f"PASS bundled OCR: {language}", flush=True)

    with tempfile.TemporaryDirectory(prefix="dezirclip-flatpak-test-") as temp_dir:
        target = Path(temp_dir) / "paste-target"
        subprocess.run([
            "cc", "-Wall", "-Wextra", "-Werror",
            str(fixtures / "x11-paste-target.c"), "-lX11", "-o", str(target),
        ], check=True)
        read_fd, write_fd = os.pipe()
        try:
            with original_process(subprocess.Popen([
                "Xvfb", "-displayfd", str(write_fd), "-screen", "0", "640x480x24",
                "-ac", "-nolisten", "tcp",
            ], pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=None), "Xvfb") as server:
                os.close(write_fd)
                write_fd = None
                display = wait_xvfb_display(server, read_fd)
                env = dict(os.environ, DISPLAY=display)
                with original_process(subprocess.Popen(
                    [str(target)], env=env, text=True,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                ), "paste receiver") as receiver:
                    assert read_line(receiver) == "READY"
                    command = base[:-1] + [
                        "--share=ipc", "--socket=x11", f"--env=DISPLAY={display}",
                        str(build_dir), "/app/bin/xdotool",
                    ]
                    subprocess.run(command + ["keyup", "Control_L", "Control_R", "Shift_L", "Shift_R", "Alt_L", "Alt_R", "Super_L", "Super_R", "Meta_L", "Meta_R"],
                                   env=env, check=True, timeout=15, capture_output=True)
                    subprocess.run(command + ["key", "ctrl+v"],
                                   env=env, check=True, timeout=15, capture_output=True)
                    output, error = receiver.communicate(timeout=15)
                    assert receiver.returncode == 0, (output, error)
                    assert "PASS Ctrl+V clipboard roundtrip" in output, output
                    print(output.strip(), flush=True)
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)
    print("PASS Flatpak helpers on org.gnome.Platform", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build_dir", type=Path)
    args = parser.parse_args()
    verify(args.build_dir.resolve())
