"""Sample an isolated clipboard application and its complete process tree.

Requires Windows and psutil. UI actions are performed separately. This script
only observes windows; it never injects input or trims process working sets.
"""

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import sqlite3
import statistics
import subprocess
import time

import psutil


def prepare(args):
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    executable = directory / args.executable.name
    shutil.copy2(args.executable, executable)
    data = directory / "data"
    data.mkdir()
    settings = {
        "app.hotkey": "Ctrl+Alt+Shift+F9",
        "app.quick_paste_hotkey": "Ctrl+Alt+Shift+F10",
        "app.screenshot_hotkey": "Ctrl+Alt+Shift+F11",
        "app.plain_paste_hotkey": "",
        "app.search_hotkey": "",
        "app.silent_start": "false",
        "app.auto_start": "false",
        "app.sequential_mode": "false",
        "app.window_pinned": "false",
        "app.edge_docking": "false",
        "app.window_width": "400",
        "app.window_height": "600",
        "app.theme": "retro",
        "app.color_mode": "light",
        "app.idle_destroy_enabled": str(args.idle).lower(),
        "app.idle_destroy_seconds": "60",
        "app.light_mode_enabled": str(args.idle).lower(),
        "app.light_mode_idle_minutes": "1",
    }
    with sqlite3.connect(data / "clipboard.db") as db:
        db.execute("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        db.executemany("INSERT INTO settings VALUES (?, ?)", settings.items())
    manifest = {
        "kind": args.kind,
        "executable": str(executable),
        "sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "settings": settings,
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
    }
    (directory / "case.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(json.dumps(manifest, indent=2))


def visible_windows(pid):
    user32 = ctypes.windll.user32
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    found = []

    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, title, len(title))
            if title.value in ("DezirClip", "TieZ"):
                found.append(title.value)
        return True

    user32.EnumWindows(visit, 0)
    return found


def run(args):
    directory = args.directory.resolve()
    case = json.loads((directory / "case.json").read_text(encoding="utf-8"))
    result_path = directory / args.output
    if result_path.exists():
        raise SystemExit(f"Refusing to overwrite {result_path}")
    environment = dict(os.environ)
    environment["WEBVIEW2_USER_DATA_FOLDER"] = str(directory / "webview-profile")
    environment["APPDATA"] = str(directory / "roaming")
    environment["LOCALAPPDATA"] = str(directory / "local")
    if args.debug_port:
        with socket.socket() as check:
            check.bind(("127.0.0.1", args.debug_port))
        inherited = environment.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
        if "--remote-debugging" in inherited:
            raise SystemExit("An inherited debugging endpoint conflicts with the test endpoint")
        environment["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
            f"{inherited} --remote-debugging-port={args.debug_port} --remote-debugging-address=127.0.0.1"
        ).strip()
    start = time.perf_counter()
    log = (directory / (args.output + ".stderr.log")).open("wb")
    process = subprocess.Popen([case["executable"]], cwd=directory, env=environment,
                               stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    root = psutil.Process(process.pid)
    print(json.dumps({"pid": process.pid, "case": str(directory)}), flush=True)
    observations = []
    events = []
    previous_visible = False
    next_sample = 0.0
    previous_key = False
    cpu_by_pid = {}
    tracked = {(root.pid, root.create_time()): root}
    metadata = {"case": case, "host": platform.platform(), "cpu": platform.processor(),
                "logical_cpus": psutil.cpu_count(), "ram_bytes": psutil.virtual_memory().total,
                "sample_seconds": args.interval, "pid": process.pid,
                "debug_port": args.debug_port,
                "started_at": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    fixture_path = directory / "history-fixture.json"
    if fixture_path.exists():
        metadata["fixture"] = json.loads(fixture_path.read_text(encoding="utf-8"))
    try:
        while process.poll() is None and time.perf_counter() - start < args.seconds:
            elapsed = time.perf_counter() - start
            visible = bool(visible_windows(process.pid))
            if visible != previous_visible:
                events.append({"seconds": round(elapsed, 4), "event": "shown" if visible else "hidden"})
                previous_visible = visible
            # Observe only the dedicated test hotkey; no other keys are read.
            key = bool(ctypes.windll.user32.GetAsyncKeyState(0x78) & 0x8001)
            if key and not previous_key:
                events.append({"seconds": round(elapsed, 4), "event": "F9-down"})
            previous_key = key
            if elapsed >= next_sample:
                members = []
                for parent in list(tracked.values()):
                    try:
                        for member in parent.children(recursive=True):
                            tracked[(member.pid, member.create_time())] = member
                    except (psutil.NoSuchProcess, psutil.AccessDenied):
                        continue
                for identity, member in list(tracked.items()):
                    try:
                        if not member.is_running():
                            del tracked[identity]
                            continue
                        memory = member.memory_info()
                        cpu = member.cpu_times()
                        cpu_by_pid[identity] = cpu.user + cpu.system
                        members.append({"pid": member.pid, "name": member.name(),
                                        "rss_bytes": memory.rss, "private_bytes": memory.private,
                                        "cpu_seconds": cpu.user + cpu.system})
                    except (psutil.NoSuchProcess, psutil.AccessDenied):
                        continue
                observations.append({"seconds": round(elapsed, 4), "visible": visible,
                                     "rss_bytes": sum(p["rss_bytes"] for p in members),
                                     "private_bytes": sum(p["private_bytes"] for p in members),
                                     "cpu_seconds": sum(cpu_by_pid.values()), "processes": members})
                next_sample = elapsed + args.interval
                result_path.write_text(json.dumps({**metadata, "events": events,
                                                   "samples": observations}, indent=2), encoding="utf-8")
            if (directory / "stop-sampling").exists():
                break
            time.sleep(0.01)
    finally:
        # Stop only the exact process tree started by this invocation.
        descendants = list(tracked.values())
        try:
            descendants.extend(root.children(recursive=True))
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
        for child in {p.pid: p for p in descendants}.values():
            try:
                if child.is_running():
                    child.terminate()
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass
        psutil.wait_procs(descendants, timeout=5)
        log.close()
        result_path.write_text(json.dumps({**metadata, "events": events,
                                           "samples": observations}, indent=2), encoding="utf-8")
        print(json.dumps({"result": str(result_path), "events": events,
                          "samples": len(observations)}), flush=True)


def summarize(args):
    result = json.loads(args.result.read_text(encoding="utf-8"))
    rows = [r for r in result["samples"] if args.start <= r["seconds"] <= args.end]
    if not rows:
        raise SystemExit("No samples in requested interval")
    print(json.dumps({"count": len(rows), "start": rows[0]["seconds"], "end": rows[-1]["seconds"],
                      "median_rss_mib": statistics.median(r["rss_bytes"] for r in rows) / 2**20,
                      "median_private_mib": statistics.median(r["private_bytes"] for r in rows) / 2**20,
                      "cpu_seconds": rows[-1]["cpu_seconds"] - rows[0]["cpu_seconds"],
                      "process_counts": sorted({len(r["processes"]) for r in rows}),
                      "events": result["events"]}, indent=2))


parser = argparse.ArgumentParser(description=__doc__)
subparsers = parser.add_subparsers(dest="command", required=True)
prep = subparsers.add_parser("prepare")
prep.add_argument("executable", type=Path)
prep.add_argument("directory", type=Path)
prep.add_argument("--kind", choices=["upstream", "fork"], required=True)
prep.add_argument("--idle", action="store_true")
prep.set_defaults(func=prepare)
sample = subparsers.add_parser("run")
sample.add_argument("directory", type=Path)
sample.add_argument("--seconds", type=float, default=240)
sample.add_argument("--interval", type=float, default=0.5)
sample.add_argument("--output", default="samples.json")
sample.add_argument("--debug-port", type=int, default=0,
                    help="Optional localhost CDP endpoint; keep 0 for memory comparisons")
sample.set_defaults(func=run)
summary = subparsers.add_parser("summary")
summary.add_argument("result", type=Path)
summary.add_argument("--start", type=float, default=0)
summary.add_argument("--end", type=float, default=float("inf"))
summary.set_defaults(func=summarize)
arguments = parser.parse_args()
if os.name != "nt":
    raise SystemExit("This benchmark requires Windows")
arguments.func(arguments)
