from contextlib import contextmanager
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "summarize-webview-interaction.py"
SPEC = importlib.util.spec_from_file_location("webview_interaction_summary", SCRIPT)
SUMMARY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUMMARY)


class InteractionSummaryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="webview-summary-tests-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name).resolve()
        self.profile = self.directory / "profile.json"

    def write_profile(self, **changes):
        source = {
            "case": {"sha256": "original-executable", "label": "原始快照"},
            "fixture": {"logical_sha256": "fixture-hash"},
            "scenarios": [{"scenario": "pending"}],
            "process_samples": [],
        }
        source.update(changes)
        contents = (json.dumps(source, ensure_ascii=False) + "\n").encode("utf-8")
        self.profile.write_bytes(contents)
        return source, contents

    def test_ordinary_summary_uses_the_profile_contents(self):
        source, contents = self.write_profile(finished_utc="2026-10-18T00:00:00Z")

        result = SUMMARY.summarize(self.profile)

        self.assertEqual(result["case"], source["case"])
        self.assertEqual(result["profile_finished_utc"], source["finished_utc"])
        self.assertEqual(result["fixture_logical_sha256"], "fixture-hash")
        self.assertEqual(result["scenarios"], [{"scenario": "pending", "status": "incomplete"}])
        self.assertEqual(result["profile_sha256_at_summary"], hashlib.sha256(contents).hexdigest())

    def test_atomic_replacement_after_read_keeps_data_and_hash_on_one_snapshot(self):
        source, contents = self.write_profile()
        replacement = self.directory / "profile.pending.json"
        replacement_contents = json.dumps({
            **source,
            "case": {"sha256": "replacement-executable"},
            "scenarios": [],
            "finished_utc": "2026-10-18T00:00:01Z",
        }).encode("utf-8")
        replacement.write_bytes(replacement_contents)
        original_open = Path.open
        reads = []

        @contextmanager
        def replace_after_first_read(path, *args, **kwargs):
            with original_open(path, *args, **kwargs) as stream:
                yield stream
            if path == self.profile:
                reads.append(path)
                if len(reads) == 1:
                    replacement.replace(self.profile)

        with patch.object(Path, "open", replace_after_first_read):
            result = SUMMARY.summarize(self.profile)

        self.assertEqual(result["case"], source["case"])
        self.assertIsNone(result["profile_finished_utc"])
        self.assertEqual(result["scenarios"], [{"scenario": "pending", "status": "incomplete"}])
        self.assertEqual(result["profile_sha256_at_summary"], hashlib.sha256(contents).hexdigest())
        self.assertEqual(reads, [self.profile])
        self.assertEqual(self.profile.read_bytes(), replacement_contents)

    def test_trace_content_does_not_replace_the_profile_hash(self):
        trace = self.directory / "trace.json"
        trace_contents = json.dumps({"traceEvents": [
            {"name": "EventTiming", "ph": "b", "pid": 1, "tid": 2, "id": 3, "ts": 1000,
             "args": {"data": {"frame": "main-frame", "type": "click", "interactionId": 4,
                                "duration": 2, "timeStamp": 1, "processingStart": 1.2, "processingEnd": 1.7}}},
            {"name": "EventTiming", "ph": "e", "pid": 1, "tid": 2, "id": 3, "ts": 3000},
        ]}).encode("utf-8")
        trace.write_bytes(trace_contents)
        _, contents = self.write_profile(scenarios=[{
            "scenario": "click", "start_marker": {}, "counter_deltas": {},
            "gauges_before_after": {}, "process_summary": {},
            "capture_start_epoch_ms": 0, "capture_end_epoch_ms": 10,
            "before": {"main_frame": {"id": "main-frame"}}, "after": {},
            "trace": {"valid": True, "path": str(trace),
                      "sha256": hashlib.sha256(trace_contents).hexdigest(),
                      "capture_trace_bounds_us": [0, 10000]},
        }])

        result = SUMMARY.summarize(self.profile)

        self.assertEqual(result["profile_sha256_at_summary"], hashlib.sha256(contents).hexdigest())
        self.assertEqual(result["scenarios"][0]["interaction_count"], 1)
        self.assertEqual(result["scenarios"][0]["interaction_duration_ms"], {"median": 2, "max": 2})


if __name__ == "__main__":
    unittest.main()
