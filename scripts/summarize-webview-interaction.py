"""Summarize saved interaction traces without connecting to or controlling any app.

Pairs target-frame EventTiming begin/end events using the public DevTools trace
model, groups events by interactionId and reports per-interaction durations.
This is browser Event Timing, not proof that an asynchronously populated panel
has finished loading. Wheel dispatch/scroll clusters remain separate because
wheel events are not click/keyboard interactions with nonzero interactionId.

Example: python scripts/summarize-webview-interaction.py CASE/interaction-profile-v2.json
The derived .analysis.json can be regenerated while or after an observer runs;
the original profile/trace files are never modified. No Node/app is launched.
For a completed --combined counters-only run, add --driver DRIVER.json to
derive process peaks/CPU within each recorded phase. Requires psutil then.
"""

import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import statistics


CASES = Path(__file__).resolve().parents[1] / ".harness" / "perf" / "cases"
SOURCE = "https://raw.githubusercontent.com/ChromeDevTools/devtools-frontend/main/front_end/models/trace/handlers/UserInteractionsHandler.ts"


def interaction_events(events, frame_id, capture_bounds):
    pending = defaultdict(list)
    interactions = defaultdict(list)
    for event in sorted(events, key=lambda e: e.get("ts", 0)):
        if event.get("name") != "EventTiming":
            continue
        identity = (event.get("pid"), event.get("tid"), event.get("id"))
        if event.get("ph") == "b":
            data = event.get("args", {}).get("data", {})
            if data.get("frame", event.get("args", {}).get("frame")) == frame_id:
                pending[identity].append(event)
        elif event.get("ph") == "e" and pending[identity]:
            begin = pending[identity].pop()
            data = begin["args"]["data"]
            interaction_id = data.get("interactionId", 0)
            if not interaction_id or data.get("duration", 0) < 1:
                continue
            if not capture_bounds[0] <= begin["ts"] <= event["ts"] <= capture_bounds[1]:
                continue
            if not all(key in data for key in ("timeStamp", "processingStart", "processingEnd")):
                continue
            duration = (event["ts"] - begin["ts"]) / 1000
            input_delay = data["processingStart"] - data["timeStamp"]
            processing = data["processingEnd"] - data["processingStart"]
            presentation = duration - (data["processingEnd"] - data["timeStamp"])
            interactions[interaction_id].append({
                "type": data.get("type"), "trace_start_us": begin["ts"],
                "trace_end_us": event["ts"], "duration_ms": duration,
                "input_delay_ms": input_delay, "processing_duration_ms": processing,
                "presentation_delay_ms": presentation, "reported_duration_ms": data["duration"],
                "components_nonnegative_with_1ms_tolerance": min(input_delay, processing, presentation) >= -1})
    result = []
    for identity, events_for_id in interactions.items():
        worst = max(events_for_id, key=lambda e: e["duration_ms"])
        result.append({"interaction_id": identity, "event_types": sorted({e["type"] for e in events_for_id}),
                       "longest_event": worst, "events": events_for_id})
    return sorted(result, key=lambda e: e["longest_event"]["trace_start_us"])


def summarize(profile, driver=None):
    source = json.loads(profile.read_text(encoding="utf-8"))
    scenarios = []
    all_samples = source.get("process_samples", [])
    for scenario in source["scenarios"]:
        if "after" not in scenario:
            scenarios.append({"scenario": scenario["scenario"], "status": "incomplete"})
            continue
        trace = scenario.get("trace", {})
        row = {"scenario": scenario["scenario"], "start_note": scenario["start_marker"].get("note"),
               "end_note": scenario.get("end_marker", {}).get("note"),
               "counter_deltas": scenario["counter_deltas"],
               "gauges_before_after": scenario["gauges_before_after"],
               "process_summary": scenario["process_summary"],
               "trace_valid": trace.get("valid", False),
               "native_window_motion_epoch_ms": trace.get("native_window_motion_epoch_ms"),
               "input_activity_windows": trace.get("input_correlated_activity_windows"),
               "long_tasks_over_50ms": trace.get("long_tasks_over_50ms"),
               "long_task_excess_over_50ms": trace.get("long_task_excess_over_50ms"),
               "draw_cadence_proxy": trace.get("main_layer_tree_draw_cadence"),
               "post_action_snapshots": scenario.get("post_action_snapshots", {}),
               "presented_fps": None}
        selected = [s for s in all_samples if scenario["capture_start_epoch_ms"] <= s["epoch_ms"] <= scenario["capture_end_epoch_ms"]]
        gaps = [b["epoch_ms"] - a["epoch_ms"] for a, b in zip(selected, selected[1:])]
        row["observed_process_sample_gaps_ms"] = {"median": statistics.median(gaps) if gaps else None,
                                                   "max": max(gaps) if gaps else None}
        row["native_window_start_end"] = [selected[0]["native_main_windows"], selected[-1]["native_main_windows"]] if selected else None
        if trace.get("valid"):
            path = Path(trace["path"]).resolve()
            if profile.parent not in path.parents:
                raise ValueError("Trace path resolves outside its case")
            contents = path.read_bytes()
            if hashlib.sha256(contents).hexdigest() != trace["sha256"]:
                raise ValueError("Trace differs from recorded hash")
            events = json.loads(contents)["traceEvents"]
            recorded = interaction_events(events, scenario["before"]["main_frame"]["id"],
                                          trace["capture_trace_bounds_us"])
            row["browser_event_timing_interactions"] = recorded
            row["interaction_count"] = len(recorded)
            durations = [e["longest_event"]["duration_ms"] for e in recorded]
            row["interaction_duration_ms"] = {"median": statistics.median(durations) if durations else None,
                                               "max": max(durations) if durations else None}
        scenarios.append(row)
    result = {"schema_version": 1, "generated_utc": datetime.now(timezone.utc).isoformat(),
            "profile": str(profile), "profile_sha256_at_summary": hashlib.sha256(profile.read_bytes()).hexdigest(),
            "profile_finished_utc": source.get("finished_utc"), "case": source["case"],
            "fixture_logical_sha256": source["fixture"]["logical_sha256"],
            "browser_version": source.get("browser_version"), "scenarios": scenarios,
            "method_source": SOURCE,
            "limits": ["EventTiming b/e events are paired by trace id and target frame; events with interactionId0 or duration below1ms are excluded, matching the official DevTools interaction model.",
                       "The longest event per interactionId represents one browser-reported interaction. Input, processing and presentation components are not equivalent to fully populated panel readiness.",
                       "Browser Event Timing can use a fallback end time; do not describe it as independently measured screen presentation or a full INP evaluation.",
                       "Wheel/scroll bursts and native-window dragging are not converted into click interaction metrics.",
                       "Manual/tool idle is excluded from input clusters, but process totals over a scene still include idle/background work. Sampled process peaks can miss shorter transients.",
                       "Trace validity, actual sampling gaps, late retention snapshots and failed calibration notes remain explicit. No screen FPS is reported."]}
    if driver:
        import importlib.util
        specification = importlib.util.spec_from_file_location("interaction_observer_summary", Path(__file__).with_name("probe-webview-interaction.py"))
        observer = importlib.util.module_from_spec(specification)
        specification.loader.exec_module(observer)
        driven = json.loads(driver.read_text(encoding="utf-8"))
        if not source.get("finished_utc") or not driven.get("completed"):
            raise ValueError("Phase summaries require completed observer and driver artifacts")
        if driven["case"]["sha256"] != source["case"]["sha256"] or driven["fixture_logical_sha256"] != source["fixture"]["logical_sha256"]:
            raise ValueError("Driver and observer executable/fixture differ")
        result["driver"] = str(driver)
        result["driver_sha256"] = hashlib.sha256(driver.read_bytes()).hexdigest()
        result["phases"] = []
        for phase in driven.get("phases", []):
            result["phases"].append({**phase,
                "process_summary": observer.process_summary(all_samples, phase["capture_start_epoch_ms"], phase["capture_end_epoch_ms"])})
        result["limits"].append("Combined phases have separate process/counter boundaries; +10/+30 retention applies only after the combined scene, not after each phase. CDP and polling remain enabled even when tracing is off.")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("profile", type=Path)
    parser.add_argument("--driver", type=Path)
    args = parser.parse_args()
    profile = args.profile.resolve(strict=True)
    if CASES.resolve() not in profile.parents or profile.suffix != ".json":
        parser.error("Only a saved profile under workspace .harness/perf/cases is supported")
    driver = args.driver.resolve(strict=True) if args.driver else None
    if driver and (driver.parent != profile.parent or driver.suffix != ".json"):
        parser.error("Driver artifact must be a JSON file in the same case as the profile")
    output = profile.with_name(profile.stem + ".analysis.json")
    output.write_text(json.dumps(summarize(profile, driver), indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(output)}))
