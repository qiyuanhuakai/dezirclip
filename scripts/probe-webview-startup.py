"""Measure fixture content readiness in isolated Windows release applications.

Requires psutil and either websocket-client or Node.js 22+ with WebSocket.
Prepare, migrate, stop and seed a case with benchmark-windows.py and
seed-performance-history.py first. This probe launches only that case executable.

The sole CDP command is Runtime.evaluate: read body.textContent and image/layout
properties. Readiness means the requested text marker, at least one visible
list-preview image, and every visible image complete with naturalWidth > 0.
This is content readiness, not proof of input responsiveness or screen paint.

The Node transport starts before timing. Timing includes local CDP discovery,
connection, polling and read-only evaluation; memory benchmarks must run without
this debugger. Each trial uses a fresh browser profile by default. OS file caches
are not flushed, and executable hash verification reads the binary before timing.
Example: python scripts/probe-webview-startup.py .harness/perf/cases/fork --repeat 3
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import socket
import statistics
import subprocess
import sys
import threading
import time
from urllib.error import URLError
from urllib.parse import parse_qs, urlsplit
from urllib.request import ProxyHandler, build_opener
import uuid

import psutil

try:
    import websocket
except ImportError:
    websocket = None


CDP_ERRORS = (URLError, OSError, TimeoutError, RuntimeError)
if websocket is not None:
    CDP_ERRORS += (websocket.WebSocketException,)


WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
CASES_ROOT = WORKSPACE_ROOT / ".harness" / "perf" / "cases"
NODE_TRANSPORT = r"""
import readline from 'node:readline';
let socket;
const output = value => process.stdout.write(JSON.stringify(value) + '\n');
if (typeof WebSocket !== 'function') {
  output({type:'error', message:'Node.js does not provide WebSocket'});
  process.exit(1);
}
output({type:'boot', version:process.version});
const input = readline.createInterface({input:process.stdin, terminal:false});
input.on('line', line => {
  try {
    const request = JSON.parse(line);
    if (request.op === 'connect') {
      socket = new WebSocket(request.url);
      socket.addEventListener('open', () => output({type:'connected'}));
      socket.addEventListener('message', event => output({type:'message', value:JSON.parse(event.data)}));
      socket.addEventListener('error', () => output({type:'error', message:'WebSocket connection failed'}));
    } else if (request.op === 'send') {
      socket.send(JSON.stringify(request.value));
    } else if (request.op === 'close') {
      socket?.close();
      output({type:'closed'});
    }
  } catch (error) {
    output({type:'error', message:String(error)});
  }
});
input.on('close', () => { socket?.close(); process.exit(0); });
"""


def readiness_expression(marker):
    return """(() => {
      const marker = MARKER;
      const visible = Array.from(document.images).filter(image => {
        const box = image.getBoundingClientRect();
        const style = getComputedStyle(image);
        return box.width > 0 && box.height > 0 && box.right > 0 && box.bottom > 0 &&
          box.left < innerWidth && box.top < innerHeight && style.display !== 'none' &&
          style.visibility !== 'hidden' && style.visibility !== 'collapse' && Number(style.opacity) !== 0;
      });
      const textPresent = !!document.body && document.body.textContent.includes(marker);
      const previewCount = visible.filter(image => image.matches('.content-preview img.image-preview')).length;
      const pendingImages = visible.filter(image => !image.complete || image.naturalWidth <= 0).length;
      return {ready:textPresent && previewCount >= 1 && pendingImages === 0,
        text_present:textPresent, visible_images:visible.length, visible_preview_images:previewCount,
        pending_images:pendingImages, document_state:document.readyState,
        visibility_state:document.visibilityState};
    })()""".replace("MARKER", json.dumps(marker))


class NodeCDP:
    def __init__(self, executable):
        self.events = queue.Queue()
        self.process = subprocess.Popen(
            [str(executable), "--input-type=module", "-e", NODE_TRANSPORT],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", creationflags=subprocess.CREATE_NO_WINDOW,
        )

        def receive():
            for line in self.process.stdout:
                try:
                    self.events.put((time.perf_counter(), json.loads(line)))
                except ValueError:
                    self.events.put((time.perf_counter(), {"type": "error", "message": "Invalid Node transport JSON"}))

        self.reader = threading.Thread(target=receive, daemon=True)
        self.reader.start()
        self.version = self.wait("boot", 5)[1]["version"]
        self.name = "node-websocket"
        self.next_id = 0

    def wait(self, kind, timeout, command_id=None):
        deadline = time.perf_counter() + timeout
        while time.perf_counter() < deadline:
            try:
                received, event = self.events.get(timeout=max(0.001, deadline - time.perf_counter()))
            except queue.Empty:
                break
            if event.get("type") == "error":
                raise RuntimeError(event.get("message", "Node WebSocket error"))
            if event.get("type") == kind and (command_id is None or event.get("value", {}).get("id") == command_id):
                return received, event
        raise TimeoutError(f"Node transport timed out waiting for {kind}")

    def send(self, request):
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()

    def connect(self, url):
        self.send({"op": "connect", "url": url})
        self.wait("connected", 1)

    def evaluate(self, expression):
        self.next_id += 1
        self.send({"op": "send", "value": evaluation_command(self.next_id, expression)})
        received, event = self.wait("message", 1, self.next_id)
        return received, evaluation_value(event["value"])

    def disconnect(self):
        if self.process.poll() is None:
            self.send({"op": "close"})
            self.wait("closed", 1)

    def close(self):
        if self.process.poll() is None:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=2)


class PythonCDP:
    name = "websocket-client"

    def __init__(self):
        self.socket = None
        self.next_id = 0
        self.version = websocket.__version__

    def connect(self, url):
        self.socket = websocket.create_connection(url, timeout=1, suppress_origin=True,
                                                   http_no_proxy=["localhost", "127.0.0.1"])

    def evaluate(self, expression):
        self.next_id += 1
        self.socket.send(json.dumps(evaluation_command(self.next_id, expression)))
        deadline = time.perf_counter() + 1
        while time.perf_counter() < deadline:
            response = json.loads(self.socket.recv())
            if response.get("id") == self.next_id:
                return time.perf_counter(), evaluation_value(response)
        raise TimeoutError("CDP evaluation timed out")

    def disconnect(self):
        if self.socket is not None:
            self.socket.close()
            self.socket = None

    close = disconnect


def evaluation_command(command_id, expression):
    return {"id": command_id, "method": "Runtime.evaluate",
            "params": {"expression": expression, "returnByValue": True, "silent": True}}


def evaluation_value(response):
    if "error" in response or "exceptionDetails" in response.get("result", {}):
        raise RuntimeError("Read-only CDP evaluation failed")
    value = response.get("result", {}).get("result", {}).get("value")
    if not isinstance(value, dict):
        raise RuntimeError("CDP evaluation returned no readiness state")
    return value


def checked_case(directory):
    directory = directory.resolve(strict=True)
    cases_root = CASES_ROOT.resolve(strict=True)
    if WORKSPACE_ROOT not in cases_root.parents or directory == cases_root or cases_root not in directory.parents:
        raise ValueError("Only workspace .harness/perf/cases destinations are allowed")
    case = json.loads((directory / "case.json").read_text(encoding="utf-8"))
    executable = Path(case["executable"]).resolve(strict=True)
    if directory not in executable.parents:
        raise ValueError("Case executable resolves outside the case directory")
    digest = hashlib.sha256(executable.read_bytes()).hexdigest()
    if digest != case["sha256"]:
        raise ValueError("Case executable differs from case.json; prepare the matching binary first")
    fixture = json.loads((directory / "history-fixture.json").read_text(encoding="utf-8"))
    if fixture.get("counts", {}).get("image", 0) < 1 or fixture.get("counts", {}).get("text", 0) < 1:
        raise ValueError("Seed both synthetic text and images before probing readiness")
    return directory, case, executable, fixture


def assert_stopped(executable):
    for process in psutil.process_iter(["name", "exe"]):
        if not process.info["exe"]:
            continue
        process_path = Path(process.info["exe"]).resolve()
        if process_path == executable or CASES_ROOT.resolve() in process_path.parents:
            raise ValueError(f"Stop existing benchmark process {process.pid} before probing")


def free_port(preferred):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as reservation:
        reservation.bind(("127.0.0.1", preferred))
        return reservation.getsockname()[1]


def remember_children(tracked):
    for parent in list(tracked.values()):
        try:
            if not parent.is_running():
                continue
            for child in parent.children(recursive=True):
                tracked[(child.pid, child.create_time())] = child
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue


def stop_owned(tracked):
    remember_children(tracked)
    pending = []
    for process in tracked.values():
        try:
            if process.is_running():
                process.terminate()
                pending.append(process)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    _, alive = psutil.wait_procs(pending, timeout=3)
    for process in alive:
        try:
            if process.is_running():
                process.kill()
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
    _, remaining = psutil.wait_procs(alive, timeout=2)
    return [process.pid for process in remaining if process.is_running()]


def main_target(targets, port):
    for target in targets:
        url = urlsplit(target.get("url", ""))
        if target.get("type") != "page" or parse_qs(url.query).get("window"):
            continue
        release_url = (url.scheme == "tauri" and url.hostname == "localhost") or (
            url.scheme in ("http", "https") and url.hostname == "tauri.localhost")
        if not release_url:
            continue
        websocket_url = urlsplit(target.get("webSocketDebuggerUrl", ""))
        if websocket_url.scheme != "ws" or websocket_url.hostname not in ("127.0.0.1", "localhost", "::1") or websocket_url.port != port:
            raise ValueError("CDP target does not belong to the dedicated localhost endpoint")
        return target
    return None


def create_transport(args):
    if args.transport != "node" and websocket is not None:
        return PythonCDP()
    if args.transport == "websocket-client":
        raise ValueError("websocket-client is not installed in this Python environment")
    node = args.node or shutil.which("node")
    if not node:
        raise ValueError("Install websocket-client or pass --node with a Node.js 22+ executable")
    return NodeCDP(node)


def trial(args, directory, executable, index, output_stem):
    assert_stopped(executable)
    profile = directory / "startup-profiles" / (
        f"{output_stem}-{index:02d}-{uuid.uuid4().hex[:8]}" if args.profile_mode == "fresh" else "reused")
    if directory not in profile.resolve().parents:
        raise ValueError("Browser profile resolves outside the case")
    profile_existed = profile.exists()
    profile.mkdir(parents=True, exist_ok=args.profile_mode == "reuse")
    environment = dict(os.environ)
    inherited = environment.get("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "")
    if "--remote-debugging" in inherited:
        raise ValueError("Remove inherited remote-debugging arguments before using the dedicated probe endpoint")
    port = free_port(args.port)
    environment["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
        f"{inherited} --remote-debugging-port={port} --remote-debugging-address=127.0.0.1").strip()
    environment["WEBVIEW2_USER_DATA_FOLDER"] = str(profile)
    environment["APPDATA"] = str(directory / "roaming")
    environment["LOCALAPPDATA"] = str(directory / "local")
    transport = create_transport(args)
    result = {"run": index, "status": "timeout", "profile": str(profile),
              "profile_existed": profile_existed, "port": port,
              "transport": transport.name, "transport_version": transport.version,
              "target_url": None, "ready_ms": None, "last_state": None,
              "first_text_ms": None, "first_preview_ms": None, "evaluations": 0}
    tracked = {}
    log_path = directory / f"{output_stem}.run-{index:02d}.stderr.log"
    opener = build_opener(ProxyHandler({}))
    connected = False
    start = None
    try:
        with log_path.open("wb") as log:
            start = time.perf_counter()
            process = subprocess.Popen([str(executable)], cwd=directory, env=environment,
                                       stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
            root = psutil.Process(process.pid)
            tracked[(root.pid, root.create_time())] = root
            result["pid"] = root.pid
            next_discovery = 0
            while time.perf_counter() - start < args.timeout:
                now = time.perf_counter()
                if now >= next_discovery:
                    remember_children(tracked)
                    next_discovery = now + 0.05
                if process.poll() is not None:
                    result["status"] = "application_exited"
                    result["exit_code"] = process.returncode
                    break
                try:
                    if not connected:
                        with opener.open(f"http://127.0.0.1:{port}/json/list", timeout=0.2) as response:
                            target = main_target(json.load(response), port)
                        if target:
                            transport.connect(target["webSocketDebuggerUrl"])
                            result["target_url"] = target["url"]
                            result["cdp_connected_ms"] = (time.perf_counter() - start) * 1000
                            connected = True
                    if connected:
                        received, state = transport.evaluate(readiness_expression(args.text_marker))
                        elapsed_ms = (received - start) * 1000
                        result["evaluations"] += 1
                        result["last_state"] = state
                        if state.get("text_present") and result["first_text_ms"] is None:
                            result["first_text_ms"] = elapsed_ms
                        if state.get("visible_preview_images", 0) >= 1 and result["first_preview_ms"] is None:
                            result["first_preview_ms"] = elapsed_ms
                        if state.get("ready"):
                            result["status"] = "ready"
                            result["ready_ms"] = elapsed_ms
                            transport.disconnect()
                            connected = False
                            result["debugger_disconnected_ms"] = (time.perf_counter() - start) * 1000
                            break
                except CDP_ERRORS as error:
                    result["last_probe_error"] = type(error).__name__ + ": " + str(error)[:180]
                    if connected:
                        try:
                            transport.disconnect()
                        except CDP_ERRORS:
                            pass
                    connected = False
                time.sleep(args.interval)
    except (OSError, ValueError, psutil.Error) as error:
        result["status"] = "error"
        result["error"] = str(error)
    finally:
        try:
            transport.close()
        finally:
            result["cleanup_remaining_pids"] = stop_owned(tracked)
            result["total_ms"] = (time.perf_counter() - start) * 1000 if start is not None else None
    return result


def run(args):
    directory, case, executable, fixture = checked_case(args.directory)
    output = (directory / args.output).resolve()
    if directory not in output.parents or output.suffix.lower() != ".json":
        raise ValueError("--output must be a JSON file inside the case directory")
    if output.exists():
        raise ValueError(f"Refusing to overwrite {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    artifact = {"case": case, "fixture": fixture,
                "method": "Read-only CDP fixture content readiness; not input latency or screen paint",
                "text_marker": args.text_marker, "profile_mode": args.profile_mode,
                "poll_interval_seconds": args.interval, "timeout_seconds": args.timeout,
                "debugging_overhead": "Local endpoint discovery, WebSocket connection and read-only layout/image evaluation are included; Node transport boot excluded",
                "os_file_cache": "Not flushed; binary hash verified before trials",
                "started_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "runs": []}
    for index in range(1, args.repeat + 1):
        result = trial(args, directory, executable, index, output.stem)
        artifact["runs"].append(result)
        ready = [run["ready_ms"] for run in artifact["runs"] if run["status"] == "ready"]
        artifact["successful_runs"] = len(ready)
        artifact["median_ready_ms"] = statistics.median(ready) if ready else None
        output.write_text(json.dumps(artifact, indent=2), encoding="utf-8")
        print(json.dumps({"artifact": str(output), **result}), flush=True)
        if result["status"] != "ready" or result["cleanup_remaining_pids"]:
            return 1
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", type=Path, help="Prepared, migrated and seeded case")
    parser.add_argument("--repeat", type=int, default=1, help="Sequential trials; default 1, use 3 for comparison")
    parser.add_argument("--output", default="startup-readiness.json", help="New JSON artifact within the case")
    parser.add_argument("--text-marker", default="Clipboard benchmark record 1000")
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--interval", type=float, default=0.02)
    parser.add_argument("--port", type=int, default=0, help="Dedicated localhost port; default selects an unused port")
    parser.add_argument("--profile-mode", choices=("fresh", "reuse"), default="fresh")
    parser.add_argument("--transport", choices=("auto", "websocket-client", "node"), default="auto")
    parser.add_argument("--node", type=Path, help="Node.js executable when websocket-client is unavailable")
    arguments = parser.parse_args()
    if os.name != "nt":
        parser.error("This startup probe requires Windows")
    if arguments.repeat < 1 or arguments.timeout <= 0 or arguments.interval <= 0 or not 0 <= arguments.port <= 65535:
        parser.error("Repeat, timeout and interval must be positive; port must be 0 through 65535")
    try:
        sys.exit(run(arguments))
    except (OSError, ValueError, KeyError, psutil.Error) as error:
        parser.exit(1, f"Cannot probe startup: {error}\n")
