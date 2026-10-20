"""Synthetic comparator tests; values here are not measured performance evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("compare_capture", Path(__file__).resolve().parents[1] / "compare_capture.py")
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)


def report(p95):
    value = {"rows": 1000, "iterations": 50, "warmups": 5, "fixture_version": 1,
            "verification_only": False, "build": {"profile": "debug", "target": "fixture",
            "rustc": "fixture", "cargo": "fixture", "environment": {}, "machine_id": "fixture",
            "template_sha256": {"main.rs": "fixture"}}, "run": {"host": "fixture"},
            "results": {"ordinary_new": {"postconditions": {"body_roundtrip": True, "html_roundtrip": True,
            "identity_preserved": True, "protected": False, "fts_visible": True, "id": 1001, "row_count": 1001},
            "metrics": {"common_store_total_ms": {"p50_ms": p95 / 2, "p95_ms": p95,
            "samples_ms": [p95 / 2] * 47 + [p95] * 3}}}}}
    ordinary = value["results"]["ordinary_new"]
    for name in COMPARE.CASES - {"ordinary_new"}:
        case = copy.deepcopy(ordinary)
        if name.startswith("sensitive"):
            case["postconditions"].update(protected=True, fts_visible=False)
        if name == "sensitive_conversion":
            case["metrics"] = {"conversion_commit_ms": case["metrics"]["common_store_total_ms"]}
        value["results"][name] = case
    return value


class ComparatorTests(unittest.TestCase):
    def test_both_branches_of_threshold_and_exact_boundary(self):
        for before, limit in [(100, 120), (1000, 1100)]:
            self.assertTrue(COMPARE.compare(report(before), report(limit))["passed_for_common_store_boundaries"])
            self.assertFalse(COMPARE.compare(report(before), report(limit + .01))["passed_for_common_store_boundaries"])

    def test_common_store_pass_never_claims_complete_capture_acceptance(self):
        result = COMPARE.compare(report(100), report(100))
        self.assertFalse(result["complete_capture_acceptance"])

    def test_invalid_payload_identity_and_fts_cannot_pass_fast_timing(self):
        for field, value in [("body_roundtrip", False), ("identity_preserved", False),
                             ("fts_visible", False), ("row_count", 1000), ("id", -1)]:
            current = report(1)
            current["results"]["ordinary_new"]["postconditions"][field] = value
            with self.assertRaises(ValueError):
                COMPARE.compare(report(100), current)

    def test_build_workload_and_verification_only_differences_reject(self):
        baseline = report(100)
        for changed in [dict(baseline, verification_only=True), dict(baseline, rows=10000)]:
            with self.assertRaises(ValueError):
                COMPARE.compare(baseline, changed)

    def test_missing_case_raw_samples_and_inaccurate_summary_reject(self):
        baseline = report(100)
        changed = copy.deepcopy(baseline)
        del changed["results"]["rich_new"]
        with self.assertRaises(ValueError):
            COMPARE.compare(baseline, changed)
        for field, value in [("samples_ms", [1] * 49), ("samples_ms", [float("nan")] * 50),
                             ("samples_ms", [-1] * 50), ("p95_ms", 1), ("p50_ms", 1)]:
            changed = copy.deepcopy(baseline)
            changed["results"]["ordinary_new"]["metrics"]["common_store_total_ms"][field] = value
            with self.assertRaises(ValueError):
                COMPARE.compare(baseline, changed)
        for field in baseline["build"]:
            changed = copy.deepcopy(baseline)
            changed["build"][field] = "different"
            with self.assertRaises(ValueError):
                COMPARE.compare(baseline, changed)


if __name__ == "__main__":
    unittest.main()
