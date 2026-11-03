"""Stage and exercise the three sibling executables in the Windows x64 test download."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def report(command, env=None):
    return json.loads(subprocess.check_output(command, env=env, timeout=30, text=True, encoding="utf-8"))


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
    parser.add_argument("--revision", required=True)
    parser.add_argument("--tag", help="For a tagged release, require the manifest to match this tag")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = json.loads((root / "package.json").read_text(encoding="utf-8"))["version"]
    if args.tag is not None and args.tag != "v" + version:
        raise ValueError("Package version must match the release tag")
    stage(root, args.output.resolve(), args.revision, version)
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-STA", "-File",
                    str(root / "scripts/tests/mygo-desktop-windows.ps1"),
                    "-Desktop", str(args.output.resolve() / "dzc-desktop.exe")], check=True, timeout=150)
