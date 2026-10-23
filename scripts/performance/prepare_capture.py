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
CAPTURE_SOURCES = ("main.rs", "measured_engine.rs", "exclusive_guard.rs")


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
    root_manifest = source / "Cargo.toml"
    root_workspace = root_manifest.is_file() and "workspace" in tomllib.loads(
        root_manifest.read_text(encoding="utf-8"))
    isolated = "workspace" not in parsed and not root_workspace
    if isolated:
        with manifest.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write('\n[workspace]\nresolver = "2"\n')
    return source, {"revision": revision, "archive_sha256": sha256(archive),
                    "manifest_original_sha256": before, "manifest_build_sha256": sha256(manifest),
                    "adjustment": ("Only empty workspace isolation in the archived package; no business source edit"
                                   if isolated else "Archived workspace preserved; no source or manifest edit")}


def baseline_layout(source):
    """Keep the baseline's real package, workspace lock and native key provider."""
    core = source / "crates/dzc-core"
    if (core / "Cargo.toml").is_file():
        workspace = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
        if "workspace" not in workspace:
            raise ValueError("Core baseline requires its archived root workspace")
        native = source / "crates/dzc-platform"
        if not (native / "Cargo.toml").is_file() or not (source / "Cargo.lock").is_file():
            raise ValueError("Core baseline requires its platform package and root lock")
        return "dzc-core", core, native, source / "Cargo.lock", ["native-provider"]
    return "dezirclip", source / "src-tauri", None, source / "src-tauri/Cargo.lock", []


def external_packages(lock):
    return {(item["name"], item["version"], item.get("source"), item.get("checksum"))
            for item in tomllib.loads(lock.read_text(encoding="utf-8"))["package"] if item.get("source")}


def select_cargo_executable(messages, manifest, name, target, triple, profile):
    """Bind the copied bytes to this build's one native binary artifact."""
    expected = (target / triple / profile / (name + (".exe" if os.name == "nt" else ""))).resolve()
    artifacts = []
    finished = []
    for line in messages.splitlines():
        if not line.lstrip().startswith("{"):
            continue
        message = json.loads(line)
        if not isinstance(message, dict):
            raise ValueError("Invalid Cargo JSON message")
        if message.get("reason") == "build-finished":
            finished.append(message.get("success"))
        if message.get("reason") != "compiler-artifact":
            continue
        emitted_target = message.get("target", {})
        if emitted_target.get("name") != name or emitted_target.get("kind") != ["bin"]:
            continue
        emitted_manifest = message.get("manifest_path")
        if not isinstance(emitted_manifest, str) or Path(emitted_manifest).resolve() != manifest.resolve():
            raise ValueError("Cargo binary artifact belongs to another manifest")
        executable = message.get("executable")
        if not isinstance(executable, str) or not Path(executable).is_absolute():
            raise ValueError("Cargo did not report an absolute executable path")
        executable = Path(executable).resolve(strict=True)
        if executable != expected or not executable.is_file():
            raise ValueError("Cargo executable is outside the explicit native target/profile namespace")
        if message.get("profile", {}).get("test") is not False:
            raise ValueError("Cargo reported a test executable instead of the adapter binary")
        if str(executable) not in [str(Path(path).resolve()) for path in message.get("filenames", [])]:
            raise ValueError("Cargo executable is absent from its artifact files")
        artifacts.append((executable, message))
    if finished != [True] or len(artifacts) != 1:
        raise ValueError("Expected one binary artifact and one successful Cargo build-finished message")
    return artifacts[0]


def build_cargo_executable(arguments, manifest, name, target, triple, profile, repository):
    completed = subprocess.run(arguments, cwd=repository, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, encoding="utf-8", errors="replace")
    messages = manifest.parent / "cargo-build.jsonl"
    stderr = manifest.parent / "cargo-build.stderr.log"
    messages.write_text(completed.stdout, encoding="utf-8")
    stderr.write_text(completed.stderr, encoding="utf-8")
    if completed.stderr:
        sys.stderr.write(completed.stderr)
    if completed.returncode:
        raise subprocess.CalledProcessError(completed.returncode, arguments, completed.stdout, completed.stderr)
    executable, artifact = select_cargo_executable(completed.stdout, manifest, name, target, triple, profile)
    return executable, {"compiler_artifact": artifact,
                        "build_messages": str(messages), "build_messages_sha256": sha256(messages),
                        "build_stderr": str(stderr), "build_stderr_sha256": sha256(stderr)}


def dependency_spec(source, name):
    """Resolve the package's dependency without selecting a different SQLite build."""
    manifest = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
    declared = manifest.get("dependencies", {}).get(name)
    if declared is None:
        raise ValueError(f"Source package does not declare {name}")
    specification = {"version": declared} if isinstance(declared, str) else dict(declared)
    base = source
    if specification.pop("workspace", False):
        explicit = manifest.get("package", {}).get("workspace")
        candidates = [(source / explicit).resolve()] if explicit is not None else [source, *source.parents]
        for directory in candidates:
            path = directory / "Cargo.toml"
            if path.is_file():
                workspace = tomllib.loads(path.read_text(encoding="utf-8")).get("workspace")
                if workspace is not None:
                    base = directory
                    break
        else:
            raise ValueError(f"Cannot locate workspace dependency {name}")
        inherited = workspace.get("dependencies", {}).get(name)
        if inherited is None:
            raise ValueError(f"Workspace does not declare {name}")
        resolved = {"version": inherited} if isinstance(inherited, str) else dict(inherited)
        for key, value in specification.items():
            if key == "features":
                resolved[key] = list(dict.fromkeys([*resolved.get(key, []), *value]))
            elif key == "default-features":
                if value is False and resolved.get(key, True):
                    raise ValueError(f"Source has an ignored workspace default-features override for {name}")
                resolved[key] = value
            elif key == "optional":
                resolved[key] = value
            else:
                raise ValueError(f"Unsupported inherited dependency field {name}.{key}")
        specification = resolved
    if specification.get("optional", False):
        raise ValueError(f"Adapter requires non-optional source dependency {name}")
    if "path" in specification:
        specification["path"] = (base / specification["path"]).resolve(strict=True).as_posix()
    return specification


def toml_value(value):
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return "[" + ",".join(toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key) + "=" + toml_value(item) for key, item in value.items()) + "}"
    raise ValueError(f"Unsupported dependency TOML value: {type(value).__name__}")


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
privacy-runtime=[]
privacy-policy=["privacy-runtime"]
capture-protection-report=["privacy-policy"]

[[bin]]
name="capture-perf-{variant}"
path="../../source/main.rs"

[dependencies]
{dependency}
serde_json="1"
rusqlite={toml_value(dependency_spec(source, "rusqlite"))}

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


def capture_contract(package, source):
    if package != "dzc-core":
        return 1
    manifest = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest.get("package", {}).get("metadata", {}).get("dzc", {}).get("capture-contract", 1)
    if type(version) is not int or version not in (1, 2, 3):
        raise ValueError("Unsupported Core capture contract")
    return version


def capture_protection_report(package, source):
    if package != "dzc-core":
        return False
    manifest = tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))
    value = manifest.get("package", {}).get("metadata", {}).get("dzc", {}).get("capture-protection-report", False)
    if type(value) is not bool or value and capture_contract(package, source) < 3:
        raise ValueError("Unsupported Core capture protection receipt")
    return value


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
    for name in CAPTURE_SOURCES:
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
             "timing_samples_collected": False,
             "functional_only": args.allow_dirty_candidate,
             "eligible_for_timing": not candidate_state["dirty"] and not args.allow_dirty_candidate,
             "candidate": candidate_state, "baseline": baseline_proof, "profile": args.profile,
             "target": command(["rustc", "--print", "host-tuple"]), "rustc": command(["rustc", "-Vv"]),
             "target_selection": "explicit native host target; Cargo artifact JSON is authoritative",
             "machine_id": hashlib.sha256(f"{platform.node()}|{platform.machine()}".encode()).hexdigest(),
             "tool_sha256": {path.name: sha256(path) for path in sorted(TOOL_ROOT.glob("*.py"))},
             "cargo": command(["cargo", "-Vv"]),
             "build_environment": {key: value for key, value in os.environ.items()
                                   if key in {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"}
                                   or key.startswith("CARGO_PROFILE_")},
             "template_sha256": {name: sha256(source / name) for name in CAPTURE_SOURCES},
             "evidence": copies, "adapters": {}}
    target = args.target_dir.resolve() if args.target_dir else output / "target"
    variants = [("legacy", *baseline_layout(baseline)),
                ("core", "dzc-core", candidate / "crates/dzc-core", candidate / "crates/dzc-platform", candidate / "Cargo.lock", ["native-provider"]),
                ("engine", "dzc-core", candidate / "crates/dzc-core", candidate / "crates/dzc-platform", candidate / "Cargo.lock", ["engine"])]
    for variant, package, package_source, native, lock, features in variants:
        contract = capture_contract(package, package_source)
        protection_report = capture_protection_report(package, package_source)
        if variant == "engine" and contract >= 2:
            features = [*features, "privacy-runtime"]
        if variant == "engine" and contract >= 3:
            features = [*features, "privacy-policy"]
        if variant == "engine" and protection_report:
            features = [*features, "capture-protection-report"]
        manifest, bridge = write_adapter(output, variant, package, package_source, native, lock)
        if bridge:
            features = [*features, "legacy-layout"]
        record = {"manifest": str(manifest), "manifest_sha256": sha256(manifest), "features": features,
                  "source_package": package, "source_manifest": str(package_source / "Cargo.toml"),
                   "role": "baseline" if variant == "legacy" else "candidate", "capture_contract": contract,
                   "capture_protection_report": protection_report}
        if args.mode != "prepare":
            name = f"capture-perf-{variant}"
            common = ["--offline", "--manifest-path", str(manifest), "--bin", name,
                      "--target", proof["target"], "--target-dir", str(target), "--jobs", str(args.jobs)]
            if features:
                common += ["--features", ",".join(features)]
            if args.profile == "release":
                common += ["--release"]
            subprocess.run(["cargo", "check", *common], cwd=repository, check=True)
            if not external_packages(manifest.parent / "Cargo.lock") <= external_packages(lock):
                raise ValueError("Harness selected external package versions outside the source lock")
            build_command = ["cargo", "build", "--locked", "--message-format=json", *common]
            executable, artifact_proof = build_cargo_executable(build_command, manifest, name, target, proof["target"], args.profile, repository)
            source_digest = sha256(executable)
            extension = ".exe" if os.name == "nt" else ""
            binary = output / "binaries" / variant / f"capture-perf-{variant}{extension}"
            binary.parent.mkdir(parents=True)
            shutil.copyfile(executable, binary)
            if sha256(binary) != source_digest or sha256(executable) != source_digest:
                raise ValueError("Cargo executable changed while freezing the adapter")
            binary.chmod(stat.S_IREAD)
            record.update({"binary": str(binary), "binary_sha256": source_digest,
                           "cargo_executable": str(executable), "cargo_executable_sha256": source_digest,
                           "lock_sha256": sha256(manifest.parent / "Cargo.lock"), "command": build_command,
                           **artifact_proof})
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
