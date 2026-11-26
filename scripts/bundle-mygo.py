"""Package native desktop binaries alongside the release's existing Rust sidecars."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile

from release_identities import inspect_machine
from release_version import rpm_parts

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'windows-x86_64': 'x86_64-pc-windows-msvc',
    'windows-aarch64': 'aarch64-pc-windows-msvc',
    'linux-x86_64': 'x86_64-unknown-linux-gnu',
    'linux-aarch64': 'aarch64-unknown-linux-gnu',
}
PRODUCT = 'DezirClip-MyGo'


def run(command, **options):
    subprocess.run(command, check=True, **options)


def stage(stage_dir, target, sidecars, native, version, revision):
    windows, arch = target.split('-')
    windows = windows == 'windows'
    ext = '.exe' if windows else ''
    if target == 'windows-aarch64':
        app = native / ('dzc-desktop' + ext)
        proof = json.loads(app.with_suffix('.exe.proof.json').read_text(encoding='utf-8'))
        expected = {'target': TARGETS[target], 'component_version': version, 'source_revision': revision,
                    'source_state': 'verified_checkout'}
        if any(proof['expected'].get(key) != value for key, value in expected.items()):
            raise ValueError('Unexpected Windows ARM64 desktop build proof')
        with app.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != proof['sha256']:
                raise ValueError('Windows ARM64 desktop differs from its build proof')
    for name, role, folder, args in (
        ('dzc-desktop', 'mygo-desktop', native, ['--build-info']),
        ('dzc', 'cli', sidecars, ['build-info', '--json']),
        ('dzc-service', 'service', sidecars, ['--build-info']),
    ):
        source = folder / (name + ext)
        inspect_machine(source, target)
        if target != 'windows-aarch64':
            info = json.loads(subprocess.check_output([source, *args], timeout=30))
            expected = {'component': role, 'target': TARGETS[target], 'component_version': version,
                        'source_revision': revision, 'source_state': 'verified_checkout'}
            if any(info.get(key) != value for key, value in expected.items()):
                raise ValueError(f'Unexpected {target} {role} build identity')
        shutil.copy2(source, stage_dir / source.name)
    skills = runpy.run_path(str(ROOT / 'scripts/package-dzc.py'))['skill_files'](ROOT)
    for name, source in skills.items():
        destination = stage_dir / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def linux_bundle(work, stage_dir, out, version, pnpm):
    """Use Tauri's Linux bundle tools on the staged Go binaries without building a Rust GUI."""
    project = work / 'bundle-project'
    (project / 'src').mkdir(parents=True)
    (project / 'src/main.rs').write_text('fn main() {}\n', encoding='utf-8')
    (project / 'Cargo.toml').write_text(
        f'[package]\nname = "dezirclip-mygo"\nversion = "{version}"\nedition = "2021"\nlicense = "GPL-3.0"\n',
        encoding='utf-8')
    cargo_target = project / 'target'
    binary_dir = cargo_target / 'release'
    binary_dir.mkdir(parents=True)
    launcher = binary_dir / 'dezirclip-mygo'
    launcher.write_text(
        '#!/bin/sh\n'
        'case "$0" in /*) self="$0" ;; *) self="$(command -v -- "$0")" ;; esac\n'
        'exec "$(dirname -- "$(readlink -f -- "$self")")/../lib/DezirClip-MyGo/dzc-desktop" "$@"\n',
        encoding='utf-8')
    launcher.chmod(0o755)
    source_config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    icon_paths = [str(ROOT / 'src-tauri' / filename) for filename in source_config['bundle']['icon']
                  if filename.endswith('.png')]
    configuration = {
        'productName': PRODUCT, 'version': version, 'identifier': 'io.github.qiyuanhuakai.dezirclip.mygo',
        'bundle': {
            'active': True, 'icon': icon_paths, 'category': 'Utility',
            'shortDescription': 'DezirClip clipboard history with the MyGo native desktop',
            'resources': {str(path): path.relative_to(stage_dir).as_posix()
                          for path in stage_dir.rglob('*') if path.is_file()},
            'linux': {
                'deb': {'depends': ['libgtk-3-0t64 | libgtk-3-0', 'libgstreamer1.0-0', 'gstreamer1.0-plugins-base',
                                    'gstreamer1.0-plugins-good',
                                    'libx11-6', 'libxi6', 'libxrandr2', 'libxtst6', 'libdbus-1-3', 'xdg-utils']},
                'rpm': {'depends': ['gtk3', 'gstreamer1', 'gstreamer1-plugins-base', 'gstreamer1-plugins-good',
                                    'libX11', 'libXi', 'libXrandr',
                                    'libXtst', 'dbus-libs', 'xdg-utils']},
            },
        },
    }
    (project / 'tauri.conf.json').write_text(json.dumps(configuration), encoding='utf-8')
    env = {**os.environ, 'TAURI_APP_PATH': str(project), 'TAURI_FRONTEND_PATH': str(ROOT),
           'CARGO_TARGET_DIR': str(cargo_target), 'APPIMAGE_EXTRACT_AND_RUN': '1'}
    command = [pnpm, 'exec', 'tauri', 'bundle', '--ci', '--no-binary-patching', '--bundles']
    run([*command, 'deb,appimage'], cwd=ROOT, env=env)
    base, rpm_release = rpm_parts(version)
    override = {'version': base, 'bundle': {'linux': {'rpm': {'release': rpm_release}}}}
    run([*command, 'rpm', '--config', json.dumps(override)], cwd=ROOT, env=env)
    for directory, suffix in (('deb', '.deb'), ('appimage', '.AppImage'), ('rpm', '.rpm')):
        files = list((binary_dir / 'bundle' / directory).glob('*' + suffix))
        if len(files) != 1:
            raise ValueError(f'Expected exactly one MyGo {suffix} package')
        shutil.copy2(files[0], out / files[0].name)


def verify_linux(work, stage_dir, out, version, revision):
    deb, = out.glob('*.deb')
    image, = out.glob('*.AppImage')
    extraction = work / 'installed-deb'
    run(['dpkg-deb', '-x', deb, extraction])
    for source in stage_dir.rglob('*'):
        if source.is_file():
            extracted = extraction / 'usr/lib' / PRODUCT / source.relative_to(stage_dir)
            with source.open('rb') as expected, extracted.open('rb') as actual:
                if hashlib.file_digest(expected, 'sha256').digest() != hashlib.file_digest(actual, 'sha256').digest():
                    raise ValueError(f'Debian payload differs for {source.name}')
    for launcher, env in ((extraction / 'usr/bin/dezirclip-mygo', os.environ),
                          (image, {**os.environ, 'APPIMAGE_EXTRACT_AND_RUN': '1'})):
        info = json.loads(subprocess.check_output([launcher, '--build-info'], env=env, timeout=180))
        if (info.get('component'), info.get('component_version'), info.get('source_revision')) != ('mygo-desktop', version, revision):
            raise ValueError(f'Unexpected desktop identity from {launcher.name}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS, required=True)
    parser.add_argument('--sidecars', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--smoke', action='store_true')
    args = parser.parse_args()
    version = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))['version']
    if args.tag != 'v' + version:
        raise ValueError('Desktop package version differs from the tag')
    pnpm = shutil.which('pnpm')
    if not pnpm:
        raise ValueError('pnpm is required for packaging and signing')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix='dezirclip-mygo-') as temporary:
        work = Path(temporary)
        stage_dir = work / 'stage'
        stage_dir.mkdir()
        stage(stage_dir, args.target, args.sidecars, args.native, version, args.revision)
        if args.target.startswith('linux-'):
            linux_bundle(work, stage_dir, output, version, pnpm)
            if args.smoke:
                verify_linux(work, stage_dir, output, version, args.revision)
        else:
            arm = args.target == 'windows-aarch64'
            arch = 'arm64' if arm else 'x64'
            installer = output / f'DezirClip-v{version}-windows-{arch}-MyGo-setup.exe'
            build = runpy.run_path(str(ROOT / 'scripts/build-mygo-installer.py'))['build']
            build(stage_dir, installer, version, arm64=arm, fast_compression=True)
            if args.smoke and not arm:
                verify = runpy.run_path(str(ROOT / 'scripts/tests/mygo-installer-windows.py'))['verify']
                verify(installer, stage_dir, native_main=True, focused=True)
    for file in output.iterdir():
        if file.suffix in {'.AppImage', '.exe', '.deb'}:
            run([pnpm, 'exec', 'tauri', 'signer', 'sign', '--app-version', version, file], cwd=ROOT)


if __name__ == '__main__':
    main()
