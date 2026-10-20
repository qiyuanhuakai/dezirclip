"""Build provenance checks use synthetic Git trees and never launch the product."""
import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which("rustc") and shutil.which("git"), "Rust and Git are required")
class BuildProvenanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workspace = tempfile.TemporaryDirectory(prefix="dzc-build-info-script-")
        cls.binary = Path(cls.workspace.name) / ("metadata-build-script.exe" if os.name == "nt" else "metadata-build-script")
        subprocess.run(["rustc", "--edition=2021", str(ROOT / "crates/dzc-protocol/build.rs"), "-o", str(cls.binary)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.workspace.cleanup()

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="dzc-build-info-tree-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        (self.root / "crates/dzc-protocol/src").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[workspace]\n')
        (self.root / "crates/dzc-protocol/Cargo.toml").write_text('[package]\nname="fixture"\n')
        (self.root / "crates/dzc-protocol/src/lib.rs").write_text('pub const VERSION: u8 = 1;\n')

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.STDOUT).decode().strip()

    def commit_fixture(self):
        self.git("init", "--quiet")
        self.git("add", "Cargo.toml", "crates")
        self.git("-c", "user.name=Build Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "synthetic fixture")
        return self.git("rev-parse", "HEAD")

    def run_script(self, root=None, **values):
        env = {key: value for key, value in os.environ.items() if key not in ("DZC_BUILD_REVISION", "DZC_BUILD_SOURCE_ROOT", "GITHUB_ACTIONS", "GITHUB_SHA")}
        env.update(CARGO_MANIFEST_DIR=str((root or self.root) / "crates/dzc-protocol"), TARGET="x86_64-pc-windows-msvc", **values)
        return subprocess.run([str(self.binary)], env=env, capture_output=True, text=True, timeout=15)

    def test_local_clean_and_modified_checkout_are_distinguished_and_tracked_inputs_watched(self):
        revision = self.commit_fixture()
        clean = self.run_script()
        self.assertEqual(clean.returncode, 0, clean.stderr)
        self.assertIn("DZC_SOURCE_REVISION=" + revision, clean.stdout)
        self.assertIn("DZC_SOURCE_STATE=verified_checkout", clean.stdout)
        normalized = clean.stdout.replace("\\\\?\\", "")
        self.assertIn("rerun-if-changed=" + str(self.root / "Cargo.toml"), normalized)
        self.assertIn("rerun-if-changed=" + str(self.root / ".git/index"), normalized)
        self.assertNotIn("rerun-if-changed=" + str(self.root / ".git/refs") + "\n", normalized)
        (self.root / "Cargo.toml").write_text('[workspace]\n# changed fixture\n')
        dirty = self.run_script()
        self.assertEqual(dirty.returncode, 0)
        self.assertIn("DZC_SOURCE_STATE=modified_checkout", dirty.stdout)

    def test_unknown_source_archive_does_not_claim_a_revision(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0)
        self.assertIn("DZC_SOURCE_REVISION=unknown", result.stdout)
        self.assertIn("DZC_SOURCE_STATE=unknown", result.stdout)

    def test_explicit_revision_must_match_git_and_actions_sha(self):
        revision = self.commit_fixture()
        good = self.run_script(DZC_BUILD_REVISION=revision, GITHUB_ACTIONS="true", GITHUB_SHA=revision)
        self.assertEqual(good.returncode, 0, good.stderr)
        for values in [dict(DZC_BUILD_REVISION="unknown"), dict(DZC_BUILD_REVISION="0" * 40),
                       dict(DZC_BUILD_REVISION=revision, GITHUB_ACTIONS="true", GITHUB_SHA="0" * 40)]:
            with self.subTest(values=values):
                self.assertNotEqual(self.run_script(**values).returncode, 0)

    def test_explicit_revision_without_a_verified_source_checkout_is_rejected(self):
        self.assertNotEqual(self.run_script(DZC_BUILD_REVISION="a" * 40).returncode, 0)

    def test_actions_copy_is_compared_to_its_mounted_checkout_and_tampering_is_rejected(self):
        (self.root / "scripts/performance").mkdir(parents=True)
        (self.root / "scripts/performance/qa.rs").write_text('fn unrelated_qa_fixture() {}\n')
        revision = self.commit_fixture()
        self.git("add", "scripts")
        self.git("-c", "user.name=Build Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "unrelated QA input")
        revision = self.git("rev-parse", "HEAD")
        with tempfile.TemporaryDirectory(prefix="dzc-build-info-copy-") as temporary:
            copied = Path(temporary).resolve()
            shutil.copytree(self.root / "crates", copied / "crates")
            shutil.copyfile(self.root / "Cargo.toml", copied / "Cargo.toml")
            values = dict(DZC_BUILD_SOURCE_ROOT=str(self.root), DZC_BUILD_REVISION=revision, GITHUB_ACTIONS="true", GITHUB_SHA=revision)
            valid = self.run_script(copied, **values)
            self.assertEqual(valid.returncode, 0, valid.stderr)
            self.assertIn("DZC_SOURCE_STATE=verified_checkout", valid.stdout)
            (copied / "crates/dzc-protocol/src/lib.rs").write_text('pub const VERSION: u8 = 2;\n')
            changed = self.run_script(copied, **values)
            self.assertNotEqual(changed.returncode, 0)
            self.assertIn("differs from the declared source checkout", changed.stderr)

    def test_external_source_override_is_not_accepted_for_local_builds(self):
        revision = self.commit_fixture()
        self.assertNotEqual(self.run_script(DZC_BUILD_SOURCE_ROOT=str(self.root), DZC_BUILD_REVISION=revision).returncode, 0)

    def test_copied_embedded_wire_schema_must_match_declared_checkout(self):
        schema = self.root / "crates/dzc-protocol/src/wire/schema.json"
        schema.parent.mkdir()
        schema.write_text('{"$defs":{}}\n')
        revision = self.commit_fixture()
        with tempfile.TemporaryDirectory(prefix="dzc-build-info-wire-copy-") as temporary:
            copied = Path(temporary).resolve()
            shutil.copytree(self.root / "crates", copied / "crates")
            shutil.copyfile(self.root / "Cargo.toml", copied / "Cargo.toml")
            values = dict(DZC_BUILD_SOURCE_ROOT=str(self.root), DZC_BUILD_REVISION=revision, GITHUB_ACTIONS="true", GITHUB_SHA=revision)
            self.assertEqual(self.run_script(copied, **values).returncode, 0)
            (copied / "crates/dzc-protocol/src/wire/schema.json").write_text('{"changed":true}\n')
            changed = self.run_script(copied, **values)
            self.assertNotEqual(changed.returncode, 0)
            self.assertIn("differs from the declared source checkout", changed.stderr)


@unittest.skipUnless(os.environ.get("DZC_TEST_BIN"), "Set DZC_TEST_BIN for the built CLI metadata contract")
class CliBuildInfoTests(unittest.TestCase):
    def test_version_stays_compatible_and_build_info_never_creates_history(self):
        binary = str(Path(os.environ["DZC_TEST_BIN"]).resolve())
        version = json.loads((ROOT / "package.json").read_text())["version"]
        with tempfile.TemporaryDirectory(prefix="dzc-build-info-query-") as temporary:
            database = Path(temporary) / "must-not-exist.db"
            environment = {**os.environ, "DEZIRCLIP_DB_PATH": str(database)}
            old_contract = subprocess.run([binary, "--version"], env=environment, capture_output=True, check=True, timeout=10)
            self.assertEqual(old_contract.stdout.decode().strip(), "dzc " + version)
            report = subprocess.run([binary, "build-info", "--json"], env=environment, capture_output=True, check=True, timeout=10)
            self.assertEqual(report.stderr, b"")
            info = json.loads(report.stdout)
            self.assertEqual(info["metadata_version"], 1)
            self.assertEqual(info["component"], "cli")
            self.assertEqual(info["component_version"], version)
            self.assertEqual(info["protocol_version"], {"major": 1, "minor": 0})
            self.assertEqual({row["component"] for row in info["linked_components"]}, {"dzc-core", "dzc-platform", "dzc-protocol"})
            if info["source_revision"] is None:
                self.assertIsNone(info["build_id"])
                self.assertEqual(info["source_state"], "unknown")
            else:
                self.assertRegex(info["source_revision"], r"^[a-f0-9]{40}$")
                self.assertEqual(info["build_id"], version + "@" + info["source_revision"])
            self.assertFalse(database.exists())


if __name__ == "__main__":
    unittest.main()
