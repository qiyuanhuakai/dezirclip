"""Read the checkout's release version and derive its Tauri RPM naming."""
import json
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
NUMBER = r'(0|[1-9][0-9]*)'
RPM_VERSION = re.compile(
    rf'{NUMBER}\.{NUMBER}\.{NUMBER}(?:-(alpha|beta|rc)\.{NUMBER})?'
    r'(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?'
)


def rpm_parts(version):
    match = RPM_VERSION.fullmatch(version) if isinstance(version, str) else None
    if match is None:
        raise ValueError('Unsupported RPM release version')
    base = '.'.join(match.group(index) for index in (1, 2, 3))
    release = f'0.{match[4]}.{match[5]}' if match[4] else '1'
    return base, release


def release_version(root=None):
    path = (Path(root) if root is not None else ROOT) / 'package.json'
    package = json.loads(path.read_text(encoding='utf-8'))
    version = package.get('version') if isinstance(package, dict) else None
    rpm_parts(version)
    return version
