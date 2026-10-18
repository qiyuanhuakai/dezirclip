"""Observe an existing isolated benchmark application; never launch it or send input.

Requires Windows, psutil, and Node.js 22+ with WebSocket. The case must already
be prepared/migrated/seeded under .harness/perf/cases. Start its application with
a dedicated localhost debugging port using benchmark-windows.py separately.

Example (the observer starts only its own Node transport):
  python scripts/probe-webview-interaction.py observe CASE --port 9338 --trace --node NODE
  python scripts/probe-webview-interaction.py mark CASE scroll-down-01 start
  python scripts/probe-webview-interaction.py mark CASE scroll-down-01 end
Wait for scenario_started before performing the manually controlled UI action.
End markers schedule read-only snapshots after 10 and 30 seconds. Do not start
the next scenario until those retention snapshots finish when measuring retention.
Stop with the output stem's .stop file; the application is left running.

Metrics/trace collection changes instrumentation, never application DOM/state.
No rAF loop, observer injection, screenshot, forced GC, throttling, or input is
used. Marker wall duration includes operator/tool idle time and is not an FPS
denominator. Trace activity clusters and native-window motion are reported
separately. Frames from Performance.getMetrics is a gauge, not presented frames.
"""

import argparse
import base64
from collections import Counter, defaultdict
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import faulthandler
import importlib.util
import json
import os
from pathlib import Path
import queue
import statistics
import sys
import threading
import time
import traceback
from urllib.request import ProxyHandler, build_opener
import uuid

import psutil


SPEC = importlib.util.spec_from_file_location(
    "interaction_startup_probe", Path(__file__).with_name("probe-webview-startup.py"))
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)

COUNTERS = {"TaskDuration", "ScriptDuration", "LayoutDuration", "RecalcStyleDuration",
            "TaskOtherDuration", "ThreadTime", "ProcessTime", "LayoutCount", "RecalcStyleCount"}
GAUGES = {"JSHeapUsedSize", "JSHeapTotalSize", "Nodes", "Documents", "Frames", "JSEventListeners"}
CATEGORIES = ["devtools.timeline", "disabled-by-default-devtools.timeline",
              "disabled-by-default-devtools.timeline.frame",
              "disabled-by-default-devtools.timeline.inputs", "input", "latencyInfo"]
INPUT_TYPES = {"wheel", "scroll", "click", "dblclick", "mousedown", "mouseup", "mousemove",
               "pointerdown", "pointerup", "pointermove", "keydown", "keyup",
               "touchstart", "touchend", "touchmove", "dragstart", "drag", "dragend", "dragover", "drop"}
FRAME_NAMES = {"DrawFrame", "BeginFrame", "BeginMainThreadFrame", "SwapBuffers",
               "Display::DrawAndSwap"}
SOURCES = [
    "https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Performance.pdl",
    "https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Tracing.pdl",
    "https://developer.chrome.com/docs/devtools/performance/reference",
    "https://raw.githubusercontent.com/ChromeDevTools/devtools-frontend/main/front_end/models/trace/handlers/MetaHandler.ts",
    "https://raw.githubusercontent.com/ChromeDevTools/devtools-frontend/main/front_end/models/trace/handlers/FramesHandler.ts",
    "https://raw.githubusercontent.com/chromium/chromium/main/base/task/sequence_manager/thread_controller_with_message_pump_impl.cc",
]


def now_ms():
    return time.time() * 1000


def utc(epoch_ms=None):
    return datetime.fromtimestamp((epoch_ms if epoch_ms is not None else now_ms()) / 1000,
                                  timezone.utc).isoformat(timespec="milliseconds")


def safe_output(directory, name):
    output = (directory / name).resolve()
    if directory not in output.parents or output.suffix != ".json":
        raise ValueError("Output must be a JSON file inside the case")
    return output


class CDP(PROBE.NodeCDP):
    """Keep asynchronous notifications rather than discarding unmatched messages."""

    def __init__(self, node):
        self.notifications = []
        super().__init__(node)

    def receive_message(self, timeout):
        try:
            received, event = self.events.get(timeout=timeout)
        except queue.Empty as error:
            raise TimeoutError("CDP response timed out") from error
        if event.get("type") == "error":
            raise RuntimeError(event.get("message", "CDP transport error"))
        return received, event

    def command(self, method, params=None, timeout=5):
        self.next_id += 1
        identity = self.next_id
        self.send({"op": "send", "value": {"id": identity, "method": method,
                                               "params": params or {}}})
        deadline = time.perf_counter() + timeout
        while time.perf_counter() < deadline:
            received, event = self.receive_message(max(0.001, deadline - time.perf_counter()))
            message = event.get("value", {}) if event.get("type") == "message" else {}
            if message.get("id") == identity:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return received, message.get("result", {})
            if "method" in message:
                self.notifications.append(message)
        raise TimeoutError(method)

    def notification(self, method, timeout=10):
        deadline = time.perf_counter() + timeout
        while time.perf_counter() < deadline:
            for index, message in enumerate(self.notifications):
                if message.get("method") == method:
                    return self.notifications.pop(index).get("params", {})
            _, event = self.receive_message(max(0.001, deadline - time.perf_counter()))
            if event.get("type") == "message" and "method" in event.get("value", {}):
                self.notifications.append(event["value"])
        raise TimeoutError(method)


def native_main_windows(pid):
    user32 = ctypes.windll.user32
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
    user32.GetDpiForWindow.argtypes = [wintypes.HWND]
    dwm = ctypes.windll.dwmapi
    dwm.DwmGetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]
    found = []

    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            title = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, title, len(title))
            if title.value in ("DezirClip", "TieZ"):
                box = wintypes.RECT()
                user32.GetWindowRect(hwnd, ctypes.byref(box))
                physical = wintypes.RECT()
                dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(physical), ctypes.sizeof(physical))
                found.append({"hwnd": int(hwnd), "visible": bool(user32.IsWindowVisible(hwnd)),
                              "topmost": bool(user32.GetWindowLongPtrW(hwnd, -20) & 8),
                              "dpi": user32.GetDpiForWindow(hwnd),
                              "bounds": [box.left, box.top, box.right, box.bottom],
                              "dwm_physical_bounds": [physical.left, physical.top, physical.right, physical.bottom]})
        return True

    user32.EnumWindows(visit, 0)
    return found


class ProcessSampler:
    def __init__(self, root, interval):
        self.root = root
        self.interval = interval
        self.tracked = {(root.pid, root.create_time()): root}
        self.cpu = {}
        self.samples = []
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        while not self.stopped.is_set():
            started = time.perf_counter()
            members = []
            for parent in list(self.tracked.values()):
                try:
                    for member in parent.children(recursive=True):
                        self.tracked[(member.pid, member.create_time())] = member
                except psutil.Error:
                    pass
            for identity, member in list(self.tracked.items()):
                try:
                    if not member.is_running():
                        del self.tracked[identity]
                        continue
                    memory = member.memory_info()
                    cpu = member.cpu_times()
                    self.cpu[identity] = cpu.user + cpu.system
                    members.append({"pid": member.pid, "name": member.name(),
                                    "private_bytes": memory.private, "rss_bytes": memory.rss})
                except psutil.Error:
                    pass
            sample = {"epoch_ms": now_ms(), "perf_counter": time.perf_counter(),
                      "private_bytes": sum(m["private_bytes"] for m in members),
                      "rss_bytes": sum(m["rss_bytes"] for m in members),
                      "lifetime_cpu_seconds": sum(self.cpu.values()), "processes": members,
                      "native_main_windows": native_main_windows(self.root.pid) if self.root.is_running() else []}
            with self.lock:
                self.samples.append(sample)
            self.stopped.wait(max(0, self.interval - (time.perf_counter() - started)))

    def snapshot(self):
        with self.lock:
            return list(self.samples)

    def close(self):
        self.stopped.set()
        self.thread.join(timeout=3)


def metric_snapshot(cdp):
    before = now_ms()
    _, response = cdp.command("Performance.getMetrics")
    after = now_ms()
    values = {m["name"]: m["value"] for m in response["metrics"]}
    _, frame = cdp.command("Page.getFrameTree")
    _, document = cdp.command("Runtime.evaluate", {"expression": "({visibility_state:document.visibilityState,viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio}})", "returnByValue": True, "silent": True})
    return {"epoch_ms": after, "utc": utc(after), "command_roundtrip_ms": after - before,
            "trace_to_epoch_ms": (before + after) / 2 - values["Timestamp"] * 1000,
            "metrics": values, "main_frame": frame["frameTree"]["frame"], "document": document["result"]["value"]}


def process_summary(samples, low, high):
    selected = [s for s in samples if low <= s["epoch_ms"] <= high]
    if not selected:
        return {"available": False}
    previous = next((s for s in reversed(samples) if s["epoch_ms"] <= low), selected[0])
    following = next((s for s in samples if s["epoch_ms"] >= high), selected[-1])
    rates = [(b["lifetime_cpu_seconds"] - a["lifetime_cpu_seconds"]) * 100000 /
             (b["epoch_ms"] - a["epoch_ms"]) for a, b in zip(selected, selected[1:])
             if b["epoch_ms"] > a["epoch_ms"]]
    return {"available": True, "sample_count": len(selected),
            "native_main_visible_all_samples": all(any(w["visible"] for w in s["native_main_windows"]) for s in selected),
            "native_main_topmost_all_samples": all(any(w["visible"] and w.get("topmost") for w in s["native_main_windows"]) for s in selected),
            "observed_epoch_ms": [selected[0]["epoch_ms"], selected[-1]["epoch_ms"]],
            "peak_private_mib": max(s["private_bytes"] for s in selected) / 1048576,
            "peak_rss_mib": max(s["rss_bytes"] for s in selected) / 1048576,
            "median_private_mib": statistics.median(s["private_bytes"] for s in selected) / 1048576,
            "median_rss_mib": statistics.median(s["rss_bytes"] for s in selected) / 1048576,
            "cpu_seconds_estimate": following["lifetime_cpu_seconds"] - previous["lifetime_cpu_seconds"],
            "cpu_boundary_sample_epoch_ms": [previous["epoch_ms"], following["epoch_ms"]],
            "peak_cpu_one_core_percent": max(rates) if rates else None,
            "process_counts": sorted({len(s["processes"]) for s in selected})}


def clusters(intervals, gap_us):
    merged = []
    for low, high in sorted(intervals):
        if merged and low - merged[-1][1] <= gap_us:
            merged[-1][1] = max(merged[-1][1], high)
        else:
            merged.append([low, high])
    return merged


def duration_events(events):
    complete = []
    stacks = defaultdict(list)
    for event in sorted(events, key=lambda e: e.get("ts", 0)):
        phase = event.get("ph")
        if phase == "X" and "dur" in event:
            complete.append(event)
        elif phase == "B":
            stacks[(event.get("pid"), event.get("tid"))].append(event)
        elif phase == "E":
            stack = stacks[(event.get("pid"), event.get("tid"))]
            if stack:
                start = stack.pop()
                complete.append({**start, "ph": "X", "dur": event["ts"] - start["ts"]})
    return complete


def trace_summary(events, frame_id, offset_ms, process_samples, sample_interval, capture_bounds):
    mappings = []
    all_frames = []
    layer_ids = set()
    threads = {}
    for event in events:
        data = event.get("args", {}).get("data", {})
        if event.get("name") == "thread_name":
            threads[(event.get("pid"), event.get("tid"))] = event.get("args", {}).get("name")
        if event.get("name") == "TracingStartedInBrowser":
            all_frames.extend(data.get("frames", []))
            mappings.extend(f for f in data.get("frames", []) if f.get("frame") == frame_id)
        elif event.get("name") == "FrameCommittedInBrowser":
            all_frames.append(data)
            if data.get("frame") == frame_id:
                mappings.append(data)
        elif event.get("name") == "CommitLoad" and data.get("frame") == frame_id:
            mappings.append({"frame": frame_id, "processId": event.get("pid"), "source": "CommitLoad"})
        if event.get("name") == "SetLayerTreeId" and data.get("frame") == frame_id:
            layer_ids.add(data["layerTreeId"])
    pids = {int(m["processId"]) for m in mappings if m.get("processId") is not None}
    main_threads = {(pid, tid) for (pid, tid), name in threads.items()
                    if pid in pids and name == "CrRendererMain"}
    low_ts, high_ts = [(t - offset_ms) * 1000 for t in capture_bounds]
    scoped_events = [e for e in events if low_ts <= e.get("ts", -1) <= high_ts]
    all_durations = duration_events(events)
    boundary_durations = [e for e in all_durations if e["ts"] < low_ts < e["ts"] + e["dur"] or
                          e["ts"] < high_ts < e["ts"] + e["dur"]]
    durations = [e for e in all_durations if low_ts <= e["ts"] and e["ts"] + e["dur"] <= high_ts]
    task_candidates = sorted((e for e in durations if e.get("name") == "RunTask" and
                              (e.get("pid"), e.get("tid")) in main_threads),
                             key=lambda e: (e["ts"], -e["dur"]))
    tasks = []
    for event in task_candidates:
        if not any(parent["pid"] == event["pid"] and parent["tid"] == event["tid"] and
                   parent["ts"] <= event["ts"] and event["ts"] + event["dur"] <= parent["ts"] + parent["dur"]
                   for parent in tasks[-20:]):
            tasks.append(event)
    long_tasks = [{"ts_us": e["ts"], "duration_ms": e["dur"] / 1000,
                   "epoch_ms": e["ts"] / 1000 + offset_ms} for e in tasks if e["dur"] > 50000]
    inputs = [e for e in durations if e.get("name") == "EventDispatch" and
              (e.get("pid"), e.get("tid")) in main_threads and
              e.get("args", {}).get("data", {}).get("type") in INPUT_TYPES and
              (e["args"]["data"]["type"] not in ("mousemove", "pointermove") or
               e["args"]["data"].get("buttons", 0) != 0)]
    input_intervals = clusters([(e["ts"], e["ts"] + e["dur"]) for e in inputs], 250000)
    active = []
    for low, high in input_intervals:
        padded_low, padded_high = low - 50000, high + 250000
        active.append({"trace_bounds_us": [low, high],
                       "input_span_ms": (high - low) / 1000,
                       "padded_bounds_epoch_ms": [padded_low / 1000 + offset_ms,
                                                  padded_high / 1000 + offset_ms],
                       "input_counts": dict(Counter(e["args"]["data"]["type"] for e in inputs
                                                     if low <= e["ts"] <= high)),
                       "process_estimate": process_summary(process_samples,
                           padded_low / 1000 + offset_ms, padded_high / 1000 + offset_ms)})
    streams = defaultdict(list)
    drawn = {}
    dropped = {}
    for event in scoped_events:
        if event.get("name") in FRAME_NAMES and event.get("ph") not in ("E", "e"):
            streams[(event["name"], event.get("pid"), event.get("tid"))].append(event["ts"])
        args = event.get("args", {})
        if args.get("layerTreeId") in layer_ids and event.get("ph") == "I" and "frameSeqId" in args:
            identity = (args["layerTreeId"], args["frameSeqId"])
            if event.get("name") == "DrawFrame":
                drawn.setdefault(identity, event)
            elif event.get("name") == "DroppedFrame":
                dropped.setdefault(identity, event)
    stream_rows = []
    for (name, pid, tid), timestamps in streams.items():
        stamps = sorted(set(timestamps))
        intervals = [(b - a) / 1000 for a, b in zip(stamps, stamps[1:])
                     if any(low - 50000 <= a <= b <= high + 250000 for low, high in input_intervals)]
        stream_rows.append({"event": name, "pid": pid, "tid": tid,
                            "mapped_to_main_renderer": pid in pids, "count": len(stamps),
                            "input_correlated_interval_count": len(intervals),
                            "median_input_correlated_interval_ms": statistics.median(intervals) if intervals else None,
                            "max_input_correlated_interval_ms": max(intervals) if intervals else None,
                            "meaning": "Trace event cadence proxy; not displayed FPS or dropped frames"})
    motion = []
    for a, b in zip(process_samples, process_samples[1:]):
        previous = [(w["hwnd"], w["bounds"]) for w in a["native_main_windows"] if w["visible"]]
        current = [(w["hwnd"], w["bounds"]) for w in b["native_main_windows"] if w["visible"]]
        if current and previous and current != previous and capture_bounds[0] <= b["epoch_ms"] <= capture_bounds[1]:
            motion.append((a["epoch_ms"], b["epoch_ms"]))
    draw_times = sorted(e["ts"] for e in drawn.values())
    draw_intervals = [(b - a) / 1000 for a, b in zip(draw_times, draw_times[1:])
                      if any(low - 50000 <= a <= b <= high + 250000 for low, high in input_intervals)]
    ordered_intervals = sorted(draw_intervals)
    rendering = {}
    for name in ("Layout", "UpdateLayoutTree", "Paint"):
        selected = [e for e in durations if e.get("name") == name and
                    (e.get("pid"), e.get("tid")) in main_threads and
                    e.get("args", {}).get("data", {}).get("frame", frame_id) == frame_id and
                    e.get("args", {}).get("beginData", {}).get("frame", frame_id) == frame_id]
        rendering[name] = {"count": len(selected), "total_duration_ms": sum(e["dur"] for e in selected) / 1000}
    return {"main_frame_id": frame_id, "frame_renderer_mappings": mappings,
            "shared_renderer_frames": [f for f in all_frames if f.get("processId") in pids],
            "mapped_renderer_pids": sorted(pids), "mapped_main_threads": sorted(main_threads),
            "attribution_scope_stable": len(pids) == 1 and len(layer_ids) <= 1 and bool(main_threads),
            "capture_trace_bounds_us": [low_ts, high_ts],
            "boundary_duration_events_excluded": len(boundary_durations),
            "main_thread_task_instrumentation_available": bool(tasks),
            "main_thread_task_count": len(tasks) if tasks else None,
            "long_tasks_over_50ms": long_tasks if tasks else None,
            "long_task_excess_over_50ms": sum(t["duration_ms"] - 50 for t in long_tasks) if tasks else None,
            "input_event_count": len(inputs), "input_correlated_activity_windows": active,
            "input_cluster_gap_ms": 250, "input_window_padding_ms": {"before": 50, "after": 250},
            "hover_moves_excluded": True,
            "input_activity_limit": "EventDispatch measures dispatch only; pure compositor scroll or OS window motion may have no corresponding DOM event",
            "native_window_motion_epoch_ms": clusters(motion, sample_interval * 2000),
            "main_layer_tree_ids": sorted(layer_ids),
            "main_layer_tree_draw_cadence": {"available": bool(layer_ids and drawn),
                "unique_draw_count": len(drawn) if layer_ids else None,
                "input_correlated_interval_count": len(draw_intervals),
                "p50_interval_ms": statistics.median(draw_intervals) if draw_intervals else None,
                "p95_interval_ms": ordered_intervals[min(len(ordered_intervals)-1, int(len(ordered_intervals)*0.95))] if ordered_intervals else None,
                "max_interval_ms": max(draw_intervals) if draw_intervals else None,
                "explicit_dropped_events": len(dropped) if layer_ids else None,
                "partial_dropped_events": sum(bool(e["args"].get("hasPartialUpdate")) for e in dropped.values()) if layer_ids else None,
                "meaning": "Same-layer-tree DrawFrame cadence; idle intervals are not dropped frames; no screen FPS claim"},
            "rendering_slices": rendering,
            "frame_event_streams": stream_rows, "presented_fps": None,
            "presented_fps_reason": "No validated presentation pipeline reconstruction; OS window dragging can bypass renderer frames",
            "raw_event_name_counts": dict(Counter(e.get("name", "") for e in events))}


def finish_trace(cdp, path):
    cdp.command("Tracing.end")
    completed = cdp.notification("Tracing.tracingComplete", 20)
    stream = completed.get("stream")
    if not stream:
        raise RuntimeError("Tracing completed without the requested stream")
    try:
        with path.open("wb") as output:
            while True:
                _, chunk = cdp.command("IO.read", {"handle": stream, "size": 1048576}, timeout=10)
                output.write(base64.b64decode(chunk["data"]) if chunk.get("base64Encoded")
                             else chunk["data"].encode("utf-8"))
                if chunk.get("eof"):
                    break
    finally:
        cdp.command("IO.close", {"handle": stream})
    trace = json.loads(path.read_text(encoding="utf-8"))
    return trace.get("traceEvents", []), completed


def observe(args):
    directory, case, executable, fixture = PROBE.checked_case(args.directory)
    output = safe_output(directory, args.output)
    if output.exists():
        raise ValueError("Refusing to overwrite the output")
    marker_path = output.with_suffix(".markers.jsonl")
    if marker_path.exists():
        raise ValueError("Refusing existing markers; select a fresh output name")
    marker_path.touch()
    roots = [p for p in psutil.process_iter(["exe"]) if p.info["exe"] and
             Path(p.info["exe"]).resolve() == executable]
    if len(roots) != 1:
        raise ValueError("Exactly one already-running executable from this case is required")
    opener = build_opener(ProxyHandler({}))
    with opener.open(f"http://127.0.0.1:{args.port}/json/list", timeout=2) as response:
        target = PROBE.main_target(json.load(response), args.port)
    if not target:
        raise ValueError("No release main-page CDP target found")
    owner_check = {"verified": False, "reason": "Listener owner unavailable"}
    try:
        owned_pids = {roots[0].pid, *(p.pid for p in roots[0].children(recursive=True))}
        listeners = [c.pid for c in psutil.net_connections(kind="tcp")
                     if c.status == psutil.CONN_LISTEN and c.laddr.port == args.port]
        if listeners and not any(pid in owned_pids for pid in listeners):
            raise ValueError("Debugging port belongs to a process outside the exact case tree")
        owner_check = {"verified": any(pid in owned_pids for pid in listeners), "listener_pids": listeners}
    except psutil.AccessDenied:
        owner_check["reason"] = "Access denied while inspecting listener owners"
    with opener.open(f"http://127.0.0.1:{args.port}/json/version", timeout=2) as response:
        browser_version = json.load(response)
    transport = CDP(args.node)
    sampler = ProcessSampler(roots[0], args.interval)
    sampler.thread.start()
    artifact = {"case": case, "fixture": fixture, "started_utc": utc(),
                "observer_pid": os.getpid(), "node_transport_pid": transport.process.pid,
                "pid": roots[0].pid, "target_id": target["id"], "target_url": target["url"],
                "endpoint_owner": owner_check,
                "browser_version": {k: browser_version.get(k) for k in
                                    ("Browser", "Protocol-Version", "V8-Version", "WebKit-Version")},
                "node_version": transport.version, "process_sample_interval_seconds": args.interval,
                "trace_enabled": args.trace, "trace_categories": CATEGORIES if args.trace else [],
                "light_trace": args.light_trace,
                "markers": [], "scenarios": [], "errors": [], "sources": SOURCES,
                "limitations": ["Instrumented interaction run; do not mix its memory with debugger-free baselines.",
                    "Scenario wall duration includes manual/tool idle; it is not a continuous activity or frame-time measurement.",
                    "Process CPU boundary samples have uncertainty of roughly one process sampling interval.",
                    "Exited process CPU tails can be missed and newly discovered children contribute already-consumed CPU; CPU is a sampling estimate.",
                    "Retention snapshots retain their actual delay; a late snapshot is not relabeled as exactly 10 or 30 seconds.",
                    "Task/script/layout metrics overlap and must not be summed as independent work.",
                    "Frames is a frame-object gauge, not an FPS counter; JS heap is not process committed memory.",
                    "No garbage collection is forced. Post-action retention is a snapshot, not proof of a leak.",
                    "Native GetWindowRect coordinates can be DPI virtualized; no per-frame OS presentation claim.",
                    "Trace event cadence and input-correlated activity windows are descriptive proxies, not presented FPS or input-to-paint latency."]}
    current = None
    pending = []
    trace_running = False
    stop = output.with_suffix(".stop")
    marker_offset = 0
    deadline = time.perf_counter() + args.seconds
    last_write = 0
    next_dom_sample = 0

    def save():
        artifact["process_samples"] = sampler.snapshot()
        temporary = output.with_suffix(".pending.json")
        temporary.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
        for attempt in range(20):
            try:
                temporary.replace(output)
                return
            except PermissionError:
                if attempt == 19:
                    raise
                time.sleep(0.05)

    try:
        transport.connect(target["webSocketDebuggerUrl"])
        transport.command("Performance.enable", {"timeDomain": "timeTicks"})
        artifact["initial_snapshot"] = metric_snapshot(transport)
        print(json.dumps({"event": "ready_to_observe", "case": str(directory),
                          "output": str(output), "markers": str(marker_path), "utc": utc()}), flush=True)
        while time.perf_counter() < deadline and not stop.exists():
            with marker_path.open("rb") as markers:
                markers.seek(marker_offset)
                chunk = markers.read()
                complete = chunk.rfind(b"\n") + 1
                lines = chunk[:complete].splitlines()
                marker_offset += complete
            for line in lines:
                marker = json.loads(line)
                artifact["markers"].append(marker)
                if marker["phase"] == "start":
                    if current:
                        artifact["errors"].append({"marker": marker, "error": "A scenario is already active"})
                        continue
                    if args.trace:
                        try:
                            transport.command("Tracing.start", {"transferMode": "ReturnAsStream",
                                "streamFormat": "json", "traceConfig": {"recordMode": "recordUntilFull",
                                "traceBufferSizeInKb": 32768, "enableSampling": False,
                                "enableSystrace": False, "includedCategories": CATEGORIES,
                                "excludedCategories": ["disabled-by-default-devtools.timeline.frame", "cc", "benchmark", "input", "latencyInfo"] if args.light_trace else []}})
                            trace_running = True
                        except PROBE.CDP_ERRORS as error:
                            artifact["errors"].append({"scenario": marker["scenario"], "trace_start_error": repr(error)})
                            print(json.dumps({"event": "trace_unavailable", "error": repr(error)}), flush=True)
                    before = metric_snapshot(transport)
                    current = {"scenario": marker["scenario"], "start_marker": marker,
                               "before": before, "sequence": len(artifact["scenarios"]) + 1,
                               "capture_start_epoch_ms": before["epoch_ms"]}
                    artifact["scenarios"].append(current)
                    print(json.dumps({"event": "scenario_started", "scenario": marker["scenario"],
                                      "utc": utc(), "metrics": {k: before["metrics"].get(k) for k in GAUGES}}), flush=True)
                elif marker["phase"] == "end":
                    if not current or current["scenario"] != marker["scenario"]:
                        artifact["errors"].append({"marker": marker, "error": "No matching active scenario"})
                        continue
                    after = metric_snapshot(transport)
                    current.update({"end_marker": marker, "after": after,
                                    "capture_end_epoch_ms": after["epoch_ms"],
                                    "marker_wall_duration_ms": marker["epoch_ms"] - current["start_marker"]["epoch_ms"],
                                    "counter_deltas": {k: after["metrics"][k] - current["before"]["metrics"][k]
                                                       for k in COUNTERS if k in after["metrics"] and k in current["before"]["metrics"]},
                                    "gauges_before_after": {k: [current["before"]["metrics"].get(k), after["metrics"].get(k)] for k in GAUGES}})
                    current["navigation_unchanged"] = all(current["before"]["main_frame"].get(k) == after["main_frame"].get(k)
                                                          for k in ("id", "loaderId"))
                    if trace_running:
                        trace_path = output.with_name(output.stem + f".trace-{len(artifact['scenarios']):02}.json")
                        try:
                            events, completed = finish_trace(transport, trace_path)
                            current["trace"] = {"path": str(trace_path), "sha256": hashlib.sha256(trace_path.read_bytes()).hexdigest(),
                                                "data_loss_occurred": completed.get("dataLossOccurred"),
                                                "valid": completed.get("dataLossOccurred") is False,
                                                **trace_summary(events, current["before"]["main_frame"]["id"],
                                                    current["before"]["trace_to_epoch_ms"], sampler.snapshot(), args.interval,
                                                    [current["capture_start_epoch_ms"], after["epoch_ms"]])}
                        except Exception as error:
                            current["trace"] = {"valid": False, "path": str(trace_path), "error": repr(error)}
                            artifact["errors"].append({"scenario": current["scenario"], "trace_end_error": repr(error)})
                            print(json.dumps({"event": "trace_unavailable", "error": repr(error)}), flush=True)
                        finally:
                            trace_running = False
                    current["process_summary"] = process_summary(sampler.snapshot(), current["capture_start_epoch_ms"], after["epoch_ms"])
                    if "trace" in current:
                        current["trace"]["valid"] = (current["trace"]["valid"] and current["navigation_unchanged"] and
                                                        current["trace"].get("attribution_scope_stable", False))
                    for seconds in (10, 30):
                        pending.append((after["epoch_ms"] + seconds * 1000, current, seconds))
                    print(json.dumps({"event": "scenario_ended", "scenario": current["scenario"],
                                      "counter_deltas": current["counter_deltas"],
                                      "process_summary": current["process_summary"], "utc": utc()}), flush=True)
                    current = None
                elif marker["phase"] == "point":
                    print(json.dumps({"event": "point_marker", **marker}), flush=True)
            if args.observe_drag_class and current and "drag" in current["scenario"].lower() and time.perf_counter() >= next_dom_sample:
                _, response = transport.command("Runtime.evaluate", {"expression": "({dragging:!!document.querySelector('.is-dragging')})",
                    "returnByValue": True, "silent": True})
                value = response.get("result", {}).get("value", {})
                samples = current.setdefault("drag_class_samples", [])
                sample = {"epoch_ms": now_ms(), "dragging": value.get("dragging")}
                if not samples or samples[-1]["dragging"] != sample["dragging"]:
                    print(json.dumps({"event": "drag_class_changed", "scenario": current["scenario"], **sample}), flush=True)
                samples.append(sample)
                next_dom_sample = time.perf_counter() + 0.1
            for item in list(pending):
                due, scenario, seconds = item
                if now_ms() >= due:
                    snapshot = metric_snapshot(transport)
                    data = sampler.snapshot()
                    snapshot["process_sample"] = data[-1] if data else None
                    snapshot["delay_after_end_ms"] = snapshot["epoch_ms"] - scenario["capture_end_epoch_ms"]
                    snapshot["overlapped_next_scenario"] = len(artifact["scenarios"]) > scenario["sequence"]
                    scenario.setdefault("post_action_snapshots", {})[str(seconds)] = snapshot
                    pending.remove(item)
                    print(json.dumps({"event": "retention_snapshot", "scenario": scenario["scenario"],
                                      "seconds": seconds, "utc": snapshot["utc"],
                                      "gauges": {k: snapshot["metrics"].get(k) for k in GAUGES}}), flush=True)
            if time.perf_counter() - last_write >= 1:
                save()
                last_write = time.perf_counter()
            time.sleep(0.05)
    except BaseException as error:
        artifact["errors"].append({"fatal_error": repr(error), "utc": utc()})
        output.with_suffix(".failure.log").write_text(traceback.format_exc(), encoding="utf-8")
        raise
    finally:
        if trace_running:
            try:
                unfinished = output.with_name(output.stem + ".trace-unfinished.json")
                finish_trace(transport, unfinished)
                artifact["unfinished_trace"] = str(unfinished)
            except Exception as error:
                artifact["errors"].append({"error": str(error)})
        try:
            transport.command("Performance.disable")
            transport.disconnect()
        except PROBE.CDP_ERRORS:
            pass
        transport.close()
        sampler.close()
        artifact["finished_utc"] = utc()
        artifact["pending_retention_snapshots_at_stop"] = len(pending)
        artifact["cleanup_node_exit_code"] = transport.process.poll()
        save()
        print(json.dumps({"event": "observer_stopped", "output": str(output),
                          "cleanup_node_exit_code": transport.process.poll()}), flush=True)


def mark(args):
    directory, _, _, _ = PROBE.checked_case(args.directory)
    output = safe_output(directory, args.output)
    marker_path = output.with_suffix(".markers.jsonl")
    if not marker_path.exists():
        raise ValueError("Observer marker file is absent; start observer first")
    marker = {"id": str(uuid.uuid4()), "scenario": args.scenario, "phase": args.phase,
              "epoch_ms": now_ms(), "utc": utc(), "note": args.note}
    with marker_path.open("a", encoding="utf-8") as markers:
        markers.write(json.dumps(marker, ensure_ascii=False) + "\n")
    print(json.dumps(marker, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    faulthandler.enable()
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    observer = commands.add_parser("observe", help="Observe one already running exact case executable")
    observer.add_argument("directory", type=Path)
    observer.add_argument("--port", type=int, required=True)
    observer.add_argument("--node", type=Path, required=True)
    observer.add_argument("--seconds", type=float, default=1800)
    observer.add_argument("--interval", type=float, default=0.25)
    observer.add_argument("--trace", action="store_true")
    observer.add_argument("--light-trace", action="store_true", help="Main-thread/EventTiming trace; omits expensive compositor/pipeline categories")
    observer.add_argument("--observe-drag-class", action="store_true",
                          help="Read .is-dragging at 100 ms only during scenarios containing 'drag'; adds instrumentation work")
    observer.add_argument("--output", default="interaction-profile.json")
    observer.set_defaults(function=observe)
    marker = commands.add_parser("mark", help="Append a local scenario marker; sends no UI input")
    marker.add_argument("directory", type=Path)
    marker.add_argument("scenario")
    marker.add_argument("phase", choices=("start", "end", "point"))
    marker.add_argument("--note", default="")
    marker.add_argument("--output", default="interaction-profile.json")
    marker.set_defaults(function=mark)
    args = parser.parse_args()
    if args.command == "observe" and args.light_trace:
        CATEGORIES[:] = ["devtools.timeline", "disabled-by-default-devtools.timeline",
                         "disabled-by-default-devtools.timeline.inputs"]
    if os.name != "nt":
        parser.error("Windows is required")
    if args.command == "observe" and (args.interval <= 0 or args.seconds <= 0 or not 1 <= args.port <= 65535):
        parser.error("Positive duration/interval and a valid dedicated port are required")
    try:
        args.function(args)
    except (OSError, ValueError, KeyError, RuntimeError, TimeoutError, psutil.Error) as error:
        parser.exit(1, f"Cannot observe interaction: {error}\n")
