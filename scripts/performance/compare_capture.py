"""Compare only equivalent measured store boundaries, never legacy-vs-engine totals."""
import argparse
import json
import math
from pathlib import Path
import statistics

CASES = {"ordinary_new", "ordinary_duplicate", "rich_new", "sensitive_new",
         "sensitive_duplicate", "sensitive_conversion"}


def measured_summary(metric, iterations):
    samples = metric["samples_ms"]
    if len(samples) != iterations or not all(math.isfinite(value) and value >= 0 for value in samples):
        raise ValueError("Missing, nonfinite or negative raw timing samples")
    ordered = sorted(samples)
    p50 = statistics.median(ordered)
    p95 = ordered[math.ceil(iterations * .95) - 1]
    if not math.isclose(metric["p50_ms"], p50, rel_tol=1e-12, abs_tol=1e-12) or not math.isclose(metric["p95_ms"], p95, rel_tol=1e-12, abs_tol=1e-12):
        raise ValueError("Timing summary does not match its complete raw samples")
    return p50, p95


def compare(baseline, candidate):
    for key in ["rows", "iterations", "warmups", "fixture_version"]:
        if baseline.get(key) != candidate.get(key) or key not in baseline:
            raise ValueError(f"Incomparable fixture or sampling field: {key}")
    if baseline["iterations"] < 30 or baseline["verification_only"] or candidate["verification_only"]:
        raise ValueError("Functional checks and short samples cannot establish a performance gate")
    for key in ["profile", "target", "rustc", "cargo", "environment", "machine_id", "template_sha256"]:
        if baseline["build"][key] != candidate["build"][key]:
            raise ValueError(f"Incomparable build or host field: {key}")
    if baseline["run"]["host"] != candidate["run"]["host"]:
        raise ValueError("Host environment differs")
    if set(baseline["results"]) != CASES or set(candidate["results"]) != CASES:
        raise ValueError("All six documented cases are required")
    comparisons = {}
    for name, reference in baseline["results"].items():
        observed = candidate["results"][name]
        for case in [reference, observed]:
            checked = case["postconditions"]
            if not all(checked[field] for field in ["body_roundtrip", "html_roundtrip", "identity_preserved"]):
                raise ValueError(f"Failed payload or identity postcondition: {name}")
            if checked["id"] <= 0 or checked["row_count"] != baseline["rows"] + 1:
                raise ValueError(f"Failed row-count or stored-ID postcondition: {name}")
            protected = name.startswith("sensitive")
            if checked["protected"] != protected or checked["fts_visible"] == protected:
                raise ValueError(f"Failed protection/FTS postcondition: {name}")
        metric = "conversion_commit_ms" if name == "sensitive_conversion" else "common_store_total_ms"
        before = reference["metrics"][metric]
        after = observed["metrics"][metric]
        before_p50, before_p95 = measured_summary(before, baseline["iterations"])
        after_p50, after_p95 = measured_summary(after, candidate["iterations"])
        threshold = before_p95 + max(before_p95 * .1, 20.0)
        comparisons[name] = {"metric": metric, "baseline_p50_ms": before_p50,
                             "candidate_p50_ms": after_p50, "baseline_p95_ms": before_p95,
                             "candidate_p95_ms": after_p95, "allowed_p95_ms": threshold,
                             "passed": after_p95 <= threshold}
    return {"scope": "Same production store dedup+commit boundaries; conversion method entry-to-commit",
            "rule": "p95 <= baseline p95 + max(baseline p95 * 10%, 20ms)", "cases": comparisons,
            "passed_for_common_store_boundaries": all(case["passed"] for case in comparisons.values()),
            "complete_capture_acceptance": False,
            "limits": ["Candidate Engine total and preparation are separate scopes",
                       "No equivalent legacy AppHandle pipeline, reader, listener, GUI or queue measurement"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Preserve existing comparison artifacts")
    try:
        result = compare(json.loads(args.baseline.read_text(encoding="utf-8")), json.loads(args.candidate.read_text(encoding="utf-8")))
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        if not result["passed_for_common_store_boundaries"]:
            raise SystemExit(1)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"Cannot compare capture evidence: {error}\n")
