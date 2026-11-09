"""Package the immutable alpha.9 payload without rebuilding its released binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

TAG = 'v0.2.0-alpha.9'
REVISION = 'c48ca6caaa4e6f3eb670e9a6957a90934e6a149c'
ASSET = f'DezirClip-{TAG}-windows-x64-MyGo.zip'
DIGEST = '135c6584ded51fdde6319af73d36be3055d5591c915258942ee9cee3fe64141b'
REPO = 'qiyuanhuakai/dezirclip'


def prepare(output):
    release = json.loads(subprocess.check_output(['gh', 'release', 'view', TAG, '--repo', REPO,
                                                  '--json', 'databaseId,isDraft,assets']))
    if release['isDraft'] or release['databaseId'] != 438668292:
        raise ValueError('Expected the original public alpha.9 release')
    asset = next(asset for asset in release['assets'] if asset['name'] == ASSET)
    if asset['digest'] != 'sha256:' + DIGEST:
        raise ValueError('Published MyGo payload changed')
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run(['gh', 'release', 'download', TAG, '--repo', REPO, '--pattern', ASSET,
                    '--dir', str(output)], check=True)
    archive = output / ASSET
    if hashlib.sha256(archive.read_bytes()).hexdigest() != DIGEST:
        raise ValueError('Downloaded payload digest mismatch')
    stage = output / 'stage'
    stage.mkdir()
    with zipfile.ZipFile(archive) as package:
        for entry in package.infolist():
            relative = Path(entry.filename)
            if relative.is_absolute() or '..' in relative.parts or ':' in entry.filename:
                raise ValueError('Unsafe ZIP entry')
        package.extractall(stage)
    for name, args, component in [('dzc-desktop.exe', ['--build-info'], 'mygo-desktop'),
                                   ('dzc-service.exe', ['--build-info'], 'service'),
                                   ('dzc.exe', ['build-info', '--json'], 'cli')]:
        info = json.loads(subprocess.check_output([str(stage / name), *args], timeout=30))
        if any(info.get(key) != value for key, value in {'component': component, 'component_version': TAG[1:],
               'target': 'x86_64-pc-windows-msvc', 'source_revision': REVISION, 'source_state': 'verified_checkout'}.items()):
            raise ValueError('Released binary identity mismatch')
    return stage


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.output.resolve())
