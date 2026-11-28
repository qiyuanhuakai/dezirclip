"""Run selected Rust Platform tests using only private X11 and synthetic input."""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import secrets
import select
import struct
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
CLIPBOARD = "DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS"
SHORTCUT = "DEZIRCLIP_RUN_LINUX_SHORTCUT_SMOKE_TESTS"
X11 = (
    "capture::echo::tests::isolated_x11_written_payloads_match_reader_echo_and_only_explicit_copy_is_skipped",
    "capture::listener::tests::isolated_x11_monitor_probe_negotiates_without_a_worker",
    "capture::reader::tests::isolated_x11_file_images_recapture_after_delete_text_and_another_image",
    "capture::reader::tests::isolated_x11_reader_keeps_text_html_and_file_priority",
    "capture::subscription::tests::isolated_x11_subscription_stops_and_joins",
    "capture::x11_clipboard::tests::linux_clipboard_owner_keeps_image_available_after_setter_returns",
    "capture::x11_clipboard::tests::linux_clipboard_owner_keeps_text_available_after_setter_returns",
    "capture::x11_privacy::tests::isolated_x11_metadata_is_bounded_and_same_owner_replacement_revokes_the_snapshot",
    "runtime::tests::isolated_x11_private_owner_retains_files_and_releases_only_its_selection",
    "runtime::x11::tests::isolated_x11_ports_probe_source_and_explicit_paste",
    "shortcut::x11::tests::isolated_x11_shortcuts_conflict_repeat_release_and_re_register",
)
SHORTCUT_NEGATIVES = {
    "shortcut::x11::tests::isolated_disabled_shortcuts_never_connect_or_register": {
        "DEZIRCLIP_DISABLE_LINUX_SERVICES": "global_shortcut",
    },
    "shortcut::x11::tests::isolated_wayland_shortcuts_are_unsupported_before_x11_access": {
        "XDG_SESSION_TYPE": "wayland",
    },
}
SETUP_CANCEL = "capture::subscription::tests::isolated_pending_wayland_setup_cancels_as_a_successful_stop"
XFIXES_RESTART = "capture::subscription::tests::isolated_established_xfixes_loss_falls_back_and_recovers_after_server_restart"
XVFB_ARGS = "-screen 0 1024x768x24 -noreset -nolisten tcp"


def workspace_tests():
    command = [
        "cargo", "test", "--locked", "--workspace", "--all-targets",
        "--no-default-features", "--features", "dzc-cli/native-screen,dzc-service/native-screen",
        "--message-format=json-render-diagnostics",
    ]
    source = ROOT / "crates/dzc-platform/src/lib.rs"
    executables = []
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True) as cargo:
        for line in cargo.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                print(line, end="", flush=True)
                continue
            if not isinstance(message, dict) or "reason" not in message:
                print(line, end="", flush=True)
                continue
            if message.get("reason") == "compiler-message":
                print(message["message"]["rendered"] or "", end="", flush=True)
            if (message.get("reason") == "compiler-artifact"
                    and message.get("profile", {}).get("test")
                    and Path(message["target"]["src_path"]).resolve() == source
                    and message.get("executable")):
                executables.append(Path(message["executable"]).resolve(strict=True))
        if code := cargo.wait():
            raise SystemExit(code)
    if len(executables) != 1:
        raise ValueError("Expected exactly one Platform unit-test executable from this workspace run")
    return executables[0]


def report_case(name, stdout, stderr, code):
    print(stdout, end="", flush=True)
    print(stderr, end="", file=sys.stderr, flush=True)
    if code or f"test {name} ... ok" not in stdout.splitlines():
        raise ValueError(f"Required native case did not execute: {name}")


def test_command(binary, name):
    return [str(binary), "--ignored", "--exact", name]


def run_exact(binary, name, env, x11=False):
    command = test_command(binary, name)
    if x11:
        command = ["xvfb-run", "-a", "-s", XVFB_ARGS, "dbus-run-session", "--", *command]
    result = subprocess.run(command, env=env, timeout=30, text=True, capture_output=True)
    report_case(name, result.stdout, result.stderr, result.returncode)


def stop_owned(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


def xauthority(path, display, cookie):
    # FamilyWild with the private display number and a fresh MIT-MAGIC-COOKIE-1.
    fields = [b"", display.encode("ascii"), b"MIT-MAGIC-COOKIE-1", cookie]
    entry = struct.pack(">H", 65535) + b"".join(struct.pack(">H", len(field)) + field for field in fields)
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "wb") as stream:
        stream.write(entry)


@contextmanager
def owned_xvfb(authority, cookie, env, display=""):
    xauthority(authority, display, cookie)
    with tempfile.TemporaryFile() as log:
        reader, writer = os.pipe()
        command = ["Xvfb", *([":" + display] if display else []), "-displayfd", str(writer),
                   "-auth", str(authority), *XVFB_ARGS.split()]
        server = None
        try:
            try:
                server = subprocess.Popen(command, env=env, pass_fds=(writer,), stderr=log)
            finally:
                os.close(writer)
            if not select.select([reader], [], [], 5)[0]:
                raise TimeoutError("Private Xvfb did not report its display")
            ready = os.read(reader, 32).decode("ascii").strip()
            if not ready.isascii() or not ready.isdecimal() or (display and ready != display) or server.poll() is not None:
                raise ValueError("Private Xvfb did not claim the expected display")
            xauthority(authority, ready, cookie)
            yield ready, server
        except BaseException:
            log.seek(0)
            print(log.read().decode(errors="replace"), end="", file=sys.stderr, flush=True)
            raise
        finally:
            os.close(reader)
            if server is not None:
                stop_owned(server)


def verify_xfixes_restart(binary, runtime, env):
    authority = runtime / "Xauthority"
    cookie = secrets.token_bytes(16)
    command = test_command(binary, XFIXES_RESTART)
    child = None
    with owned_xvfb(authority, cookie, env) as (display, original):
        options = {**env, "DISPLAY": ":" + display, "XAUTHORITY": str(authority),
                   "DEZIRCLIP_RUN_XFIXES_RESTART_FIXTURE": "1"}
        try:
            child = subprocess.Popen(command, env=options, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            deadline = time.monotonic() + 7
            while not (runtime / "p94-xfixes-ready").is_file():
                if child.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError("XFixes fixture did not confirm readiness")
                time.sleep(.02)
            stop_owned(original)
            with owned_xvfb(authority, cookie, env, display):
                (runtime / "p94-x11-restarted").touch()
                stdout, stderr = child.communicate(timeout=12)
                report_case(XFIXES_RESTART, stdout, stderr, child.returncode)
        except BaseException:
            if child is not None:
                stop_owned(child)
                stdout, stderr = child.communicate()
                print(stdout, end="", flush=True)
                print(stderr, end="", file=sys.stderr, flush=True)
            raise


def exercise(binary, source_revision=None):
    with tempfile.TemporaryDirectory(prefix="dezirclip-linux-platform-") as directory:
        runtime = Path(directory)
        (runtime / "dezirclip-isolated-wayland-fixture").write_text("private synthetic clipboard only\n", encoding="utf-8")
        (runtime / "p94-private-native-fixture").write_text("synthetic-p94-native-only\n", encoding="utf-8")
        env = {"PATH": os.environ["PATH"], "HOME": directory, "XDG_RUNTIME_DIR": directory,
               "TMPDIR": directory, "LANG": "C.UTF-8", "XDG_SESSION_TYPE": "x11"}
        for name in X11:
            run_exact(binary, name, {**env, CLIPBOARD: "1", SHORTCUT: "1"}, x11=True)
        if Path("/tmp/.X11-unix/X65530").exists():
            raise RuntimeError("The shortcut negative control needs a disconnected display")
        for name, options in SHORTCUT_NEGATIVES.items():
            run_exact(binary, name, {**env, SHORTCUT: "1", "DISPLAY": ":65530", **options})
        run_exact(binary, SETUP_CANCEL, {
            **env, "XDG_SESSION_TYPE": "wayland", "WAYLAND_DISPLAY": "wayland-p94-setup",
            "DEZIRCLIP_RUN_STARTUP_CANCEL_FIXTURE": "1",
        })
        verify_xfixes_restart(binary, runtime, env)
    with binary.open("rb") as executable:
        print(json.dumps({
            "result_kind": "isolated_linux_platform",
            "artifact_sha256": hashlib.file_digest(executable, "sha256").hexdigest(),
            "source_revision": source_revision,
            "executed": [*X11, *SHORTCUT_NEGATIVES, SETUP_CANCEL, XFIXES_RESTART],
        }))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--workspace", action="store_true", help="Run workspace tests and reuse the compiled Platform test executable")
    mode.add_argument("--binary", type=Path, help="Run only the isolated tests with a local Platform test executable")
    args = parser.parse_args()
    if args.workspace:
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        exercise(workspace_tests(), revision)
    else:
        exercise(args.binary.resolve(strict=True))
