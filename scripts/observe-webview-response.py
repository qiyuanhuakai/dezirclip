"""Observe an already running benchmark through a dedicated localhost CDP port.

Requires psutil and Node.js 22+. This tool launches three Node WebSocket
transports, never a desktop application, and sends only read-only
Runtime.evaluate commands. It does not send keyboard input or change the DOM.
Prepare/migrate/seed the case first, start this observer, wait for its ready
message, then launch the matching application with the selected debugging port.

Events include target/document changes, content readiness, visibility and paint
entry changes, plus timestamped F9/F10/F11 handlers and lifecycle log messages.
The application log uses UTC. Log timestamps and CDP receive times are distinct;
polling/evaluation add measurement delay. No memory result from this debugger
session should be mixed with the separate debugger-free memory benchmark.
Example: python scripts/observe-webview-response.py .harness/perf/cases/baseline
  --port 9223 --node /absolute/path/to/node.exe --seconds 600
"""

import argparse
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import shutil
import sys
import threading
import time
from urllib.parse import parse_qs, urlsplit
from urllib.request import ProxyHandler, build_opener


PROBE_PATH = Path(__file__).resolve().with_name("probe-webview-startup.py")
SPEC = importlib.util.spec_from_file_location("webview_startup_probe", PROBE_PATH)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)
LABELS = ("main", "quick-paste", "region-select")
LOG_LINE = re.compile(r"^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\] (.*)$")
HOTKEY = re.compile(r"\bkey:\s*(F9|F10|F11)\b")
HOTKEY_LABELS = {"F9": "main", "F10": "quick-paste", "F11": "region-select"}


def expression(label, marker):
    if label == "main":
        body = PROBE.readiness_expression(marker)
    elif label == "quick-paste":
        body = """(() => {
          const list = document.querySelector('.quick-paste-window__list');
          const rows = list ? list.querySelectorAll('[data-testid="quick-paste-item"]').length : 0;
          const textPresent = !!list && list.textContent.includes(MARKER);
          return {ready:rows > 0 && textPresent, row_count:rows, text_present:textPresent};
        })()""".replace("MARKER", json.dumps(marker))
    else:
        body = """(() => {
          const overlay = document.querySelector('[data-testid="region-select-overlay"]');
          const box = overlay ? overlay.getBoundingClientRect() : null;
          return {ready:!!box && box.width > 0 && box.height > 0,
            overlay_width:box ? box.width : 0, overlay_height:box ? box.height : 0};
        })()"""
    return """(() => {
      const state = BODY;
      const paints = performance.getEntriesByType('paint');
      const firstPaint = paints.find(entry => entry.name === 'first-paint');
      const fcp = paints.find(entry => entry.name === 'first-contentful-paint');
      return {...state, visibility_state:document.visibilityState,
        document_state:document.readyState, time_origin_ms:performance.timeOrigin,
        application_instance:new URLSearchParams(location.search).get('instance'),
        inner_width:innerWidth, inner_height:innerHeight, device_pixel_ratio:devicePixelRatio,
        performance_now_ms:performance.now(),
        first_paint_start_ms:firstPaint ? firstPaint.startTime : null,
        fcp_start_ms:fcp ? fcp.startTime : null};
    })()""".replace("BODY", body)


class Clock:
    def __init__(self):
        self.perf = time.perf_counter()
        self.wall = time.time()

    def event(self, kind, at=None, **fields):
        now_perf = time.perf_counter()
        now_wall = time.time()
        at = now_perf if at is None else at
        # Align each observation with the application's wall-clock logger even
        # when Windows gradually corrects its clock during a long idle test.
        epoch = now_wall + at - now_perf
        return {"event": kind, "elapsed_seconds": at - self.perf,
                "epoch_ms": epoch * 1000,
                "utc": datetime.fromtimestamp(epoch, timezone.utc).isoformat(timespec="milliseconds"),
                **fields}


def targets_by_label(targets, port):
    selected = {}
    for target in targets:
        url = urlsplit(target.get("url", ""))
        release = (url.scheme == "tauri" and url.hostname == "localhost") or (
            url.scheme in ("http", "https") and url.hostname == "tauri.localhost")
        if target.get("type") != "page" or not release:
            continue
        values = parse_qs(url.query).get("window")
        label = "main" if values is None else values[0] if len(values) == 1 else None
        if label not in LABELS:
            continue
        socket = urlsplit(target.get("webSocketDebuggerUrl", ""))
        if socket.scheme != "ws" or socket.hostname not in ("localhost", "127.0.0.1", "::1") or socket.port != port:
            raise ValueError("Observed target does not belong to the dedicated localhost endpoint")
        if label in selected:
            raise ValueError(f"Multiple {label} targets on the dedicated endpoint")
        selected[label] = target
    return selected


class Monitor:
    def __init__(self, label, transport, clock, events, stop, interval, marker, geometry):
        self.label = label
        self.transport = transport
        self.clock = clock
        self.events = events
        self.stop = stop
        self.interval = interval
        self.expression = expression(label, marker)
        self.geometry = geometry
        self.target = None
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.observe, daemon=True)

    def assign(self, target):
        with self.lock:
            self.target = target

    def current(self):
        with self.lock:
            return self.target

    def observe(self):
        connected_id = None
        document_target_id = None
        generation = 0
        origin = None
        previous = None
        last_error = None
        while not self.stop.is_set():
            target = self.current()
            target_id = target.get("id") if target else None
            try:
                if connected_id != target_id:
                    if connected_id is not None:
                        self.transport.disconnect()
                    connected_id = None
                    if target is not None:
                        self.transport.connect(target["webSocketDebuggerUrl"])
                        connected_id = target_id
                        if document_target_id != target_id:
                            generation += 1
                            previous = None
                            origin = None
                            document_target_id = target_id
                        self.events.put(self.clock.event("cdp_connected", label=self.label,
                                                        target_id=target_id, document_generation=generation))
                if connected_id is not None:
                    received, state = self.transport.evaluate(self.expression)
                    if self.current() is not target:
                        continue
                    self.geometry.update(self.label, state)
                    new_origin = state.get("time_origin_ms")
                    if origin is not None and new_origin != origin:
                        generation += 1
                        previous = None
                    if new_origin != origin:
                        self.events.put(self.clock.event("document_observed", at=received, label=self.label,
                            target_id=target_id, document_generation=generation, time_origin_ms=new_origin,
                            application_instance=state.get("application_instance")))
                    origin = new_origin
                    boundary = (bool(state.get("ready")), state.get("visibility_state"),
                                state.get("first_paint_start_ms"), state.get("fcp_start_ms"))
                    if boundary != previous:
                        changes = []
                        if previous is None or boundary[0] != previous[0]:
                            changes.append("readiness")
                        if previous is None or boundary[1] != previous[1]:
                            changes.append("visibility")
                        if previous is None or boundary[2:] != previous[2:]:
                            changes.append("paint_entries")
                        self.events.put(self.clock.event("state_changed", at=received, label=self.label,
                            target_id=target_id, document_generation=generation, changes=changes, state=state))
                        previous = boundary
                    last_error = None
            except PROBE.CDP_ERRORS as error:
                message = type(error).__name__ + ": " + str(error)[:180]
                if message != last_error:
                    self.events.put(self.clock.event("cdp_error", label=self.label,
                                                    target_id=target_id, message=message))
                    last_error = message
                try:
                    self.transport.disconnect()
                except PROBE.CDP_ERRORS:
                    pass
                connected_id = None
            self.stop.wait(self.interval)
        try:
            self.transport.disconnect()
        except PROBE.CDP_ERRORS:
            pass


class LogTail:
    def __init__(self, path, clock, from_start=False):
        self.path = path
        self.clock = clock
        self.offset = 0 if from_start or not path.exists() else path.stat().st_size
        self.pending = b""

    def read(self):
        if not self.path.exists():
            return []
        if self.path.stat().st_size < self.offset:
            self.offset = 0
            self.pending = b""
        with self.path.open("rb") as stream:
            stream.seek(self.offset)
            content = stream.read(256 * 1024)
            self.offset = stream.tell()
        lines = (self.pending + content).split(b"\n")
        self.pending = lines.pop()
        events = []
        for raw in lines:
            match = LOG_LINE.match(raw.decode("utf-8", errors="replace").rstrip("\r"))
            if not match:
                continue
            raw_time, message = match.groups()
            at = datetime.strptime(raw_time, "%Y-%m-%d %H:%M:%S.%f").replace(tzinfo=timezone.utc)
            fields = {"log_utc": at.isoformat(timespec="milliseconds"),
                      "log_epoch_ms": at.timestamp() * 1000,
                      "log_elapsed_seconds": at.timestamp() - self.clock.wall, "message": message}
            if "[global-shortcut] Handler invoked:" in message:
                hotkey = HOTKEY.search(message)
                if hotkey:
                    events.append(self.clock.event("shortcut_handler", key=hotkey[1],
                                                   label=HOTKEY_LABELS[hotkey[1]], **fields))
            elif any(token in message for token in (
                "[STARTUP]", "first painted frame", "paint wait", "first paint",
                "Destroying main webview", "recreated", "quick-paste-show", "region-select-show",
                "startup-show", "toggle-show", "hide-main", "quick-paste-hide", "region-select-hide",
                "auxiliary-prewarm", "[auxiliary-windows]",
            )):
                events.append(self.clock.event("application_log", **fields))
        return events


class Geometry:
    def __init__(self):
        self.values = {}
        self.lock = threading.Lock()

    def update(self, label, state):
        with self.lock:
            self.values[label] = (state.get("inner_width", 0) * state.get("device_pixel_ratio", 1),
                                  state.get("inner_height", 0) * state.get("device_pixel_ratio", 1))

    def classify(self, title, width, height):
        if title in ("DezirClip", "TieZ"):
            return "main"
        if title:
            return "aux-unknown"
        with self.lock:
            quick = self.values.get("quick-paste")
            region = self.values.get("region-select")
        close = lambda size: size and abs(width - size[0]) <= 4 and abs(height - size[1]) <= 4
        if close(quick) or any(close((480 * scale, 360 * scale)) for scale in (1, 1.25, 1.5, 1.75, 2)):
            return "quick-paste"
        if close(region) or (width >= 1000 and height >= 800):
            return "region-select"
        return "aux-unknown"


def native_windows(pid, geometry):
    user32 = ctypes.windll.user32
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    found = {}

    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid:
            return True
        class_name = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, class_name, len(class_name))
        if class_name.value != "Tauri Window":
            return True
        title = ctypes.create_unicode_buffer(256)
        rectangle = wintypes.RECT()
        user32.GetWindowTextW(hwnd, title, len(title))
        if not user32.GetWindowRect(hwnd, ctypes.byref(rectangle)):
            return True
        width, height = rectangle.right - rectangle.left, rectangle.bottom - rectangle.top
        found[int(hwnd)] = {"hwnd": int(hwnd), "pid": pid, "title": title.value,
            "class": class_name.value, "label": geometry.classify(title.value, width, height),
            "visible": bool(user32.IsWindowVisible(hwnd)),
            "bounds": {"left": rectangle.left, "top": rectangle.top,
                       "right": rectangle.right, "bottom": rectangle.bottom,
                       "width": width, "height": height}}
        return True

    user32.EnumWindows(visit, 0)
    return found


def observe_native(executable, requested_pid, geometry, clock, events, stop, interval):
    previous = {}
    owner = None
    next_pid_lookup = 0
    while not stop.is_set():
        now = time.perf_counter()
        if owner is None and now >= next_pid_lookup:
            next_pid_lookup = now + 0.25
            candidates = []
            if requested_pid:
                try:
                    candidate = PROBE.psutil.Process(requested_pid)
                    if Path(candidate.exe()).resolve() == executable:
                        candidates.append(candidate)
                except (PROBE.psutil.NoSuchProcess, PROBE.psutil.AccessDenied):
                    pass
            else:
                for candidate in PROBE.psutil.process_iter(["exe"]):
                    if candidate.info["exe"] and Path(candidate.info["exe"]).resolve() == executable:
                        candidates.append(candidate)
            if len(candidates) == 1:
                owner = candidates[0]
                events.put(clock.event("native_owner_identified", pid=owner.pid, executable=str(executable)))
        if owner is not None:
            if not owner.is_running():
                owner = None
                current = {}
            else:
                current = native_windows(owner.pid, geometry)
            at = time.perf_counter()
            for hwnd, state in current.items():
                old = previous.get(hwnd)
                if old is None:
                    events.put(clock.event("native_window_observed", at=at, **state))
                if (old is None and state["visible"]) or (old is not None and old["visible"] != state["visible"]):
                    events.put(clock.event("native_visibility_changed", at=at,
                        first_observation=old is None, previous_visible=old["visible"] if old else None, **state))
                elif old is not None and (old["bounds"] != state["bounds"] or old["label"] != state["label"]):
                    events.put(clock.event("native_geometry_changed", at=at, **state))
            for hwnd, state in previous.items():
                if hwnd not in current:
                    events.put(clock.event("native_window_destroyed", at=at, **state))
            previous = current
        stop.wait(interval)


def run(args):
    directory, case, executable, fixture = PROBE.checked_case(args.directory)
    output = (directory / args.output).resolve()
    if directory not in output.parents or output.suffix.lower() != ".json" or output.exists():
        raise ValueError("--output must be a new JSON artifact within the benchmark case")
    stop_path = directory / f"{output.stem}.stop"
    if stop_path.exists():
        raise ValueError(f"Remove the previous observer stop marker {stop_path}")
    output.parent.mkdir(parents=True, exist_ok=True)
    node = args.node or shutil.which("node")
    if not node:
        raise ValueError("Pass --node with a Node.js 22+ executable")
    transports = []
    monitors = []
    stop = threading.Event()
    events = queue.Queue()
    artifact = None
    native_thread = None
    try:
        for _ in LABELS:
            transports.append(PROBE.NodeCDP(node))
        clock = Clock()
        geometry = Geometry()
        log = LogTail(directory / "data" / args.log_name, clock, args.log_from_start)
        artifact = {"case": case, "fixture": fixture, "endpoint": f"http://127.0.0.1:{args.port}",
                    "started_utc": clock.event("start")["utc"], "poll_seconds": args.interval,
                    "log_path": str(log.path), "initial_log_offset": log.offset,
                    "text_marker": args.text_marker, "transport": "node-websocket",
                    "node_version": transports[0].version, "events": [],
                    "native_requested_pid": args.pid,
                    "epoch_source": "Wall/performance clock alignment refreshed for each event",
                    "method": "Read-only CDP and UTC application-log observer; no application launch or input",
                    "overhead": "Three debuggers, localhost discovery and read-only layout/paint polling; not a debugger-free memory run"}
        for label, transport in zip(LABELS, transports):
            monitor = Monitor(label, transport, clock, events, stop, args.interval, args.text_marker, geometry)
            monitors.append(monitor)
            monitor.thread.start()
        native_thread = threading.Thread(target=observe_native,
            args=(executable, args.pid, geometry, clock, events, stop, args.interval), daemon=True)
        native_thread.start()
        output.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
        print(json.dumps({"event": "ready_to_observe", "case": str(directory), "port": args.port,
                          "channels": list(LABELS), "output": str(output), "stop_file": str(stop_path),
                          "utc": clock.event("ready")["utc"]}), flush=True)
        opener = build_opener(ProxyHandler({}))
        previous_targets = {}
        last_endpoint_error = None
        while time.perf_counter() - clock.perf < args.seconds and not stop_path.exists():
            try:
                with opener.open(f"http://127.0.0.1:{args.port}/json/list", timeout=0.1) as response:
                    targets = targets_by_label(json.load(response), args.port)
                last_endpoint_error = None
                for monitor in monitors:
                    target = targets.get(monitor.label)
                    old = previous_targets.get(monitor.label)
                    if (target or {}).get("id") != (old or {}).get("id"):
                        if old is not None:
                            events.put(clock.event("target_lost", label=monitor.label, target_id=old["id"]))
                        if target is not None:
                            events.put(clock.event("target_observed", label=monitor.label,
                                                   target_id=target["id"], url=target["url"]))
                        monitor.assign(target)
                previous_targets = targets
            except PROBE.CDP_ERRORS + (ValueError,) as error:
                message = type(error).__name__ + ": " + str(error)[:180]
                if message != last_endpoint_error:
                    events.put(clock.event("endpoint_unavailable", message=message))
                    last_endpoint_error = message
            for event in log.read():
                events.put(event)
            changed = False
            while True:
                try:
                    event = events.get_nowait()
                except queue.Empty:
                    break
                artifact["events"].append(event)
                print(json.dumps(event), flush=True)
                changed = True
            if changed:
                output.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
            stop.wait(args.interval)
    except KeyboardInterrupt:
        if artifact is not None:
            artifact["interrupted"] = True
    finally:
        stop.set()
        for monitor in monitors:
            monitor.thread.join(timeout=2)
        if native_thread is not None:
            native_thread.join(timeout=2)
        for transport in transports:
            transport.close()
        if artifact is not None:
            while not events.empty():
                artifact["events"].append(events.get_nowait())
            artifact["finished_utc"] = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
            artifact["cleanup_node_pids"] = [transport.process.pid for transport in transports]
            artifact["cleanup_remaining_node_pids"] = [transport.process.pid for transport in transports
                                                       if transport.process.poll() is None]
            output.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
            print(json.dumps({"event": "observer_stopped", "output": str(output),
                              "events": len(artifact["events"]),
                              "cleanup_remaining_node_pids": artifact["cleanup_remaining_node_pids"]}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--port", type=int, required=True, help="Already selected dedicated localhost CDP port")
    parser.add_argument("--node", type=Path)
    parser.add_argument("--pid", type=int, help="Exact case application PID; default discovers its unique executable path")
    parser.add_argument("--seconds", type=float, default=600)
    parser.add_argument("--interval", type=float, default=0.02)
    parser.add_argument("--output", default="response-observations.json")
    parser.add_argument("--text-marker", default="Clipboard benchmark record 1000")
    parser.add_argument("--log-name", default="dezirclip.log")
    parser.add_argument("--log-from-start", action="store_true")
    arguments = parser.parse_args()
    if os.name != "nt":
        parser.error("This observer requires Windows")
    if not 1 <= arguments.port <= 65535 or arguments.seconds <= 0 or arguments.interval <= 0:
        parser.error("Port must be 1 through 65535; duration and interval must be positive")
    if Path(arguments.log_name).name != arguments.log_name:
        parser.error("--log-name must be a filename in case/data")
    try:
        run(arguments)
    except (OSError, ValueError, KeyError, PROBE.psutil.Error) as error:
        parser.exit(1, f"Cannot observe benchmark response: {error}\n")
