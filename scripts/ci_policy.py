"""Select CI jobs and reconcile the exact GitHub Actions run attempt."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


TARGETS = (
    ("linux-x86_64", "ubuntu-24.04", "", ""),
    ("linux-aarch64", "ubuntu-24.04-arm", "", ""),
    ("windows-x86_64", "windows-latest", "", ""),
    ("windows-aarch64", "windows-latest", "aarch64-pc-windows-msvc", "amd64_arm64"),
)
DEVELOPMENT_RUNNERS = ("ubuntu-24.04", "windows-latest")
NSIS_SCENARIOS = ("LegacyUninstall", "OldToNew", "PublishedAlphaToAlpha", "OwnedUpgrade",
                  "OwnedExternalEdit", "DeniedGuiUpgrade")
LEGACY_MODES = {
    "source_acceptance": "source-acceptance",
    "macos_native_probe": "darwin-native",
    "native_file_drag_only": "native-file-drag",
    "windows_release_memory": "historical-windows-memory",
    "windows_x64_mygo_test_package": "mygo-package",
    "linux_release_inputs": "linux-release-inputs",
    "windows_surface_performance": "windows-performance",
    "windows_service_crash_stacks": "windows-service-startup",
    "windows_surface_crash_diagnostics": "windows-surface-crash",
}
NAMED_MODES = ({"development", "full", "windows-crash"} | set(LEGACY_MODES.values())) - {
    "windows-service-startup", "windows-surface-crash",
}
DEVELOPMENT_MODES = {"development", "full", "source-acceptance"}
LEGACY_INSTALLER_PATHS = (
    ".github/workflows/ci.yml", ".github/workflows/release.yml",
    "scripts/build-mygo-installer.py", "scripts/mygo-installer.nsi",
    "scripts/package-published-mygo.py", "scripts/tests/mygo-*-windows.*",
    "scripts/tests/service-process-receipt.ps1", "src-tauri/icons/icon.ico",
)
NATIVE_DESKTOP_STEPS = (
    "Test desktop tooling in its platform environment",
    "Test Tauri crate",
    "Verify native Rust and Go protocol compatibility",
    "Verify optional local service without native capture",
    "Verify MyGo React library operations against the Rust Service",
)
CURRENT_MYGO_STEPS = (
    "Stage Windows x64 MyGo package for testing", "Upload Windows x64 MyGo test installer",
)
SOURCE_JOBS = ("frontend", "rust-development", "go-development", "ci-desktop-plan", "tauri")


def resolve_mode(event, inputs):
    named = inputs.get("mode") or "auto"
    crash_target = inputs.get("windows_crash_target") or "auto"
    profile = inputs.get("windows_crash_build_profile") or "release"
    aliases = [name for name, mode in LEGACY_MODES.items() if inputs.get(name) is True]
    if named not in NAMED_MODES | {"auto"}:
        raise ValueError("Unknown CI mode")
    if len(aliases) > 1 or (aliases and named != "auto"):
        raise ValueError("Select one CI mode or one legacy focused input")
    if crash_target not in ("auto", "service-startup", "surface") or profile not in ("release", "debug"):
        raise ValueError("Unknown Windows crash target or build profile")

    if event != "workflow_dispatch":
        # The reusable Release call retains its existing source_acceptance input.
        if aliases not in ([], ["source_acceptance"]) or named != "auto":
            raise ValueError("Focused modes are manual CI requests")
        mode = "source-acceptance" if aliases else "development"
    elif aliases:
        mode = LEGACY_MODES[aliases[0]]
    elif named == "auto":
        mode = "development" if inputs.get("full_validation") is False else "full"
    elif named == "windows-crash":
        if crash_target == "auto":
            raise ValueError("Select service-startup or surface for windows-crash")
        mode = {"surface": "windows-surface-crash", "service-startup": "windows-service-startup"}[crash_target]
    else:
        mode = named

    if named != "windows-crash" and crash_target != "auto":
        raise ValueError("The crash target is specific to the named windows-crash mode")
    if profile == "debug" and mode != "windows-surface-crash":
        raise ValueError("Debug build profile is specific to Surface crash diagnostics")
    return mode, profile


def desktop_targets(mode):
    names = {
        "source-acceptance": {"linux-x86_64", "windows-x86_64"},
        "mygo-package": {"windows-x86_64"},
        "linux-release-inputs": {"linux-x86_64"},
    }.get(mode, set())
    return [
        dict(name=name, platform=platform, rust_target=triple, msvc_arch=arch,
             can_test=not triple, cargo_args=f"--target {triple}" if triple else "")
        for name, platform, triple, arch in TARGETS
        if mode == "full" or name in names
    ]


def requires_legacy_installer(mode, event, base):
    if mode != "development":
        return mode == "full"
    if event not in ("pull_request", "push") or not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        return True
    if subprocess.run(["git", "cat-file", "-e", f"{base}^{{commit}}"], stderr=subprocess.DEVNULL).returncode:
        if subprocess.run(["git", "fetch", "--no-tags", "--depth=1", "origin", base]).returncode:
            return True
    return subprocess.run(["git", "diff", "--quiet", base, "HEAD", "--", *LEGACY_INSTALLER_PATHS]).returncode != 0


def job_requirements(mode, legacy_installer, crash_profile="release"):
    jobs = {"Select desktop acceptance targets": ()}
    desktop = desktop_targets(mode)
    if mode in DEVELOPMENT_MODES or desktop:
        jobs["Frontend"] = ("Test portable tooling and release contracts",)
    if mode in DEVELOPMENT_MODES:
        jobs["Frontend"] += ("Check single-source local protocol generation", "Run unit tests",
                             "Build frontend", "Verify production WebView startup")
        for runner in DEVELOPMENT_RUNNERS:
            jobs[f"Go development ({runner})"] = ("Test local protocol client", "Test MyGo desktop and native surfaces")
            if runner == "windows-latest":
                jobs[f"Go development ({runner})"] += ("Verify WebView fixture environment restoration",)
            if runner == "windows-latest":
                core = ("Test shared core, protocol, Host, Service and CLI",
                        "Verify the bare CLI in a real Windows pseudoterminal")
            else:
                core = ("Test Linux workspace and isolated Platform",)
            jobs[f"Rust development ({runner}, core-tests)"] = core
            jobs[f"Rust development ({runner}, desktop-check)"] = ("Check all desktop targets without linking another application",)
    if mode == "full" or (mode == "development" and legacy_installer):
        jobs["MyGo installer acceptance"] = ("Verify installation, installed desktop, upgrade, locked files and uninstall",)
    if mode == "full":
        jobs["Build shared native NSIS fixtures"] = ()
        for scenario in NSIS_SCENARIOS:
            jobs[f"Native NSIS ({scenario})"] = ("Exercise actual installer and application",)
        for name, _, _, _ in TARGETS:
            steps = ("Package actual executable with dependency report", "Upload standalone archive and checksum")
            if name == "windows-aarch64":
                steps += ("Verify cross-built ARM64 architecture and static CRT",)
            else:
                steps += ("Verify extracted CLI and persistent history", "Test integrations against the isolated actual binary")
            if name.startswith("linux-"):
                steps += ("Run actual archive in AlmaLinux 8, Ubuntu 22.04 and Debian 12",)
            jobs[f"Compatible standalone CLI / Standalone ({name})"] = steps
        for name in ("linux-x86_64", "linux-aarch64"):
            jobs[f"Verified Flatpak / Flatpak ({name})"] = ("Verify OCR and paste on the final Platform runtime",)
    for item in desktop:
        name = item["name"]
        steps = ("Build standalone CLI and Service for desktop acceptance",)
        steps += (NATIVE_DESKTOP_STEPS if item["can_test"] else (
            "Build Tauri crate", "Check cross-built Tauri targets preserving standalone companions"))
        if item["can_test"]:
            if name.startswith("linux-"):
                steps += ("Verify MyGo Native history and clipboard on X11",)
            else:
                steps += ("Verify MyGo Native paste into an independent Windows editor",
                          "Verify Windows clipboard reader window ownership",
                          "Verify middle-button hook survives a blocked resident UI",
                          "Build actual Windows desktop with embedded frontend for Surface acceptance",
                          "Verify real Service-mode paste target binding on disposable Windows")
        if name == "windows-x86_64" and mode in ("full", "mygo-package"):
            steps += CURRENT_MYGO_STEPS
        if mode == "linux-release-inputs":
            steps += ("Supply optional Linux x64 release inputs",
                      "Preserve optional Linux x64 release inputs and original build failures")
        jobs[f"Tauri backend ({name})"] = steps
    if mode == "native-file-drag":
        jobs.update({
            "native-file-drag (ubuntu-24.04)": ("Verify actual Linux file transfers", "Verify X11 screenshot pixels across color depths"),
            "native-file-drag (windows-latest)": ("Verify actual Windows file transfers",),
        })
    if mode == "darwin-native":
        jobs["Darwin native foundations"] = ("Verify profile persistence across signed helper processes",
                                            "Verify signed Swift sessions against the local Rust Host and platform fixture")
    if mode == "historical-windows-memory":
        jobs["Published Windows memory comparison"] = ("Measure original published executables",)
    if mode == "windows-service-startup":
        jobs["Diagnose Windows Service startup"] = ("Diagnose Windows Service startup crashes with local symbols",)
    if mode == "windows-performance":
        jobs["Windows Surface comparison or crash diagnostics"] = (
            "Build the actual release Desktop with embedded frontend",
            "Verify these release bytes before the sequential ABBA comparison", "Run the complete sequential release ABBA comparison")
    if mode == "windows-surface-crash":
        build_step = ("Build the exact debug Desktop and retain its symbol receipt" if crash_profile == "debug"
                      else "Build the actual release Desktop with embedded frontend")
        jobs["Windows Surface comparison or crash diagnostics"] = (
            build_step, "Run up to five success-only diagnostic cases on the selected build")
    return jobs


ALL_MODES = (NAMED_MODES - {"windows-crash"}) | {"windows-service-startup", "windows-surface-crash"}
KNOWN_JOBS = set().union(*(job_requirements(mode, mode in ("full", "development")) for mode in ALL_MODES))


def evaluate(mode, legacy_installer, jobs, sha, run, attempt, prefix="", crash_profile="release"):
    requirements = job_requirements(mode, legacy_installer, crash_profile)
    by_name = {}
    for job in jobs:
        by_name.setdefault(job["name"], []).append(job)

    report = []
    for name in sorted(KNOWN_JOBS):
        matches = by_name.get(prefix + name, ())
        required = name in requirements
        if not matches:
            actual = "missing" if required else "not scheduled"
        elif len(matches) != 1:
            actual = "duplicate"
        else:
            (job,) = matches
            recorded_attempt = job.get("run_attempt")
            valid_attempt = type(recorded_attempt) is int and (
                recorded_attempt == attempt if mode == "source-acceptance" else 1 <= recorded_attempt <= attempt
            )
            if (job.get("run_id"), job.get("head_sha")) != (run, sha) or not valid_attempt:
                actual = "wrong run, attempt or source"
            elif job["status"] != "completed":
                actual = job["status"]
            else:
                actual = job.get("conclusion") or "missing conclusion"
        valid = actual == "success" if required else actual in ("skipped", "not scheduled")
        report.append((name, "required" if required else "not_applicable", actual, valid))
        if required and actual == "success":
            steps = matches[0].get("steps", [])
            for expected in requirements[name]:
                observed = [step for step in steps if step["name"] == expected]
                if len(observed) == 1:
                    (step,) = observed
                    conclusion = step.get("conclusion") if step["status"] == "completed" else step["status"]
                else:
                    conclusion = "missing" if not observed else "duplicate"
                report.append((f"{name} / {expected}", "required", conclusion, conclusion == "success"))
    return report


def plan():
    env = os.environ
    event = env["GITHUB_EVENT_NAME"]
    mode, profile = resolve_mode(event, json.loads(env["CI_INPUTS"]) or {})
    base = env.get("PR_BASE_SHA" if event == "pull_request" else "PUSH_BEFORE", "")
    desktop = desktop_targets(mode)
    outputs = {
        "mode": mode,
        "crash_profile": profile,
        "desktop": str(bool(desktop)).lower(),
        "matrix": json.dumps({"include": desktop}, separators=(",", ":")),
        "development": str(mode in DEVELOPMENT_MODES).lower(),
        "legacy_installer": str(requires_legacy_installer(mode, event, base)).lower(),
    }
    print(f"CI mode {mode}; desktop targets: {', '.join(row['name'] for row in desktop) or 'none'}")
    with open(env["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in outputs.items():
            print(f"{name}={value}", file=output)


def workflow_jobs(env, exact_attempt):
    run = f"repos/{env['GITHUB_REPOSITORY']}/actions/runs/{env['GITHUB_RUN_ID']}"
    endpoint = (f"{run}/attempts/{env['GITHUB_RUN_ATTEMPT']}/jobs?per_page=100" if exact_attempt
                else f"{run}/jobs?filter=latest&per_page=100")
    pages = json.loads(subprocess.check_output(["gh", "api", "--paginate", "--slurp", endpoint], text=True))
    if not isinstance(pages, list) or not pages:
        raise ValueError("GitHub returned no CI job listing")
    count = pages[0]["total_count"]
    jobs = [job for page in pages for job in page["jobs"]]
    if (len(jobs) != count or any(page["total_count"] != count for page in pages)
            or len({job["id"] for job in jobs}) != count):
        raise ValueError("GitHub returned an incomplete or duplicated CI job listing")
    return jobs


def summarize():
    env = os.environ
    needs = json.loads(env["DEPENDENCIES"])
    selected = needs["ci-desktop-plan"]
    if selected["result"] != "success":
        raise ValueError(f"CI request did not produce a plan ({selected['result']})")
    mode = selected["outputs"]["mode"]
    if mode not in ALL_MODES:
        raise ValueError("Invalid selected CI mode")
    source = env["GITHUB_SHA"]
    if subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() != source:
        raise ValueError("The checked-out source does not match the CI run")
    legacy_installer = selected["outputs"]["legacy_installer"] == "true"
    crash_profile = selected["outputs"]["crash_profile"]
    run = int(env["GITHUB_RUN_ID"])
    prefix = "Source acceptance / " if mode == "source-acceptance" and env["GITHUB_EVENT_NAME"] != "workflow_dispatch" else ""
    # GitHub job details can lag behind the scheduler dependency status.
    attempt = int(env["GITHUB_RUN_ATTEMPT"])
    for check in range(3):
        jobs = workflow_jobs(env, exact_attempt=mode == "source-acceptance")
        report = evaluate(mode, legacy_installer, jobs, env["CI_REPORTED_SOURCE"], run, attempt, prefix, crash_profile)
        if check == 2 or all(actual not in ("missing", "queued", "in_progress") for _, _, actual, _ in report):
            break
        time.sleep(3)

    lines = [f"## CI acceptance · {mode}", "", "| Check | Plan | Result |", "|---|---|---|"]
    lines.extend(f"| {name} | {required} | {actual} |" for name, required, actual, _ in report)
    with open(env["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write("\n".join(lines) + "\n")
    invalid = [(name, actual) for name, _, actual, valid in report if not valid]
    if invalid:
        raise ValueError(f"CI execution did not match the required plan: {invalid}")
    if mode == "source-acceptance":
        results = {name: needs[name]["result"] for name in SOURCE_JOBS}
        targets = sorted(row["name"] for row in json.loads(selected["outputs"]["matrix"])["include"])
        if set(results.values()) != {"success"} or targets != ["linux-x86_64", "windows-x86_64"]:
            raise ValueError("Source acceptance did not complete its required jobs and targets")
        receipt = dict(schema_version=1, mode=mode, source_revision=source, run_id=run,
                       run_attempt=attempt, jobs=results, desktop_targets=targets)
        (Path(env["RUNNER_TEMP"]) / "source-acceptance.json").write_text(
            json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        with open(env["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            print(f"source={source}", file=output)


if __name__ == "__main__":
    try:
        {"plan": plan, "summarize": summarize}[sys.argv[1]]()
    except (KeyError, ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
