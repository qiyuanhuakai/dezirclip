from pathlib import Path
import json
import re
import unittest
from unittest.mock import patch

from scripts import ci_policy

SHA = "1" * 40
RUN = 217
ATTEMPT = 2


def finished(mode, *, legacy_installer=False, prefix=""):
    return [
        dict(name=prefix + name, head_sha=SHA, run_id=RUN, run_attempt=ATTEMPT,
             status="completed", conclusion="success",
             steps=[dict(name=step, status="completed", conclusion="success") for step in steps])
        for name, steps in ci_policy.job_requirements(mode, legacy_installer).items()
    ]


class ModeTests(unittest.TestCase):
    def test_automatic_and_reusable_defaults(self):
        for event in ("push", "pull_request"):
            self.assertEqual(ci_policy.resolve_mode(event, {}), ("development", "release"))
        self.assertEqual(ci_policy.resolve_mode("push", {"source_acceptance": True, "full_validation": False}),
                         ("source-acceptance", "release"))
        self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {}), ("full", "release"))
        self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {"full_validation": False}),
                         ("development", "release"))

    def test_focused_requests_override_the_legacy_full_default(self):
        for flag, mode in ci_policy.LEGACY_MODES.items():
            for full in (True, False):
                with self.subTest(flag=flag, full=full):
                    self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {flag: True, "full_validation": full}),
                                     (mode, "release"))

    def test_all_named_modes_and_windows_targets(self):
        for mode in ci_policy.NAMED_MODES - {"windows-crash"}:
            with self.subTest(mode=mode):
                self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {"mode": mode}), (mode, "release"))
        for target in ("service-startup", "surface"):
            self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {
                "mode": "windows-crash", "windows_crash_target": target,
            }), (f"windows-{target}" if target == "service-startup" else "windows-surface-crash", "release"))
        self.assertEqual(ci_policy.resolve_mode("workflow_dispatch", {
            "mode": "windows-crash", "windows_crash_target": "surface", "windows_crash_build_profile": "debug",
        }), ("windows-surface-crash", "debug"))

    def test_conflicts_are_rejected_before_selecting_jobs(self):
        for invalid in (
            {"mode": "surprise"},
            {"mode": "full", "macos_native_probe": True},
            {"source_acceptance": True, "windows_x64_mygo_test_package": True},
            {"windows_surface_performance": True, "windows_surface_crash_diagnostics": True},
            {"mode": "windows-crash"},
            {"mode": "windows-performance", "windows_crash_build_profile": "debug"},
            {"mode": "windows-crash", "windows_crash_target": "service-startup", "windows_crash_build_profile": "debug"},
            {"mode": "darwin-native", "windows_crash_target": "surface"},
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                ci_policy.resolve_mode("workflow_dispatch", invalid)

    def test_desktop_matrix_is_the_exact_requested_slice(self):
        targets = {
            "full": ["linux-x86_64", "linux-aarch64", "windows-x86_64", "windows-aarch64"],
            "source-acceptance": ["linux-x86_64", "windows-x86_64"],
            "mygo-package": ["windows-x86_64"],
            "linux-release-inputs": ["linux-x86_64"],
        }
        for mode in ci_policy.ALL_MODES:
            self.assertEqual([row["name"] for row in ci_policy.desktop_targets(mode)], targets.get(mode, []))
        self.assertFalse(ci_policy.desktop_targets("full")[-1]["can_test"])

    def test_desktop_focused_requests_keep_the_portable_tooling_contract_only(self):
        for mode in ("mygo-package", "linux-release-inputs"):
            jobs = ci_policy.job_requirements(mode, False)
            self.assertEqual(jobs["Frontend"], ("Test portable tooling and release contracts",))
            self.assertFalse(any(name.startswith(("Go development", "Rust development")) for name in jobs))

    def test_legacy_installer_is_conservative_with_no_reliable_diff(self):
        self.assertFalse(ci_policy.requires_legacy_installer("source-acceptance", "push", ""))
        self.assertTrue(ci_policy.requires_legacy_installer("development", "push", ""))
        with patch("scripts.ci_policy.subprocess.run") as git:
            git.return_value.returncode = 0
            self.assertFalse(ci_policy.requires_legacy_installer("development", "pull_request", SHA))
            self.assertEqual(git.call_args_list[-1].args[0][:5], ["git", "diff", "--quiet", SHA, "HEAD"])
            git.return_value.returncode = 128
            self.assertTrue(ci_policy.requires_legacy_installer("development", "pull_request", SHA))


class AcceptanceTests(unittest.TestCase):
    def test_every_required_step_is_declared_in_a_workflow(self):
        workflows = Path(__file__).resolve().parents[2] / ".github" / "workflows"
        declared = {
            name
            for filename in ("ci.yml", "standalone-cli.yml", "flatpak.yml")
            for name in re.findall(r"^      - name: (.+)$", (workflows / filename).read_text(encoding="utf-8"), re.MULTILINE)
        }
        for mode in ci_policy.ALL_MODES:
            for profile in ("release", "debug"):
                for steps in ci_policy.job_requirements(mode, mode == "full", profile).values():
                    self.assertTrue(set(steps) <= declared, set(steps) - declared)

    def test_incomplete_github_job_pages_cannot_approve_ci(self):
        env = dict(GITHUB_REPOSITORY="test/ci", GITHUB_RUN_ID=str(RUN), GITHUB_RUN_ATTEMPT=str(ATTEMPT))
        for pages in (
            [{"total_count": 2, "jobs": [{"id": 1}]}],
            [{"total_count": 2, "jobs": [{"id": 1}, {"id": 1}]}],
        ):
            with self.subTest(pages=pages):
                with patch("scripts.ci_policy.subprocess.check_output", return_value=json.dumps(pages)):
                    with self.assertRaisesRegex(ValueError, "incomplete or duplicated"):
                        ci_policy.workflow_jobs(env, exact_attempt=True)

    def test_each_mode_passes_its_exact_job_set(self):
        for mode in ci_policy.ALL_MODES:
            with self.subTest(mode=mode):
                legacy = mode == "full"
                jobs = finished(mode, legacy_installer=legacy)
                report = ci_policy.evaluate(mode, legacy, jobs, SHA, RUN, ATTEMPT)
                self.assertTrue(all(row[-1] for row in report), report)

    def test_full_requires_each_nsis_scenario_and_current_mygo_package(self):
        jobs = finished("full", legacy_installer=True)
        jobs = [row for row in jobs if row["name"] != "Native NSIS (DeniedGuiUpgrade)"]
        windows = next(row for row in jobs if row["name"] == "Tauri backend (windows-x86_64)")
        windows["steps"][-1]["conclusion"] = "skipped"
        report = ci_policy.evaluate("full", True, jobs, SHA, RUN, ATTEMPT)
        self.assertIn(("Native NSIS (DeniedGuiUpgrade)", "required", "missing", False), report)
        self.assertIn("MyGo installer acceptance", ci_policy.job_requirements("full", False))
        self.assertIn(("Tauri backend (windows-x86_64) / Upload Windows x64 MyGo test installer",
                       "required", "skipped", False), report)

    def test_actual_failure_skip_and_cancellation_fail_the_requested_job(self):
        for status in ("failure", "skipped", "cancelled"):
            jobs = finished("darwin-native")
            jobs[-1]["conclusion"] = status
            report = ci_policy.evaluate("darwin-native", False, jobs, SHA, RUN, ATTEMPT)
            self.assertIn(("Darwin native foundations", "required", status, False), report)

    def test_source_checks_reject_previous_attempt_and_missing_matrix_row(self):
        prefix = "Source acceptance / "
        jobs = finished("source-acceptance", prefix=prefix)
        jobs[-1]["run_attempt"] -= 1
        jobs.pop(-2)
        report = ci_policy.evaluate("source-acceptance", False, jobs, SHA, RUN, ATTEMPT, prefix)
        self.assertIn(("Tauri backend (linux-x86_64)", "required", "missing", False), report)
        self.assertIn(("Tauri backend (windows-x86_64)", "required", "wrong run, attempt or source", False), report)

    def test_surface_crash_debug_mode_requires_the_debug_artifact(self):
        mode = "windows-surface-crash"
        report = ci_policy.evaluate(mode, False, finished(mode), SHA, RUN, ATTEMPT, crash_profile="debug")
        self.assertIn(("Windows Surface comparison or crash diagnostics / Build the exact debug Desktop and retain its symbol receipt",
                       "required", "missing", False), report)

    def test_development_accepts_an_unchanged_job_reused_by_github(self):
        jobs = finished("development")
        jobs[-1]["run_attempt"] -= 1
        report = ci_policy.evaluate("development", False, jobs, SHA, RUN, ATTEMPT)
        self.assertTrue(all(row[-1] for row in report), report)
        jobs[-1]["run_id"] -= 1
        report = ci_policy.evaluate("development", False, jobs, SHA, RUN, ATTEMPT)
        self.assertIn(("Rust development (windows-latest, desktop-check)", "required", "wrong run, attempt or source", False), report)

    def test_focused_modes_reject_accidentally_scheduled_full_jobs(self):
        jobs = finished("windows-performance") + [finished("full")[-1]]
        report = ci_policy.evaluate("windows-performance", False, jobs, SHA, RUN, ATTEMPT)
        self.assertIn(("Tauri backend (windows-aarch64)", "not_applicable", "success", False), report)
        jobs[-1]["conclusion"] = "skipped"
        self.assertTrue(all(row[-1] for row in ci_policy.evaluate("windows-performance", False, jobs, SHA, RUN, ATTEMPT)))


if __name__ == "__main__":
    unittest.main()
