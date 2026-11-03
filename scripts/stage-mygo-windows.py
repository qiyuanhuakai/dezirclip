"""Stage and exercise the three sibling executables in the Windows x64 test download."""
import argparse
import hashlib
import json
import os
import runpy
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile


def report(command, env=None):
    return json.loads(subprocess.check_output(command, env=env, timeout=30, text=True, encoding="utf-8"))


def verify_archive(archive, stage):
    expected = {path.relative_to(stage).as_posix(): path for path in stage.rglob("*") if path.is_file()}
    required = {"dzc.exe", "dzc-desktop.exe", "dzc-service.exe",
                "skills/dzc-cli/SKILL.md", "skills/dezirclip-theme/SKILL.md"}
    if not required.issubset(expected):
        raise ValueError("Staged MyGo distribution is missing executables or guides")
    with zipfile.ZipFile(archive) as package:
        entries = [entry for entry in package.infolist() if not entry.is_dir()]
        names = [entry.filename.replace("\\", "/") for entry in entries]
        if len(names) != len(expected) or set(names) != set(expected):
            raise ValueError("MyGo ZIP file list differs from the staged distribution")
        for name, entry in zip(names, entries):
            with package.open(entry) as packed, expected[name].open("rb") as source:
                if hashlib.file_digest(packed, "sha256").digest() != hashlib.file_digest(source, "sha256").digest():
                    raise ValueError(f"MyGo ZIP has different bytes for {name}")


def stage(root, output, revision, version):
    if os.name != "nt":
        raise RuntimeError("Run the package acceptance on Windows")
    target = "x86_64-pc-windows-msvc"
    output.mkdir(parents=True, exist_ok=False)
    desktop = output / "dzc-desktop.exe"
    cli = output / "dzc.exe"
    service = output / "dzc-service.exe"
    shutil.copy2(root / "artifacts/dzc-desktop" / target / desktop.name, desktop)
    for binary in (cli, service):
        shutil.copy2(root / "target/release" / binary.name, binary)
    resources = runpy.run_path(str(root / "scripts/package-dzc.py"))["skill_files"](root)
    for relative, source in resources.items():
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    tools = report([cli, "desktop-tools", "info"])
    if (Path(tools["cli_path"]) != cli or tools["cli_version"] != version
            or Path(tools["skill_path"]) != output / "skills/dzc-cli"
            or Path(tools["theme_skill_path"]) != output / "skills/dezirclip-theme"):
        raise RuntimeError("Packaged desktop tools did not identify its CLI and both bundled guides")
    for role, command in (
        ("mygo-desktop", [desktop, "--build-info"]),
        ("cli", [cli, "build-info", "--json"]),
        ("service", [service, "--build-info"]),
    ):
        info = report(command)
        expected = {"component":role, "target":target, "component_version":version,
                    "source_revision":revision, "source_state":"verified_checkout", "build_id":f"{version}@{revision}"}
        if any(info.get(key) != value for key, value in expected.items()):
            raise RuntimeError(f"Unexpected {role} build identity")
    web_root = root / "dist/mygo-web"
    expected_assets = {
        path.relative_to(web_root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in web_root.rglob("*") if path.is_file()
    }
    if "index.html" not in expected_assets or report([desktop, "--web-asset-hashes"]) != expected_assets:
        raise RuntimeError("Packaged MyGo WebView assets differ from the shared React production build")
    with tempfile.TemporaryDirectory(prefix="dzc-mygo-package-") as temp:
        env = {**os.environ, "DEZIRCLIP_DB_PATH":str(Path(temp) / "clipboard.db")}
        started = report([cli, "service", "start", "--no-capture", "--json"], env)
        try:
            if started["mode"] != "service" or started["capture_owner"]:
                raise RuntimeError("Unexpected isolated Host")
            attached = report([cli, "service", "desktop-endpoint"], env)
            if attached["pid"] <= 0 or not attached["instance"] or attached["version"] != version:
                raise RuntimeError("Desktop endpoint discovery failed")
        finally:
            subprocess.run([cli, "service", "stop"], env=env, check=True, timeout=30)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--revision")
    mode.add_argument("--verify-archive", type=Path)
    parser.add_argument("--tag", help="For a tagged release, require the manifest to match this tag")
    args = parser.parse_args()
    if args.verify_archive:
        verify_archive(args.verify_archive, args.output)
        raise SystemExit(0)
    root = Path(__file__).resolve().parents[1]
    version = json.loads((root / "package.json").read_text(encoding="utf-8"))["version"]
    if args.tag is not None and args.tag != "v" + version:
        raise ValueError("Package version must match the release tag")
    stage(root, args.output.resolve(), args.revision, version)
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-STA", "-File",
                    str(root / "scripts/tests/mygo-desktop-windows.ps1"),
                    "-Desktop", str(args.output.resolve() / "dzc-desktop.exe")], check=True, timeout=240)
