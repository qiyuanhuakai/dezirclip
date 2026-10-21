"""Exercise release version inheritance and workspace distribution boundaries."""
from pathlib import Path
import json
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]


class WorkspaceDistributionTests(unittest.TestCase):
    def test_core_dependencies_have_no_graphical_native_or_network_adapter(self):
        manifest = tomllib.loads((ROOT / "crates/dzc-core/Cargo.toml").read_text(encoding="utf-8"))
        forbidden = {"tauri", "gtk", "webkit2gtk", "dzc-platform", "keyring", "windows", "reqwest", "tokio"}
        self.assertFalse(forbidden.intersection(manifest["dependencies"]))
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        self.assertEqual(workspace["profile"]["release"]["opt-level"], "s")
        self.assertTrue(workspace["profile"]["release"]["lto"])
        self.assertEqual(workspace["profile"]["release"]["panic"], "abort")

    def test_protocol_wire_validation_is_offline_opt_in_and_legacy_bridge_is_opt_in(self):
        protocol = tomllib.loads((ROOT / "crates/dzc-protocol/Cargo.toml").read_text(encoding="utf-8"))
        self.assertEqual(set(protocol["dependencies"]), {"serde", "serde_json", "jsonschema"})
        validator = protocol["dependencies"]["jsonschema"]
        self.assertTrue(validator["optional"])
        self.assertFalse(validator["default-features"])
        self.assertFalse(set(validator.get("features", [])).intersection({"resolve-http", "resolve-file", "tls-aws-lc-rs", "tls-ring"}))
        self.assertEqual(protocol["features"]["wire"], ["dep:jsonschema"])
        self.assertNotIn("wire", protocol.get("features", {}).get("default", []))
        core = tomllib.loads((ROOT / "crates/dzc-core/Cargo.toml").read_text(encoding="utf-8"))
        self.assertNotIn("legacy-bridge", core.get("features", {}).get("default", []))
        platform = tomllib.loads((ROOT / "crates/dzc-platform/Cargo.toml").read_text(encoding="utf-8"))
        self.assertEqual(platform["dependencies"]["dzc-core"], {"workspace": True})

    @unittest.skipUnless(shutil.which("node"), "Node.js is required for version tooling")
    def test_version_tool_updates_all_members_and_locked_local_packages(self):
        with tempfile.TemporaryDirectory(prefix="dzc-workspace-version-") as temporary:
            directory = Path(temporary)
            for name in ["Cargo.toml", "Cargo.lock", "package.json", "src-tauri/tauri.conf.json",
                         "src-tauri/Cargo.toml", "crates/dzc-core/Cargo.toml", "crates/dzc-platform/Cargo.toml", "crates/dzc-protocol/Cargo.toml",
                         "crates/dzc-host/Cargo.toml", "crates/dzc-service/Cargo.toml", "scripts/set-release-version.mjs"]:
                target = directory / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, target)
            version = "0.2.0-alpha.99"
            subprocess.run(["node", "scripts/set-release-version.mjs", "v" + version], cwd=directory,
                           check=True, capture_output=True)
            workspace = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))
            self.assertEqual(workspace["workspace"]["package"]["version"], version)
            for member in workspace["workspace"]["members"]:
                package = tomllib.loads((directory / member / "Cargo.toml").read_text(encoding="utf-8"))["package"]
                self.assertEqual(package["version"], {"workspace": True})
            locked = tomllib.loads((directory / "Cargo.lock").read_text(encoding="utf-8"))["package"]
            local = {package["name"]: package["version"] for package in locked
                     if package["name"] in {"dezirclip", "dzc-core", "dzc-platform", "dzc-protocol", "dzc-host", "dzc-service"}}
            self.assertEqual(local, {name: version for name in ["dezirclip", "dzc-core", "dzc-platform", "dzc-protocol", "dzc-host", "dzc-service"]})
            self.assertEqual(json.loads((directory / "src-tauri/tauri.conf.json").read_text())["version"], version)
            self.assertEqual(json.loads((directory / "package.json").read_text())["version"], version)

    @unittest.skipUnless(shutil.which("node"), "Node.js is required for version tooling")
    def test_missing_lock_rejects_version_change_before_touching_manifests(self):
        with tempfile.TemporaryDirectory(prefix="dzc-workspace-version-incomplete-") as temporary:
            directory = Path(temporary)
            for name in ["Cargo.toml", "package.json", "src-tauri/tauri.conf.json", "scripts/set-release-version.mjs"]:
                target = directory / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, target)
            previous = (directory / "Cargo.toml").read_bytes()
            result = subprocess.run(["node", "scripts/set-release-version.mjs", "0.2.0-alpha.1"],
                                    cwd=directory, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((directory / "Cargo.toml").read_bytes(), previous)


if __name__ == "__main__":
    unittest.main()
