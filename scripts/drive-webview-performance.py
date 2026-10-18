"""Drive explicitly authorized benchmark UI through CDP Input, without launching apps.

Requires an existing isolated case, its localhost CDP endpoint, and the separate
probe-webview-interaction.py observer. Runtime.evaluate only reads DOM/state;
all interaction uses Input.synthesizeScrollGesture or Input.dispatchMouseEvent.
No DOM click(), scrollTop assignment, OS input, framework, or product edit.
The prepared fixture uses Chinese header titles for its settings/tag buttons.

Run scroll-pilot to verify real motion before formal all workloads. Formal all
warms one scroll pair, then records four 4000px/800px-per-second gestures, first
settings/tag opens separately, and 20 warmed panel cycles. Every measured scene
waits for observer+10/+30 retention before the next. Native OS drag is excluded.
Use --combined for a counters-only replica with three phase boundaries inside
one scene and retention only after all actions. This still uses a debugger.
"""

import argparse
from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import threading
import time
from urllib.request import ProxyHandler, build_opener


SPEC = importlib.util.spec_from_file_location("interaction_probe", Path(__file__).with_name("probe-webview-interaction.py"))
OBSERVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OBSERVER)

STATE = r"""(() => {
 const visible = e => {if(!e)return false; const r=e.getBoundingClientRect();
   if(!(r.width>0&&r.height>0&&r.right>0&&r.bottom>0&&r.left<innerWidth&&r.top<innerHeight))return false;
   for(let p=e;p;p=p.parentElement){const s=getComputedStyle(p);if(s.visibility==='hidden'||s.display==='none'||Number(s.opacity)===0)return false;}return true;};
 const first = selectors => {for(const selector of selectors){const e=[...document.querySelectorAll(selector)].find(visible);if(e)return e;}return null;};
 const back=first(['header .header-back-btn','header .header-top > div:first-child button']);
 const scroller=first(['.virtual-list-wrapper [data-testid="virtuoso-scroller"]','[data-testid="virtuoso-scroller"]']);
 const box=e=>{if(!e)return null;const r=e.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
 return {url:location.href,visibility_state:document.visibilityState,viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio},
 settings_open:!!back&&!!first(['.settings-view .settings-layout','.settings-view .settings-panel-root']),
 tags_open:!!back&&!!first(['.themed-tag-manager','.tag-manager-page']),back_visible:!!back,
 scroller:scroller?{bounds:box(scroller),top:scroller.scrollTop,height:scroller.scrollHeight,client_height:scroller.clientHeight}:null};
})()"""


def read_json(path, timeout=5):
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            time.sleep(.05)
    raise RuntimeError(f"Cannot read {path.name}")


def evaluate(cdp, expression):
    _, response = cdp.command("Runtime.evaluate", {"expression": expression, "returnByValue": True, "silent": True})
    if "exceptionDetails" in response:
        raise RuntimeError("Read-only DOM evaluation failed")
    return response["result"]["value"]


class Driver:
    def __init__(self, args, directory, case, fixture, target):
        self.args = args
        self.directory = directory
        self.profile = directory / args.observer_output
        self.output = OBSERVER.safe_output(directory, args.output)
        if self.output.exists():
            raise ValueError("Refusing to overwrite driver results")
        self.input = OBSERVER.CDP(args.node)
        self.reader = OBSERVER.CDP(args.node)
        self.input.connect(target["webSocketDebuggerUrl"])
        self.reader.connect(target["webSocketDebuggerUrl"])
        self.artifact = {"case": case, "fixture_logical_sha256": fixture["logical_sha256"],
                         "started_utc": datetime.now(timezone.utc).isoformat(), "actions": [], "errors": [],
                         "protocol": {"distance_css_px": args.distance, "speed_css_px_per_second": args.speed,
                            "scroll_pairs": args.pairs, "panel_cycles": args.cycles,
                            "open_hold_ms": args.open_ms, "closed_hold_ms": args.closed_ms,
                            "combined_scene": args.combined},
                         "method": "CDP Input browser pipeline; DOM reads only; no OS input or application launch",
                         "sources": ["https://chromedevtools.github.io/devtools-protocol/tot/Input/",
                                     "https://playwright.dev/docs/webview2"],
                         "limits": ["DOM state readiness differs from complete asynchronous panel content readiness.",
                             "Polling scroller/visible state adds instrumentation work, identical across cases.",
                             "No native OS window dragging or DWM frame rate measurement."]}
        if args.combined:
            self.reader.command("Performance.enable", {"timeDomain": "timeTicks"})
            self.artifact["phases"] = []

    def save(self):
        temporary = self.output.with_suffix(".pending.json")
        temporary.write_text(json.dumps(self.artifact, indent=2) + "\n", encoding="utf-8")
        for attempt in range(20):
            try:
                temporary.replace(self.output)
                return
            except PermissionError:
                if attempt == 19:
                    raise
                time.sleep(.05)

    def state(self):
        return evaluate(self.reader, STATE)

    def scene(self, name, phase):
        if phase == "start":
            profile = read_json(self.profile)
            windows = OBSERVER.native_main_windows(profile["pid"])
            if not any(w["visible"] and w["topmost"] for w in windows) or self.state()["visibility_state"] != "visible":
                raise RuntimeError("Formal scene requires visible topmost native main and visible document")
        OBSERVER.mark(argparse.Namespace(directory=self.directory, output=self.args.observer_output,
            scenario=name, phase=phase, note="Authorized CDP Input automation; separate from earlier Sky workloads"))
        deadline = time.perf_counter() + 90
        while time.perf_counter() < deadline:
            data = read_json(self.profile)
            matches = [s for s in data["scenarios"] if s["scenario"] == name]
            if matches and ((phase == "start" and "before" in matches[-1]) or
                            (phase == "end" and "after" in matches[-1])):
                if phase == "end":
                    summary = matches[-1].get("process_summary")
                    if summary is None:
                        time.sleep(.05)
                        continue
                    if not summary.get("native_main_visible_all_samples") or not summary.get("native_main_topmost_all_samples"):
                        raise RuntimeError("Hidden or non-topmost interval invalidates formal scene")
                return
            time.sleep(.05)
        raise RuntimeError(f"Observer did not acknowledge {phase} {name}")

    def retained(self, name):
        deadline = time.perf_counter() + 75
        while time.perf_counter() < deadline:
            data = read_json(self.profile)
            scene = next(s for s in data["scenarios"] if s["scenario"] == name)
            if "30" in scene.get("post_action_snapshots", {}):
                return
            time.sleep(.1)
        raise RuntimeError(f"Retention snapshots missing for {name}")

    def scroll(self, distance):
        before = self.state()
        scroll = before["scroller"]
        if not scroll:
            raise RuntimeError("Visible list scroller absent")
        box = scroll["bounds"]
        parameters = {"x": (max(0, box["left"]) + min(before["viewport"]["width"], box["right"])) / 2,
                      "y": (max(0, box["top"]) + min(before["viewport"]["height"], box["bottom"])) / 2,
                      "yDistance": distance, "speed": self.args.speed,
                      "gestureSourceType": "mouse", "preventFling": True}
        result = {}
        started = time.perf_counter()
        epoch = time.time() * 1000

        def perform():
            try:
                self.input.command("Input.synthesizeScrollGesture", parameters,
                                   timeout=abs(distance) / self.args.speed + 15)
            except Exception as error:
                result["error"] = repr(error)

        worker = threading.Thread(target=perform)
        worker.start()
        samples = []
        while worker.is_alive():
            current = self.state()
            samples.append({"epoch_ms": time.time() * 1000, "scroll": current["scroller"]})
            time.sleep(.1)
        worker.join()
        after = self.state()
        changed = [s for s in samples if s["scroll"] and abs(s["scroll"]["top"] - scroll["top"]) > 1]
        distinct = {round(s["scroll"]["top"], 1) for s in changed}
        action = {"kind": "scroll", "epoch_ms": epoch, "parameters": parameters,
                  "elapsed_ms": (time.perf_counter() - started) * 1000,
                  "before": before, "after": after, "samples": samples,
                  "distinct_changed_positions": len(distinct),
                  "observed_motion_span_ms": changed[-1]["epoch_ms"] - changed[0]["epoch_ms"] if changed else 0,
                  "actual_top_delta": after["scroller"]["top"] - scroll["top"] if after["scroller"] else None,
                  **result}
        action["motion_verified"] = len(distinct) >= 3 and action["actual_top_delta"] is not None and abs(action["actual_top_delta"]) > 100
        self.artifact["actions"].append(action)
        self.save()
        print(json.dumps({k: action[k] for k in ("kind", "elapsed_ms", "observed_motion_span_ms", "actual_top_delta", "motion_verified")}), flush=True)
        if result or not action["motion_verified"]:
            raise RuntimeError("Synthetic scroll did not produce verified motion")

    def click(self, panel, opening):
        selectors = [f'header button[title="{("设置" if panel == "settings" else "标签管理")}"]'] if opening else [
            'header .header-back-btn', 'header .header-top > div:first-child button']
        expression = """(() => {for(const selector of SELECTORS){for(const e of document.querySelectorAll(selector)){
          const r=e.getBoundingClientRect(); const left=Math.max(0,r.left),right=Math.min(innerWidth,r.right);
          const top=Math.max(0,r.top),bottom=Math.min(innerHeight,r.bottom);if(right<=left||bottom<=top)continue;
          const x=(left+right)/2,y=(top+bottom)/2;const hit=document.elementFromPoint(x,y);
          if(hit&&(hit===e||e.contains(hit))&&!e.disabled)return {x,y,selector};}}
          return null;})()""".replace("SELECTORS", json.dumps(selectors))
        point = evaluate(self.reader, expression)
        if not point:
            raise RuntimeError(f"Visible unoccluded {panel} {'open' if opening else 'back'} button absent")
        started = time.perf_counter()
        epoch = time.time() * 1000
        self.input.command("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": point["x"], "y": point["y"]})
        for kind, buttons in (("mousePressed", 1), ("mouseReleased", 0)):
            self.input.command("Input.dispatchMouseEvent", {"type": kind, "x": point["x"], "y": point["y"],
                "button": "left", "buttons": buttons, "clickCount": 1})
        deadline = time.perf_counter() + 5
        while time.perf_counter() < deadline:
            current = self.state()
            if bool(current[panel + "_open"]) == opening and (opening or (current["scroller"] and not current["back_visible"])):
                break
            time.sleep(.01)
        else:
            raise RuntimeError(f"{panel} did not reach expected state")
        action = {"kind": "click", "panel": panel, "opening": opening, "epoch_ms": epoch,
                  "point": point, "dom_state_ready_ms": (time.perf_counter() - started) * 1000,
                  "after": current}
        self.artifact["actions"].append(action)
        self.save()
        hold = self.args.open_ms if opening else self.args.closed_ms
        time.sleep(max(0, hold / 1000 - (time.perf_counter() - started)))

    def scroll_sequence(self):
        for _ in range(self.args.pairs):
            self.scroll(-self.args.distance)
            self.scroll(self.args.distance)

    def panel_sequence(self, panel):
        self.click(panel, True)
        self.click(panel, False)
        for _ in range(self.args.cycles):
            self.click(panel, True)
            self.click(panel, False)

    def capture_phase(self, name, operation):
        before = OBSERVER.metric_snapshot(self.reader)
        phase = {"name": name, "before": before, "state_before": self.state(),
                 "action_index_start": len(self.artifact["actions"]),
                 "capture_start_epoch_ms": time.time() * 1000}
        operation()
        phase["capture_end_epoch_ms"] = time.time() * 1000
        phase["state_after"] = self.state()
        phase["main_content_ready_after"] = evaluate(self.reader,
            OBSERVER.PROBE.readiness_expression("Clipboard benchmark record 1000"))
        phase["after"] = OBSERVER.metric_snapshot(self.reader)
        phase["action_index_end"] = len(self.artifact["actions"])
        phase["counter_deltas"] = {key: phase["after"]["metrics"].get(key, 0) - before["metrics"].get(key, 0)
                                   for key in OBSERVER.COUNTERS}
        self.artifact["phases"].append(phase)
        self.save()
        print(json.dumps({"event": "combined_phase_ended", "name": name,
                          "counter_deltas": phase["counter_deltas"]}), flush=True)

    def run(self):
        try:
            if self.args.workload == "scroll-pilot":
                self.scene("auto-scroll-pilot", "start")
                self.scroll(-self.args.distance)
                self.scroll(self.args.distance)
                self.scene("auto-scroll-pilot", "end")
                self.retained("auto-scroll-pilot")
            else:
                self.scroll(-800)
                self.scroll(800)
                if self.args.combined:
                    self.scene("auto-combined-interaction", "start")
                    self.capture_phase("continuous-scroll", self.scroll_sequence)
                    for panel in ("settings", "tags"):
                        self.capture_phase(panel + "-first-plus-20-cycles", lambda panel=panel: self.panel_sequence(panel))
                    self.scene("auto-combined-interaction", "end")
                    self.retained("auto-combined-interaction")
                else:
                    self.scene("auto-continuous-scroll", "start")
                    self.scroll_sequence()
                    self.scene("auto-continuous-scroll", "end")
                    self.retained("auto-continuous-scroll")
                    for panel in ("settings", "tags"):
                        repeated = "auto-" + panel + "-first-plus-20-cycles"
                        self.scene(repeated, "start")
                        self.panel_sequence(panel)
                        self.scene(repeated, "end")
                        self.retained(repeated)
            self.artifact["completed"] = True
        except Exception as error:
            self.artifact["errors"].append(repr(error))
            raise
        finally:
            if self.args.combined:
                try:
                    self.reader.command("Performance.disable")
                except Exception:
                    pass
            for cdp in (self.input, self.reader):
                try:
                    cdp.disconnect()
                except Exception:
                    pass
                cdp.close()
            self.artifact["cleanup_node_exit_codes"] = [self.input.process.poll(), self.reader.process.poll()]
            self.artifact["finished_utc"] = datetime.now(timezone.utc).isoformat()
            self.save()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--node", type=Path, required=True)
    parser.add_argument("--workload", choices=("scroll-pilot", "all"), default="all")
    parser.add_argument("--combined", action="store_true", help="One counters-only scene, three phase boundaries, final retention only")
    parser.add_argument("--observer-output", default="interaction-profile.auto.json")
    parser.add_argument("--output", default="interaction-driver.auto.json")
    parser.add_argument("--distance", type=float, default=4000)
    parser.add_argument("--speed", type=int, default=800)
    parser.add_argument("--pairs", type=int, default=2)
    parser.add_argument("--cycles", type=int, default=20)
    parser.add_argument("--open-ms", type=int, default=200)
    parser.add_argument("--closed-ms", type=int, default=200)
    args = parser.parse_args()
    if args.distance <= 100 or args.speed <= 0 or args.pairs < 1 or args.cycles < 1 or min(args.open_ms, args.closed_ms) < 0:
        parser.error("Positive scroll distance/speed/counts and nonnegative hold durations are required")
    if args.combined and args.workload != "all":
        parser.error("Combined mode requires all workloads")
    directory, case, executable, fixture = OBSERVER.PROBE.checked_case(args.directory)
    with build_opener(ProxyHandler({})).open(f"http://127.0.0.1:{args.port}/json/list", timeout=2) as response:
        target = OBSERVER.PROBE.main_target(json.load(response), args.port)
    if not target:
        parser.error("Release main-page target absent")
    Driver(args, directory, case, fixture, target).run()
