#!/usr/bin/env python3
"""Build isolated standalone artifacts: Ubuntu 22.04 sysroot or static MSVC CRT."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys

from dzc_compat import enforce, inspect_binary

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"linux-x86_64": "x86_64-unknown-linux-gnu", "linux-aarch64": "aarch64-unknown-linux-gnu",
           "windows-x86_64": "x86_64-pc-windows-msvc", "windows-aarch64": "aarch64-pc-windows-msvc"}


def run(arguments, **kwargs):
    subprocess.run(arguments, check=True, cwd=ROOT, stdout=sys.stderr, **kwargs)


def build(platform, directory):
    target = TARGETS[platform]
    directory = directory.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    binary = directory / target / "release" / ("dzc.exe" if platform.startswith("windows-") else "dzc")
    if platform.startswith("windows-"):
        linker = shutil.which("link.exe")
        if linker:
            resolved_linker = Path(linker).resolve()
            if resolved_linker.parent.name.lower() == "bin" and resolved_linker.parent.parent.name.lower() == "usr":
                raise ValueError(f"GNU link shadows the MSVC linker: {resolved_linker}; run from PowerShell or a native Python process with the MSVC environment")
        environment = {**os.environ, "CARGO_ENCODED_RUSTFLAGS": "-C\x1ftarget-feature=+crt-static",
                       "CARGO_BUILD_JOBS": "4"}
        environment.pop("RUSTFLAGS", None)
        command = ["cargo", "--manifest-path", str(ROOT / "src-tauri" / "Cargo.toml")]
        common = ["--locked", "--no-default-features", "--release", "--target", target, "--target-dir", str(directory)]
        run([command[0], "build", *command[1:], "--bin", "dzc", *common], env=environment)
        before = hashlib.sha256(binary.read_bytes()).hexdigest()
        run([command[0], "check", *command[1:], "--lib", "--bin", "dzc", *common], env=environment)
        if platform == "windows-x86_64":
            run([command[0], "test", *command[1:], "--lib", "--bin", "dzc", *common], env=environment)
        if hashlib.sha256(binary.read_bytes()).hexdigest() != before:
            raise ValueError("cargo check replaced the standalone CLI artifact")
    else:
        active = subprocess.check_output(["rustup", "show", "active-toolchain"], text=True).split()[0]
        rustup = subprocess.check_output(["rustup", "show", "home"], text=True).strip()
        cargo_bin = str(Path(shutil.which("cargo")).parent)
        image = "dezirclip-cli-builder:ubuntu22.04"
        run(["docker", "build", "-t", image, "-f", "scripts/compat/ubuntu22.Dockerfile", "scripts/compat"])
        script = r'''
set -eu
mkdir -p /work/project /work/home
if [ -d /registry-seed ] && [ ! -d /cargo/registry/index ]; then
    mkdir -p /cargo/registry
    cp -a /registry-seed/. /cargo/registry/
fi
tar -C /source --exclude=src-tauri/target --exclude=src-tauri/binaries --exclude=src-tauri/gen \
    --exclude=src-tauri/.env --exclude='src-tauri/.env.*' -cf - src-tauri skills/dzc-cli skills/dezirclip-theme \
    | tar -C /work/project -xf -
cd /work/project
cargo build --manifest-path src-tauri/Cargo.toml --release --bin dzc --locked --no-default-features --target "$DZC_BUILD_TARGET"
binary="/target/$DZC_BUILD_TARGET/release/dzc"
before="$(sha256sum "$binary")"
cargo check --manifest-path src-tauri/Cargo.toml --release --lib --bin dzc --locked --no-default-features --target "$DZC_BUILD_TARGET"
cargo test --manifest-path src-tauri/Cargo.toml --release --lib --bin dzc --locked --no-default-features --target "$DZC_BUILD_TARGET"
test "$before" = "$(sha256sum "$binary")"
'''
        seed = Path(os.environ.get("CARGO_HOME", Path(cargo_bin).parent)) / "registry"
        seed_mount = ["-v", f"{seed}:/registry-seed:ro"] if seed.is_dir() else []
        run(["docker", "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
             "-v", f"{ROOT}:/source:ro", "-v", f"{directory}:/target",
             "-v", f"{rustup}:/rustup:ro", "-v", f"{cargo_bin}:/rust-tools:ro",
             "-v", "dzc-cli-cargo-ubuntu22:/cargo",
             *seed_mount,
             "-e", "RUSTUP_HOME=/rustup", "-e", f"RUSTUP_TOOLCHAIN={active}",
             "-e", "CARGO_HOME=/cargo", "-e", "CARGO_TARGET_DIR=/target",
             "-e", "CARGO_BUILD_JOBS=4",
             "-e", "HOME=/work/home", "-e", "PATH=/rust-tools:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
             "-e", f"DZC_BUILD_TARGET={target}", image, "bash", "-c", script])
    enforce(inspect_binary(binary, platform))
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=TARGETS, required=True)
    parser.add_argument("--target-dir", type=Path)
    args = parser.parse_args()
    try:
        directory = args.target_dir or ROOT / ".harness" / "cli-target" / args.platform
        print(build(args.platform, directory))
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"standalone dzc build: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
