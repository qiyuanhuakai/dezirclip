import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


def load(name, filename):
    location = Path(__file__).resolve().parents[1] / filename
    spec = importlib.util.spec_from_file_location(name, location)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


package = load("dzc_package", "package-dzc.py")
verify = load("dzc_package_verify", "verify-dzc-package.py")


def executable_header(platform):
    if platform.startswith("windows-"):
        data = bytearray(512)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3C, 0x80)
        data[0x80:0x84] = b"PE\0\0"
        struct.pack_into("<H", data, 0x84, package.PLATFORMS[platform])
        struct.pack_into("<H", data, 0x94, 240)
        struct.pack_into("<H", data, 0x98, 0x20B)
        struct.pack_into("<I", data, 0x98 + 60, 512)
        struct.pack_into("<I", data, 0x98 + 108, 16)
    else:
        data = bytearray(64)
        data[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HH", data, 16, 2, package.PLATFORMS[platform])
    return data


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dzc-package-tests-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        for name in ("LICENSE", "docs/dzc-standalone.md", "docs/data-safety.md", "docs/cli.md", "docs/LINUX_INSTALL.md",
                     "integrations/README.md", "integrations/dzc-integrations.py",
                     "integrations/dzc_integrations/__main__.py",
                     "integrations/dzc_integrations/client.py", "integrations/shell/dzc.nu",
                     "integrations/requirements-tui.txt", "skills/dzc-cli/SKILL.md",
                     "skills/dezirclip-theme/SKILL.md", "skills/dezirclip-theme/assets/forest.theme.json",
                     "integrations/tests/test_private.py", "integrations/.venv/private.txt",
                     "integrations/dzc_integrations/__pycache__/client.pyc",
                     "skills/dzc-cli/.private", "skills/dezirclip-theme/.private",
                     "skills/dezirclip-theme/scripts/__pycache__/private.pyc"):
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name, encoding="utf-8")

    def binary(self, platform):
        path = self.root / f"binary-{platform}"
        path.write_bytes(executable_header(platform))
        path.chmod(0o644)
        return path

    def test_all_architectures_have_matching_headers(self):
        for platform in package.PLATFORMS:
            with self.subTest(platform=platform):
                package.validate_binary(self.binary(platform), platform)

    def test_rejects_placeholder_scripts_and_wrong_architecture(self):
        path = self.root / "placeholder"
        for data in (b"", b"\0" * 256, b"#!/bin/sh\n" + b"echo fake\n" * 10):
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                package.validate_binary(path, "linux-x86_64")
        with self.assertRaises(ValueError):
            package.validate_binary(self.binary("linux-aarch64"), "linux-x86_64")
        with self.assertRaises(ValueError):
            package.validate_binary(self.binary("windows-aarch64"), "windows-x86_64")

    def test_version_must_match_cargo_including_tag_prefix(self):
        self.assertEqual(package.requested_version("v1.2.3", "1.2.3"), "1.2.3")
        self.assertEqual(package.requested_version(None, "1.2.3"), "1.2.3")
        with self.assertRaises(ValueError):
            package.requested_version("v1.2.4", "1.2.3")

    def test_target_directory_environment_is_respected(self):
        target_directory = self.root / "cargo-output"
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target_directory)}):
            actual = package.default_binary("linux-aarch64", "aarch64-unknown-linux-gnu")
        self.assertEqual(actual, target_directory.resolve() / "aarch64-unknown-linux-gnu" / "release" / "dzc")

    def test_archive_layout_checksum_and_linux_executable_mode(self):
        for platform in ("linux-x86_64", "windows-x86_64"):
            with self.subTest(platform=platform):
                archive = package.package(platform, self.binary(platform), "1.2.3",
                                          self.root / "output", self.repo)
                checksum = Path(str(archive) + ".sha256").read_text(encoding="utf-8")
                self.assertEqual(checksum, f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
                verify.verify_checksum(archive)
                extracted = self.root / f"extracted-{platform}"
                verify.extract_archive(archive, extracted)
                binary_name = "dzc.exe" if platform.startswith("windows-") else "dzc"
                self.assertTrue((extracted / binary_name).is_file())
                report = json.loads((extracted / "COMPATIBILITY.json").read_text())
                self.assertEqual(report["binary_sha256"], hashlib.sha256((extracted / binary_name).read_bytes()).hexdigest())
                verify.verify(archive, static_only=True)
                self.assertEqual((extracted / "README.md").read_text(), "docs/dzc-standalone.md")
                self.assertTrue((extracted / "integrations" / "shell" / "dzc.nu").is_file())
                self.assertTrue((extracted / "skills" / "dzc-cli" / "SKILL.md").is_file())
                self.assertTrue((extracted / "skills" / "dezirclip-theme" / "SKILL.md").is_file())
                self.assertTrue((extracted / "skills" / "dezirclip-theme" / "assets" / "forest.theme.json").is_file())
                self.assertTrue((extracted / "docs" / "LINUX_INSTALL.md").is_file())
                self.assertFalse((extracted / "integrations" / "tests").exists())
                self.assertFalse((extracted / "integrations" / ".venv").exists())
                self.assertFalse((extracted / "integrations" / "dzc_integrations" / "__pycache__").exists())
                self.assertFalse((extracted / "skills" / "dzc-cli" / ".private").exists())
                self.assertFalse((extracted / "skills" / "dezirclip-theme" / ".private").exists())
                self.assertFalse((extracted / "skills" / "dezirclip-theme" / "scripts" / "__pycache__").exists())
                if platform.startswith("linux-"):
                    with tarfile.open(archive, "r:gz") as source:
                        self.assertEqual(source.getmember("dzc").mode, 0o755)

    def test_desktop_resource_map_matches_both_complete_visible_skills(self):
        repository = Path(__file__).resolve().parents[2]
        config = json.loads((repository / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
        resources = config["bundle"]["resources"]
        self.assertIsInstance(resources, dict)
        expected = {name for name in package.archive_files(self.binary("windows-x86_64"), "windows-x86_64", repository)
                    if name.startswith("skills/")}
        self.assertEqual(set(resources.values()), expected)
        for source, target in resources.items():
            original = (repository / "src-tauri" / source).resolve()
            self.assertTrue(original.is_file())
            self.assertEqual(target, "skills/" + original.relative_to(repository / "skills").as_posix())

    def test_theme_skill_and_its_example_are_required_distribution_resources(self):
        for name in ("SKILL.md", "assets/forest.theme.json"):
            resource = self.repo / "skills" / "dezirclip-theme" / name
            original = resource.read_bytes()
            resource.unlink()
            with self.assertRaisesRegex(ValueError, "missing matching distribution resource"):
                package.archive_files(self.binary("linux-x86_64"), "linux-x86_64", self.repo)
            resource.write_bytes(original)

    def test_verifier_rejects_a_package_missing_the_theme_example(self):
        archive = package.package("windows-x86_64", self.binary("windows-x86_64"), "1.2.3", self.root / "output", self.repo)
        with zipfile.ZipFile(archive) as source:
            files = {name: source.read(name) for name in source.namelist()
                     if name != "skills/dezirclip-theme/assets/forest.theme.json"}
        with zipfile.ZipFile(archive, "w") as output:
            for name, content in files.items():
                output.writestr(name, content)
        Path(str(archive) + ".sha256").write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n", encoding="ascii")
        with self.assertRaisesRegex(ValueError, "missing packaged resource"):
            verify.verify(archive, static_only=True)

    def prepare_resource_fixture(self, script_name):
        script = self.repo / "scripts" / script_name
        script.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(Path(__file__).resolve().parents[1] / script_name, script)
        resources = {}
        for skill in ("dzc-cli", "dezirclip-theme"):
            directory = self.repo / "skills" / skill
            for path in directory.rglob("*"):
                if package.visible_file(path, directory):
                    relative = path.relative_to(self.repo).as_posix()
                    resources["../" + relative] = relative
        config = self.repo / "src-tauri" / "tauri.conf.json"
        config.parent.mkdir(parents=True, exist_ok=True)
        config.write_text(json.dumps({"bundle": {"resources": resources}}), encoding="utf-8")
        return script

    @unittest.skipUnless(shutil.which("powershell") or shutil.which("pwsh"), "PowerShell is not available")
    def test_portable_packages_both_skills_and_failed_resource_copy_preserves_previous_archive(self):
        script = self.prepare_resource_fixture("package-portable.ps1")
        binaries = self.root / "portable-binaries"
        binaries.mkdir()
        for name in ("dezirclip.exe", "dzc.exe"):
            (binaries / name).write_bytes(executable_header("windows-x86_64"))
        output = self.root / "portable-output"
        command = [shutil.which("powershell") or shutil.which("pwsh"), "-NoProfile", "-ExecutionPolicy", "Bypass",
                   "-File", str(script), "-TargetDirectory", str(binaries), "-OutputDirectory", str(output)]
        subprocess.run(command, check=True, capture_output=True, text=True, timeout=45)
        archive = output / "dezirclip-portable.zip"
        original = archive.read_bytes()
        with zipfile.ZipFile(archive) as package_file:
            names = set(package_file.namelist())
            self.assertTrue({"dezirclip.exe", "dzc.exe", "skills/dzc-cli/SKILL.md", "skills/dezirclip-theme/SKILL.md",
                             "skills/dezirclip-theme/assets/forest.theme.json"} <= names)
            self.assertFalse(any("__pycache__" in name or "/.private" in name for name in names))
        (self.repo / "skills" / "dezirclip-theme" / "assets" / "forest.theme.json").unlink()
        result = subprocess.run(command, capture_output=True, text=True, timeout=45)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(archive.read_bytes(), original)
        self.assertFalse(any(path.name.startswith(".dezirclip-portable-") for path in output.iterdir()))

    @unittest.skipUnless(shutil.which("dpkg-deb") and shutil.which("node"), "Debian packaging tools are not available")
    def test_flatpak_staging_has_both_skills_at_the_manifest_share_location(self):
        script = self.prepare_resource_fixture("prepare-flatpak-root.mjs")
        icon = self.repo / "src-tauri" / "icons" / "icon.png"
        icon.parent.mkdir()
        icon.write_bytes(b"fixture icon")
        deb_root = self.root / "deb-root"
        control = deb_root / "DEBIAN" / "control"
        control.parent.mkdir(parents=True)
        control.write_text("Package: dezirclip\nVersion: 1.2.3\nArchitecture: all\nMaintainer: Test <test@example.invalid>\nDescription: packaging fixture\n", encoding="utf-8")
        desktop = deb_root / "usr" / "share" / "applications" / "DezirClip.desktop"
        desktop.parent.mkdir(parents=True)
        desktop.write_text("[Desktop Entry]\nExec=old\nIcon=old\n", encoding="utf-8")
        deb_directory = self.repo / "target" / "release" / "bundle" / "deb"
        deb_directory.mkdir(parents=True)
        subprocess.run(["dpkg-deb", "--build", str(deb_root), str(deb_directory / "fixture.deb")], check=True, capture_output=True, timeout=30)
        subprocess.run(["node", str(script)], check=True, capture_output=True, timeout=30)
        resources = self.repo / "flatpak" / "build-root" / "usr" / "share" / "dezirclip" / "skills"
        for name in ("dzc-cli/SKILL.md", "dezirclip-theme/SKILL.md", "dezirclip-theme/assets/forest.theme.json"):
            self.assertEqual((resources / name).read_bytes(), (self.repo / "skills" / name).read_bytes())
        self.assertFalse((resources / "dezirclip-theme" / "scripts" / "__pycache__").exists())

    def test_checksum_rejects_tampering_wrong_filename_and_missing_sidecar(self):
        archive = package.package("linux-x86_64", self.binary("linux-x86_64"), "1.2.3",
                                  self.root / "output", self.repo)
        sidecar = Path(str(archive) + ".sha256")
        original_archive = archive.read_bytes()
        original_sidecar = sidecar.read_text(encoding="utf-8")
        archive.write_bytes(original_archive + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            verify.verify_checksum(archive)
        archive.write_bytes(original_archive)
        sidecar.write_text(original_sidecar.replace(archive.name, "other.tar.gz"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "name exactly"):
            verify.verify_checksum(archive)
        sidecar.unlink()
        with self.assertRaises(FileNotFoundError):
            verify.verify_checksum(archive)

    def test_dependency_report_cannot_claim_a_different_executable(self):
        archive = package.package("windows-aarch64", self.binary("windows-aarch64"), "1.2.3",
                                  self.root / "output", self.repo)
        with zipfile.ZipFile(archive) as source:
            files = {member.filename: source.read(member) for member in source.infolist()}
        report = json.loads(files["COMPATIBILITY.json"])
        report["binary_sha256"] = "0" * 64
        files["COMPATIBILITY.json"] = json.dumps(report).encode()
        with zipfile.ZipFile(archive, "w") as output:
            for name, content in files.items():
                output.writestr(name, content)
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        Path(str(archive) + ".sha256").write_text(f"{checksum}  {archive.name}\n", encoding="ascii")
        with self.assertRaisesRegex(ValueError, "compatibility report"):
            verify.verify(archive, static_only=True)

    def test_extractor_rejects_path_traversal_and_zip_symlinks(self):
        for name in ("../outside", "/absolute", "..\\outside", "C:/outside"):
            with self.subTest(name=name):
                archive = self.root / "unsafe.zip"
                with zipfile.ZipFile(archive, "w") as output:
                    output.writestr(name, b"bad")
                with self.assertRaises(ValueError):
                    verify.extract_archive(archive, self.root / "extract-unsafe")
        archive = self.root / "link.zip"
        with zipfile.ZipFile(archive, "w") as output:
            member = zipfile.ZipInfo("link")
            member.external_attr = (stat.S_IFLNK | 0o777) << 16
            output.writestr(member, "../outside")
        with self.assertRaises(ValueError):
            verify.extract_archive(archive, self.root / "extract-link")

    def test_extractor_rejects_tar_links(self):
        archive = self.root / "link.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            member = tarfile.TarInfo("link")
            member.type = tarfile.SYMTYPE
            member.linkname = "../outside"
            output.addfile(member, io.BytesIO())
        with self.assertRaises(ValueError):
            verify.extract_archive(archive, self.root / "extract-tar-link")


if __name__ == "__main__":
    unittest.main()
