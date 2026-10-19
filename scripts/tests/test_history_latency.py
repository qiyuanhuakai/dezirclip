"""Keep the performance threshold and comparison preconditions explicit."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("history_latency", Path(__file__).resolve().parents[1] / "sample-history-latency.py")
SAMPLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SAMPLER)


def report(p95):
    return {"host": "fixture-host", "machine_id": "fixture-machine", "rows": 1000, "iterations": 30,
            "scope": "fixture-query", "build_profile": "debug", "target": "fixture-target",
            "fixture_version": 1, "source_revision": "fixture-revision",
            "results": {"list": {"p50_ms": p95 / 2, "p95_ms": p95}}}


class HistoryLatencyTests(unittest.TestCase):
    def test_threshold_accepts_boundary_and_rejects_excess(self):
        for baseline, allowed in [(100, 120), (1000, 1100)]:
            self.assertTrue(SAMPLER.compare_reports(report(baseline), report(allowed))["passed"])
            self.assertFalse(SAMPLER.compare_reports(report(baseline), report(allowed + .01))["passed"])

    def test_incomparable_environment_and_workload_are_rejected(self):
        for field, value in [("host", "another-host"), ("machine_id", "another-machine"), ("build_profile", "release"),
                             ("rows", 100), ("target", "another-target"), ("fixture_version", 2)]:
            current = report(100)
            current[field] = value
            with self.assertRaises(ValueError):
                SAMPLER.compare_reports(report(100), current)

    def test_missing_provenance_and_short_runs_do_not_claim_acceptance(self):
        for field, value in [("iterations", 10), ("build_profile", "unspecified"),
                             ("source_revision", None), ("target", "")]:
            baseline = report(100)
            baseline[field] = value
            with self.assertRaises(ValueError):
                SAMPLER.compare_reports(baseline, copy.deepcopy(baseline))

    def test_write_workload_size_and_fixture_must_match(self):
        for field in ["payload_bytes", "fixture_sha256"]:
            current = report(100)
            current[field] = "changed fixture"
            with self.assertRaises(ValueError):
                SAMPLER.compare_reports(report(100), current)


if __name__ == "__main__":
    unittest.main()
