"""Prepare reproducible real-repository/CaptureEngine performance adapters.

Only writes a new directory beneath the selected repository's .harness. This
tool never runs timing samples; verify mode keeps postconditions without times.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import stat
import subprocess
import sys
import tomllib
import zipfile

TOOL_ROOT = Path(__file__).resolve().parent


def command(arguments, cwd=None):
    return subprocess.check_output(arguments, cwd=cwd).decode("utf-8").strip()


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_fingerprint(directory):
    revision = command(["git", "rev-parse", "HEAD"], directory)
    status = command(["git", "status", "--porcelain"], directory)
    listed = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=directory)
    files = {}
    for name in sorted(set(listed.decode("utf-8").split("\0")) - {""}):
        path = directory / name
        if not path.exists():
            files[name] = None
            continue
        if not path.resolve().is_relative_to(directory):
            raise ValueError(f"Source resolves outside checkout: {name}")
        files[name] = sha256(path)
    digest = hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()
    return {"revision": revision, "dirty": bool(status), "status": status,
            "files_sha256": digest, "files": files}


def archive_baseline(repository, revision, output):
    archive = output / "baseline-source.zip"
    subprocess.run(["git", "archive", "--format=zip", "--output", str(archive), revision], cwd=repository, check=True)
    source = output / "baseline-source"
    source.mkdir()
    with zipfile.ZipFile(archive) as zipped:
        for member in zipped.infolist():
            relative = PurePosixPath(member.filename)
            if relative.is_absolute() or ".." in relative.parts or ":" in member.filename:
                raise ValueError("Invalid archived source path")
            if stat.S_ISLNK(member.external_attr >> 16):
                raise ValueError("Archived source symlinks need explicit handling")
            target = source.joinpath(*relative.parts)
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(zipped.read(member))
    manifest = source / "src-tauri/Cargo.toml"
    before = sha256(manifest)
    parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
    if "workspace" not in parsed:
        with manifest.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write('\n[workspace]\nresolver = "2"\n')
    return source, {"revision": revision, "archive_sha256": sha256(archive),
                    "manifest_original_sha256": before, "manifest_build_sha256": sha256(manifest),
                    "adjustment": "Only empty workspace isolation in the archived package; no business source edit"}


def external_packages(lock):
    return {(item["name"], item["version"], item.get("source"), item.get("checksum"))
            for item in tomllib.loads(lock.read_text(encoding="utf-8"))["package"] if item.get("source")}


def write_adapter(output, variant, package, source, native, lock):
    directory = output / "adapters" / variant
    directory.mkdir(parents=True)
    bridge = package == "dzc-core" and "legacy-bridge" in tomllib.loads(
        (source / "Cargo.toml").read_text(encoding="utf-8")).get("features", {})
    dependency_features = '["legacy-bridge"]' if bridge else '[]'
    dependency = f'app_backend={{package="{package}",path={json.dumps(source.as_posix())},default-features=false,features={dependency_features}}}'
    manifest = f'''[package]
name="capture-perf-{variant}"
version="0.0.0"
edition="2021"

[workspace]
resolver="2"

[features]
native-provider=[]
engine=["native-provider"]
legacy-layout=[]

[[bin]]
name="capture-perf-{variant}"
path="../../source/main.rs"

[dependencies]
{dependency}
serde_json="1"
rusqlite={{version="0.31",features=["bundled","functions"]}}

[profile.release]
lto=true
codegen-units=1
opt-level="s"
strip=true
panic="abort"
'''
    if native:
        manifest += f'\n[dependencies.dzc-platform]\npath={json.dumps(native.as_posix())}\n'
    path = directory / "Cargo.toml"
    path.write_text(manifest, encoding="utf-8")
    shutil.copyfile(lock, directory / "Cargo.lock")
    return path, bridge


def prepare(args):
    repository = args.repository.resolve(strict=True)
    candidate = args.candidate.resolve(strict=True)
    output = args.output.resolve()
    permitted = (repository / ".harness").resolve()
    if output == permitted or not output.is_relative_to(permitted):
        raise ValueError("Output must be a new child of repository/.harness")
    candidate_state = source_fingerprint(candidate)
    if args.candidate_revision and candidate_state["revision"] != command(["git", "rev-parse", args.candidate_revision], candidate):
        raise ValueError("Candidate checkout does not match the requested revision")
    if candidate_state["dirty"] and not args.allow_dirty_candidate:
        raise ValueError("Candidate source is dirty; use allow-dirty only for preparation/functional checks")
    output.mkdir(parents=True, exist_ok=False)
    source = output / "source"
    source.mkdir()
    for name in ["main.rs", "measured_engine.rs"]:
        shutil.copyfile(TOOL_ROOT / "capture" / name, source / name)
    baseline_revision = command(["git", "rev-parse", args.baseline_revision], repository)
    baseline, baseline_proof = archive_baseline(repository, baseline_revision, output)
    copies = []
    for index, evidence in enumerate(args.evidence):
        evidence = evidence.resolve(strict=True)
        parsed = json.loads(evidence.read_text(encoding="utf-8"))
        destination = output / "evidence" / f"{index:02}-{evidence.name}"
        destination.parent.mkdir(exist_ok=True)
        shutil.copyfile(evidence, destination)
        copies.append({"path": str(destination.relative_to(output)), "sha256": sha256(destination),
                       "verification_only": parsed.get("verification_only", False),
                       "scope": parsed.get("scope", parsed.get("limits")),
                       "binary_sha256": parsed.get("binary_sha256")})
    proof = {"schema_version": 1, "prepared_at": datetime.now(timezone.utc).isoformat(),
             "timing_samples_collected": False, "eligible_for_timing": not candidate_state["dirty"],
             "candidate": candidate_state, "baseline": baseline_proof, "profile": args.profile,
             "target": command(["rustc", "--print", "host-tuple"]), "rustc": command(["rustc", "-Vv"]),
             "machine_id": hashlib.sha256(f"{platform.node()}|{platform.machine()}".encode()).hexdigest(),
             "tool_sha256": {path.name: sha256(path) for path in sorted(TOOL_ROOT.glob("*.py"))},
             "cargo": command(["cargo", "-Vv"]),
             "build_environment": {key: value for key, value in os.environ.items()
                                   if key in {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"}
                                   or key.startswith("CARGO_PROFILE_")},
             "template_sha256": {name: sha256(source / name) for name in ["main.rs", "measured_engine.rs"]},
             "evidence": copies, "adapters": {}}
    target = args.target_dir.resolve() if args.target_dir else output / "target"
    variants = [("legacy", "dezirclip", baseline / "src-tauri", None, baseline / "src-tauri/Cargo.lock", []),
                ("core", "dzc-core", candidate / "crates/dzc-core", candidate / "crates/dzc-platform", candidate / "Cargo.lock", ["native-provider"]),
                ("engine", "dzc-core", candidate / "crates/dzc-core", candidate / "crates/dzc-platform", candidate / "Cargo.lock", ["engine"])]
    for variant, package, package_source, native, lock, features in variants:
        manifest, bridge = write_adapter(output, variant, package, package_source, native, lock)
        if bridge:
            features = [*features, "legacy-layout"]
        record = {"manifest": str(manifest), "manifest_sha256": sha256(manifest), "features": features}
        if args.mode != "prepare":
            common = ["--offline", "--manifest-path", str(manifest), "--target-dir", str(target), "--jobs", str(args.jobs)]
            if features:
                common += ["--features", ",".join(features)]
            if args.profile == "release":
                common += ["--release"]
            subprocess.run(["cargo", "check", *common], check=True)
            if not external_packages(manifest.parent / "Cargo.lock") <= external_packages(lock):
                raise ValueError("Harness selected external package versions outside the source lock")
            subprocess.run(["cargo", "build", "--locked", *common], check=True)
            extension = ".exe" if os.name == "nt" else ""
            binary = output / "binaries" / variant / f"capture-perf-{variant}{extension}"
            binary.parent.mkdir(parents=True)
            shutil.copyfile(target / args.profile / binary.name, binary)
            binary.chmod(stat.S_IREAD)
            record.update({"binary": str(binary), "binary_sha256": sha256(binary),
                           "lock_sha256": sha256(manifest.parent / "Cargo.lock"), "command": ["cargo", "build", "--locked", *common]})
            if args.mode == "verify":
                checked = output / f"functional-{variant}.json"
                subprocess.run([str(binary), str(output / "cases" / variant), "1000", "1", str(checked), "--verify-only"], check=True)
                record["functional_report"] = str(checked.relative_to(output))
                record["functional_report_sha256"] = sha256(checked)
        proof["adapters"][variant] = record
    if source_fingerprint(candidate) != candidate_state:
        raise ValueError("Candidate source changed during preparation; artifacts are not a frozen comparison")
    (output / "preparation.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
    print(output / "preparation.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline-revision", default="802808b")
    parser.add_argument("--candidate-revision")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--profile", choices=["debug", "release"], default="debug")
    parser.add_argument("--mode", choices=["prepare", "build", "verify"], default="prepare")
    parser.add_argument("--allow-dirty-candidate", action="store_true")
    parser.add_argument("--evidence", type=Path, action="append", default=[])
    arguments = parser.parse_args()
    if not 1 <= arguments.jobs <= 4:
        parser.error("Use one to four build jobs")
    try:
        prepare(arguments)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Cannot prepare capture evidence: {error}\n")
