#!/usr/bin/env python3
"""Package the real dzc executable with its matching local integrations."""

import argparse
import io
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tarfile
import tempfile
import tomllib
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dzc_compat import enforce, inspect_binary, sha256_file

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = {
    "windows-x86_64": 0x8664,
    "windows-aarch64": 0xAA64,
    "linux-x86_64": 0x3E,
    "linux-aarch64": 0xB7,
}
EXCLUDED = {"tests", "__pycache__", "venv", "node_modules", "target"}


def requested_version(requested, actual):
    value = requested.removeprefix("v") if requested else actual
    if value != actual:
        raise ValueError(f"requested version {value!r} does not match Cargo.toml {actual!r}")
    return value


def default_binary(platform, target=None):
    configured = os.environ.get("CARGO_TARGET_DIR")
    directory = Path(configured).expanduser().resolve() if configured else ROOT / "src-tauri" / "target"
    if target:
        directory /= target
    return directory / "release" / ("dzc.exe" if platform.startswith("windows-") else "dzc")


def validate_binary(path, platform):
    if not path.is_file() or path.is_symlink() or path.stat().st_size < 64:
        raise ValueError(f"missing or empty executable: {path}")
    with path.open("rb") as binary:
        header = binary.read(64)
        if platform.startswith("windows-"):
            if header[:2] != b"MZ":
                raise ValueError("Windows dzc must be a PE executable, not a placeholder or script")
            offset = struct.unpack_from("<I", header, 0x3C)[0]
            if offset < 64 or offset + 26 > path.stat().st_size:
                raise ValueError("invalid PE header location")
            binary.seek(offset)
            pe = binary.read(26)
            if pe[:4] != b"PE\0\0" or struct.unpack_from("<H", pe, 24)[0] != 0x20B:
                raise ValueError("dzc must be a 64-bit PE executable")
            machine = struct.unpack_from("<H", pe, 4)[0]
        else:
            if header[:7] != b"\x7fELF\x02\x01\x01":
                raise ValueError("Linux dzc must be a little-endian ELF64 executable")
            executable_type, machine = struct.unpack_from("<HH", header, 16)
            if executable_type not in (2, 3):
                raise ValueError("ELF input is not an executable or position-independent executable")
        if machine != PLATFORMS[platform]:
            raise ValueError(f"wrong executable architecture for {platform}: 0x{machine:x}")


def visible_file(path, base):
    return path.is_file() and not path.is_symlink() and all(
        not part.startswith(".") and part not in EXCLUDED
        for part in path.relative_to(base).parts
    )


def archive_files(binary, platform, repository):
    files = {
        "dzc.exe" if platform.startswith("windows-") else "dzc": binary,
        "LICENSE": repository / "LICENSE",
        "README.md": repository / "docs" / "dzc-standalone.md",
        "docs/cli.md": repository / "docs" / "cli.md",
        "docs/dzc-standalone.md": repository / "docs" / "dzc-standalone.md",
        "docs/data-safety.md": repository / "docs" / "data-safety.md",
        "docs/LINUX_INSTALL.md": repository / "docs" / "LINUX_INSTALL.md",
    }
    integrations = repository / "integrations"
    for path in sorted(integrations.rglob("*")):
        if not visible_file(path, integrations):
            continue
        relative = path.relative_to(integrations)
        if (relative.parts[0] in ("dzc_integrations", "shell")
                or (len(relative.parts) == 1 and (path.suffix == ".py"
                    or path.name == "README.md" or path.name.startswith("requirements")))):
            files[f"integrations/{relative.as_posix()}"] = path
    skill = repository / "skills" / "dzc-cli"
    for path in sorted(skill.rglob("*")):
        if visible_file(path, skill):
            files[f"skills/dzc-cli/{path.relative_to(skill).as_posix()}"] = path
    for required in ("integrations/dzc-integrations.py", "integrations/README.md",
                     "integrations/dzc_integrations/__main__.py", "skills/dzc-cli/SKILL.md"):
        if required not in files:
            raise ValueError(f"missing matching distribution resource: {required}")
    for name, path in files.items():
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"missing distribution resource {name}: {path}")
    return files


def package(platform, binary, version, output_directory, repository=ROOT):
    binary = Path(binary).resolve()
    validate_binary(binary, platform)
    compatibility = json.dumps(enforce(inspect_binary(binary, platform)), indent=2).encode("utf-8") + b"\n"
    files = archive_files(binary, platform, Path(repository))
    output_directory = Path(output_directory).resolve()
    output_directory.mkdir(parents=True, exist_ok=True)
    extension = ".zip" if platform.startswith("windows-") else ".tar.gz"
    archive = output_directory / f"dzc-{version}-{platform}{extension}"
    descriptor, staging_name = tempfile.mkstemp(prefix=".dzc-package-", dir=output_directory)
    os.close(descriptor)
    staging = Path(staging_name)
    try:
        if extension == ".zip":
            with zipfile.ZipFile(staging, "w", compression=zipfile.ZIP_DEFLATED) as output:
                for name, source in files.items():
                    info = zipfile.ZipInfo(name)
                    info.create_system = 3
                    mode = 0o755 if name in ("dzc", "dzc.exe") else 0o644
                    info.external_attr = (stat.S_IFREG | mode) << 16
                    info.compress_type = zipfile.ZIP_DEFLATED
                    output.writestr(info, source.read_bytes())
                output.writestr("COMPATIBILITY.json", compatibility)
        else:
            with tarfile.open(staging, "w:gz") as output:
                for name, source in files.items():
                    info = output.gettarinfo(str(source), arcname=name)
                    info.mode = 0o755 if name == "dzc" else 0o644
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    with source.open("rb") as stream:
                        output.addfile(info, stream)
                info = tarfile.TarInfo("COMPATIBILITY.json")
                info.mode = 0o644
                info.size = len(compatibility)
                output.addfile(info, io.BytesIO(compatibility))
        staging.replace(archive)
    finally:
        staging.unlink(missing_ok=True)
    checksum = sha256_file(archive)
    Path(str(archive) + ".sha256").write_text(f"{checksum}  {archive.name}\n", encoding="utf-8")
    return archive


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    parser.add_argument("--target", help="Cargo target triple when building with --target")
    parser.add_argument("--version", help="Expected Cargo package version, optionally prefixed with v")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "artifacts" / "dzc")
    args = parser.parse_args(argv)
    try:
        actual = tomllib.loads((ROOT / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
        version = requested_version(args.version, actual)
        binary = args.binary or default_binary(args.platform, args.target)
        print(package(args.platform, binary, version, args.output_dir))
        return 0
    except (OSError, ValueError, KeyError, struct.error, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"dzc package: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
