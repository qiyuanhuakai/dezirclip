#!/usr/bin/env python3
"""Exercise bundled OCR and paste helpers against the Platform, without installing."""

import argparse
import os
from pathlib import Path
import select
import subprocess
import tempfile


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
            with subprocess.Popen([
                "Xvfb", "-displayfd", str(write_fd), "-screen", "0", "640x480x24",
                "-ac", "-nolisten", "tcp",
            ], pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE) as server:
                os.close(write_fd)
                write_fd = None
                try:
                    if not select.select([read_fd], [], [], 15)[0]:
                        raise RuntimeError("Isolated Xvfb did not start")
                    display = ":" + os.read(read_fd, 32).decode("ascii").strip()
                    env = dict(os.environ, DISPLAY=display)
                    with subprocess.Popen(
                        [str(target)], env=env, text=True,
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    ) as receiver:
                        try:
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
                            if receiver.poll() is None:
                                receiver.kill()
                                receiver.wait()
                finally:
                    server.terminate()
                    server.wait(timeout=10)
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
