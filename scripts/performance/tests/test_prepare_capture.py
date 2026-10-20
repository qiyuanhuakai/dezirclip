"""Controlled Cargo fixtures; no report produced here is performance evidence."""
from contextlib import redirect_stdout, redirect_stderr
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("prepare_capture", Path(__file__).resolve().parents[1] / "prepare_capture.py")
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)
HOST = "x86_64-pc-windows-msvc" if os.name == "nt" else "x86_64-unknown-linux-gnu"
EXTENSION = ".exe" if os.name == "nt" else ""
SQLITE_GIT = "https://github.com/rusqlite/rusqlite"
SQLITE_REV = "661107f6eb5d7477c61be1221744de3ad96c53e7"


def artifact(manifest, executable, name, fresh=False):
    return {"reason": "compiler-artifact", "package_id": "path+file:///synthetic#" + name + "@0.0.0",
            "manifest_path": str(manifest), "target": {"name": name, "kind": ["bin"]},
            "profile": {"test": False, "opt_level": "0"}, "features": [],
            "filenames": [str(executable)], "executable": str(executable), "fresh": fresh}


def messages(*items):
    return "\n".join(json.dumps(item) for item in [*items, {"reason": "build-finished", "success": True}]) + "\n"


class PreparationTests(unittest.TestCase):
    def prepare(self, stale=False, cargo_target=HOST, bad_artifact=False, workspace_baseline=False):
        temporary = tempfile.TemporaryDirectory(prefix="dzc-prepare-artifact-test-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        lock = 'version = 4\n[[package]]\nname = "fixture"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "' + "0" * 64 + '"\n'
        core = root / "crates/dzc-core"
        core.mkdir(parents=True)
        (core / "Cargo.toml").write_text('[package]\nname="dzc-core"\nversion="0.0.0"\n[features]\nlegacy-bridge=[]\n[dependencies]\nrusqlite.workspace=true\n')
        (root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/dzc-core"]\n[workspace.dependencies]\nrusqlite={git="' + SQLITE_GIT + '",rev="' + SQLITE_REV + '",features=["bundled","functions"]}\n')
        (root / "Cargo.lock").write_text(lock)
        # This conflicting config and environment must not redirect a native
        # benchmark or make its proof advertise the wrong architecture.
        (root / ".cargo").mkdir()
        (root / ".cargo/config.toml").write_text('[build]\ntarget="aarch64-unknown-linux-gnu"\n')
        target = root / "reused target"
        if stale:
            (target / "debug").mkdir(parents=True)
            for variant in ("legacy", "core", "engine"):
                (target / "debug" / ("capture-perf-" + variant + EXTENSION)).write_bytes(b"stale-root-artifact")
        output = root / ".harness/new-preparation"
        state = {"revision": "frozen-candidate", "dirty": False, "status": "", "files_sha256": "synthetic", "files": {}}

        def command(args, cwd=None):
            if args == ["git", "rev-parse", "frozen-candidate"]: return "frozen-candidate"
            if args[:2] == ["git", "rev-parse"]: return "baseline-revision"
            if args == ["rustc", "--print", "host-tuple"]: return HOST
            if args[0] in ("rustc", "cargo"): return args[0] + " fixture version"
            raise AssertionError(args)

        def baseline(repository, revision, directory):
            source = directory / "baseline-source"
            if workspace_baseline:
                package = source / "crates/dzc-core"
                package.mkdir(parents=True)
                (package / "Cargo.toml").write_bytes((core / "Cargo.toml").read_bytes())
                (source / "Cargo.toml").write_bytes((root / "Cargo.toml").read_bytes())
                (source / "Cargo.lock").write_text(lock)
                native = source / "crates/dzc-platform"
                native.mkdir()
                (native / "Cargo.toml").write_text('[package]\nname="dzc-platform"\nversion="0.0.0"\n')
                return source, {"revision": revision, "archive_sha256": "fixture"}
            package = source / "src-tauri"
            package.mkdir(parents=True)
            (package / "Cargo.toml").write_text('[package]\nname="dezirclip"\nversion="0.0.0"\n[dependencies]\nrusqlite={version="0.31",features=["bundled","functions"]}\n')
            (package / "Cargo.lock").write_text(lock)
            return source, {"revision": revision, "archive_sha256": "fixture"}

        seen_builds = []
        def cargo(args, **kwargs):
            self.assertEqual(args[0], "cargo")
            self.assertIn("--offline", args)
            self.assertEqual(args[args.index("--target") + 1], HOST)
            self.assertEqual(args[args.index("--jobs") + 1], "2")
            self.assertEqual(kwargs["cwd"], root)
            if args[1] == "check": return subprocess.CompletedProcess(args, 0)
            self.assertEqual(args[1], "build")
            self.assertIn("--locked", args)
            self.assertIn("--message-format=json", args)
            name = args[args.index("--bin") + 1]
            manifest = Path(args[args.index("--manifest-path") + 1])
            executable = target / HOST / "debug" / (name + EXTENSION)
            executable.parent.mkdir(parents=True, exist_ok=True)
            executable.write_bytes(("current-cargo-output:" + name).encode())
            seen_builds.append(executable)
            stdout = messages() if bad_artifact else messages(artifact(manifest, executable, name))
            return subprocess.CompletedProcess(args, 0, stdout, "synthetic cargo stderr\n")

        args = SimpleNamespace(repository=root, candidate=root, output=output, candidate_revision="frozen-candidate",
                               allow_dirty_candidate=False, baseline_revision="baseline", evidence=[], profile="debug",
                               target_dir=target, jobs=2, mode="build")
        with patch.object(PREPARE, "command", side_effect=command), patch.object(PREPARE, "source_fingerprint", return_value=state), \
             patch.object(PREPARE, "archive_baseline", side_effect=baseline), patch.object(PREPARE.subprocess, "run", side_effect=cargo), \
             patch.object(PREPARE.platform, "node", return_value="synthetic-host"), \
             patch.object(PREPARE.platform, "machine", return_value="synthetic-native"), \
             patch.dict(os.environ, {"CARGO_BUILD_TARGET": cargo_target}), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            if bad_artifact:
                with self.assertRaisesRegex(ValueError, "Expected one binary artifact"):
                    PREPARE.prepare(args)
                self.assertFalse((output / "preparation.json").exists())
                self.assertEqual((target / "debug" / ("capture-perf-legacy" + EXTENSION)).read_bytes(), b"stale-root-artifact")
                self.assertTrue((output / "adapters/legacy/cargo-build.jsonl").is_file())
                return
            PREPARE.prepare(args)
        proof = json.loads((output / "preparation.json").read_text())
        self.assertEqual(proof["target"], HOST)
        self.assertEqual(proof["build_environment"]["CARGO_BUILD_TARGET"], cargo_target)
        self.assertTrue(proof["eligible_for_timing"])
        self.assertEqual(len(seen_builds), 3)
        for variant, record in proof["adapters"].items():
            dependency = tomllib.loads(Path(record["manifest"]).read_text())["dependencies"]["rusqlite"]
            if variant == "legacy" and not workspace_baseline:
                self.assertEqual(dependency, {"version": "0.31", "features": ["bundled", "functions"]})
            else:
                self.assertEqual(dependency, {"git": SQLITE_GIT, "rev": SQLITE_REV, "features": ["bundled", "functions"]})
            self.assertEqual(record["role"], "baseline" if variant == "legacy" else "candidate")
            if variant == "legacy" and workspace_baseline:
                declared = tomllib.loads(Path(record["manifest"]).read_text())["dependencies"]
                self.assertEqual(record["source_package"], "dzc-core")
                self.assertEqual(declared["app_backend"]["path"], (output / "baseline-source/crates/dzc-core").as_posix())
                self.assertEqual(declared["dzc-platform"]["path"], (output / "baseline-source/crates/dzc-platform").as_posix())
                self.assertEqual(record["features"], ["native-provider", "legacy-layout"])
            expected = target / HOST / "debug" / ("capture-perf-" + variant + EXTENSION)
            self.assertEqual(Path(record["cargo_executable"]), expected)
            self.assertEqual(Path(record["binary"]).read_bytes(), ("current-cargo-output:capture-perf-" + variant).encode())
            self.assertEqual(record["binary_sha256"], PREPARE.sha256(expected))
            self.assertEqual(record["cargo_executable_sha256"], record["binary_sha256"])
            self.assertEqual(record["compiler_artifact"]["executable"], str(expected))
            self.assertEqual(record["build_messages_sha256"], PREPARE.sha256(Path(record["build_messages"])))
            if stale:
                self.assertEqual((target / "debug" / expected.name).read_bytes(), b"stale-root-artifact")

    def test_explicit_host_target_in_clean_cache_freezes_actual_reported_binary(self):
        self.prepare()

    def test_old_root_artifact_is_never_bound_to_the_new_source_proof(self):
        self.prepare(stale=True)

    def test_explicit_native_target_overrides_config_and_environment(self):
        self.prepare(stale=True, cargo_target="aarch64-unknown-linux-gnu")

    def test_missing_cargo_artifact_rejects_even_when_root_binary_exists(self):
        self.prepare(stale=True, bad_artifact=True)

    def test_workspace_baseline_keeps_its_core_lock_and_native_provider(self):
        self.prepare(workspace_baseline=True)


@unittest.skipUnless(shutil.which("git"), "Archived baseline checks require Git")
class BaselineArchiveTests(unittest.TestCase):
    def archive(self, workspace):
        temporary = tempfile.TemporaryDirectory(prefix="dzc-baseline-archive-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        repository = root / "repository"
        repository.mkdir()
        package = repository / "src-tauri"
        package.mkdir()
        manifest = package / "Cargo.toml"
        if workspace:
            manifest.write_text('[package]\nname="dezirclip"\nversion.workspace=true\n')
            (repository / "Cargo.toml").write_text('[workspace]\nmembers=["src-tauri","crates/dzc-core","crates/dzc-platform"]\n[workspace.package]\nversion="0.2.0-alpha.1"\n[workspace.dependencies]\nrusqlite="0.31"\n')
            (repository / "Cargo.lock").write_text('version=4\n')
            for name in ["dzc-core", "dzc-platform"]:
                member = repository / "crates" / name
                member.mkdir(parents=True)
                (member / "Cargo.toml").write_text('[package]\nname="' + name + '"\nversion.workspace=true\n[dependencies]\nrusqlite.workspace=true\n')
        else:
            manifest.write_text('[package]\nname="dezirclip"\nversion="0.1.0"\n')
            (package / "Cargo.lock").write_text('version=4\n')
        original = manifest.read_bytes()
        for args in [["init", "--quiet"], ["add", "."],
                     ["-c", "user.name=Controlled fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture"]]:
            subprocess.run(["git", *args], cwd=repository, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        revision = PREPARE.command(["git", "rev-parse", "HEAD"], repository)
        output = root / "output"
        output.mkdir()
        source, proof = PREPARE.archive_baseline(repository, revision, output)
        self.assertEqual(manifest.read_bytes(), original)
        self.assertEqual(proof["revision"], revision)
        self.assertEqual(proof["archive_sha256"], PREPARE.sha256(output / "baseline-source.zip"))
        return source, proof, original

    def test_workspace_inheritance_survives_real_git_archive_without_manifest_edits(self):
        source, proof, original = self.archive(workspace=True)
        self.assertEqual((source / "src-tauri/Cargo.toml").read_bytes(), original)
        self.assertEqual(proof["manifest_original_sha256"], proof["manifest_build_sha256"])
        package, core, native, lock, features = PREPARE.baseline_layout(source)
        self.assertEqual((package, core, native, lock, features),
                         ("dzc-core", source / "crates/dzc-core", source / "crates/dzc-platform", source / "Cargo.lock", ["native-provider"]))
        self.assertEqual(PREPARE.dependency_spec(core, "rusqlite"), {"version": "0.31"})
        (source / "Cargo.lock").unlink()
        with self.assertRaisesRegex(ValueError, "platform package and root lock"):
            PREPARE.baseline_layout(source)

    def test_early_single_package_archive_gets_only_empty_workspace_isolation(self):
        source, proof, original = self.archive(workspace=False)
        self.assertEqual((source / "src-tauri/Cargo.toml").read_bytes(), original + b'\n[workspace]\nresolver = "2"\n')
        self.assertNotEqual(proof["manifest_original_sha256"], proof["manifest_build_sha256"])
        self.assertEqual(PREPARE.baseline_layout(source),
                         ("dezirclip", source / "src-tauri", None, source / "src-tauri/Cargo.lock", []))


class DependencyInheritanceTests(unittest.TestCase):
    def test_registry_shorthand_keeps_the_source_version(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[dependencies]\nrusqlite="0.31"\n')
            self.assertEqual(PREPARE.dependency_spec(root, "rusqlite"), {"version": "0.31"})

    def test_workspace_git_pin_version_defaults_and_additive_features_are_preserved(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            source = root / "crates/core"
            source.mkdir(parents=True)
            (root / "Cargo.toml").write_text('[workspace]\n[workspace.dependencies]\nrusqlite={git="' + SQLITE_GIT + '",rev="' + SQLITE_REV + '",version="0.40.1",default-features=false,features=["bundled"]}\n')
            (source / "Cargo.toml").write_text('[dependencies]\nrusqlite={workspace=true,features=["functions","bundled"]}\n')
            expected = {"git": SQLITE_GIT, "rev": SQLITE_REV, "version": "0.40.1", "default-features": False, "features": ["bundled", "functions"]}
            resolved = PREPARE.dependency_spec(source, "rusqlite")
            self.assertEqual(resolved, expected)
            self.assertEqual(tomllib.loads("rusqlite=" + PREPARE.toml_value(resolved))["rusqlite"], expected)

    def test_relative_path_uses_the_declaring_workspace_and_explicit_workspace_location(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            workspace = root / "workspace"
            source = root / "detached/core"
            dependency = workspace / "dependencies/sqlite"
            source.mkdir(parents=True)
            dependency.mkdir(parents=True)
            (workspace / "Cargo.toml").write_text('[workspace]\n[workspace.dependencies]\nrusqlite={path="dependencies/sqlite",features=["bundled","functions"]}\n')
            (source / "Cargo.toml").write_text('[package]\nworkspace="../../workspace"\n[dependencies]\nrusqlite.workspace=true\n')
            resolved = PREPARE.dependency_spec(source, "rusqlite")
            self.assertEqual(resolved["path"], dependency.resolve().as_posix())

    def test_local_path_uses_package_directory(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            (root / "sqlite").mkdir()
            (root / "Cargo.toml").write_text('[dependencies]\nrusqlite={path="sqlite",features=["bundled","functions"]}\n')
            self.assertEqual(PREPARE.dependency_spec(root, "rusqlite")["path"], (root / "sqlite").resolve().as_posix())

    def test_missing_inherited_or_optional_source_is_rejected_without_fallback(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            for content, error in [('[dependencies]\n', "does not declare"),
                                   ('[workspace]\n[dependencies]\nrusqlite.workspace=true\n', "Workspace does not declare"),
                                   ('[dependencies]\nrusqlite={version="0.31",optional=true}\n', "non-optional")]:
                with self.subTest(content=content):
                    (root / "Cargo.toml").write_text(content)
                    with self.assertRaisesRegex(ValueError, error):
                        PREPARE.dependency_spec(root, "rusqlite")

    def test_ignored_default_features_override_is_not_reinterpreted_as_effective(self):
        with tempfile.TemporaryDirectory(prefix="dzc-adapter-dependency-") as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[workspace]\n[workspace.dependencies]\nrusqlite="0.31"\n[dependencies]\nrusqlite={workspace=true,default-features=false}\n')
            with self.assertRaisesRegex(ValueError, "ignored workspace default-features"):
                PREPARE.dependency_spec(root, "rusqlite")


class ArtifactValidationTests(unittest.TestCase):
    def test_wrong_manifest_duplicate_missing_or_test_artifact_cannot_select_a_binary(self):
        with tempfile.TemporaryDirectory(prefix="dzc-artifact-parser-") as directory:
            root = Path(directory).resolve()
            manifest = root / "Cargo.toml"
            manifest.write_text("synthetic")
            name = "capture-perf-engine"
            executable = root / "target" / HOST / "debug" / (name + EXTENSION)
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"current")
            value = artifact(manifest, executable, name, fresh=True)
            self.assertEqual(PREPARE.select_cargo_executable(messages(value), manifest, name, root / "target", HOST, "debug")[0], executable)
            invalid = [messages(), messages(value, value)]
            for field, replacement in [("manifest_path", str(root / "other/Cargo.toml")), ("executable", None),
                                       ("profile", {"test": True}), ("filenames", [])]:
                invalid.append(messages(dict(value, **{field: replacement})))
            invalid.append(json.dumps(value) + '\n{"reason":"build-finished","success":false}\n')
            stale = root / "target/debug" / executable.name
            stale.parent.mkdir()
            stale.write_bytes(b"stale")
            invalid.append(messages(artifact(manifest, stale, name)))
            for stream in invalid:
                with self.subTest(stream=stream), self.assertRaises(ValueError):
                    PREPARE.select_cargo_executable(stream, manifest, name, root / "target", HOST, "debug")

    @unittest.skipUnless(os.environ.get("DZC_TEST_REAL_CARGO") == "1" and shutil.which("cargo"), "Opt in to the small native Cargo artifact smoke test")
    def test_real_cargo_explicit_host_json_ignores_old_root_and_reports_fresh_cache(self):
        with tempfile.TemporaryDirectory(prefix="dzc-real-cargo-artifact-") as directory:
            root = Path(directory).resolve()
            name = "capture-perf-fixture"
            manifest = root / "Cargo.toml"
            manifest.write_text('[package]\nname="' + name + '"\nversion="0.0.0"\nedition="2021"\n[workspace]\n')
            (root / "Cargo.lock").write_text('version=4\n[[package]]\nname="' + name + '"\nversion="0.0.0"\n')
            (root / "src").mkdir()
            (root / "src/main.rs").write_text('fn main() {}\n')
            target = root / "target"
            (target / "debug").mkdir(parents=True)
            stale = target / "debug" / (name + EXTENSION)
            stale.write_bytes(b"stale root cache")
            host = subprocess.check_output(["rustc", "--print", "host-tuple"], text=True).strip()
            arguments = ["cargo", "build", "--offline", "--locked", "--message-format=json", "--manifest-path", str(manifest), "--bin", name, "--target", host, "--target-dir", str(target), "--jobs", "2"]
            with patch.dict(os.environ, {"CARGO_BUILD_TARGET": host}):
                binary, proof = PREPARE.build_cargo_executable(arguments, manifest, name, target, host, "debug", root)
                self.assertEqual(binary, target / host / "debug" / (name + EXTENSION))
                self.assertNotEqual(binary.read_bytes(), stale.read_bytes())
                _, cached = PREPARE.build_cargo_executable(arguments, manifest, name, target, host, "debug", root)
            self.assertTrue(cached["compiler_artifact"]["fresh"])
            self.assertEqual(stale.read_bytes(), b"stale root cache")
            self.assertEqual(proof["compiler_artifact"]["manifest_path"], str(manifest))


if __name__ == "__main__":
    unittest.main()
