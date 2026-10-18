import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import struct
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
                     "integrations/tests/test_private.py", "integrations/.venv/private.txt",
                     "integrations/dzc_integrations/__pycache__/client.pyc",
                     "skills/dzc-cli/.private"):
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
                self.assertTrue((extracted / "docs" / "LINUX_INSTALL.md").is_file())
                self.assertFalse((extracted / "integrations" / "tests").exists())
                self.assertFalse((extracted / "integrations" / ".venv").exists())
                self.assertFalse((extracted / "integrations" / "dzc_integrations" / "__pycache__").exists())
                self.assertFalse((extracted / "skills" / "dzc-cli" / ".private").exists())
                if platform.startswith("linux-"):
                    with tarfile.open(archive, "r:gz") as source:
                        self.assertEqual(source.getmember("dzc").mode, 0o755)

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
