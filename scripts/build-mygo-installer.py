"""Wrap the verified MyGo distribution in a separate per-user NSIS installer."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def nsis_string(value):
    return str(value).replace('$', '$$').replace('"', '$\\"')


def build(stage, output, version):
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?", version):
        raise ValueError("Invalid version")
    stage = stage.resolve(strict=True)
    files = sorted(path for path in stage.rglob('*') if path.is_file())
    names = {path.relative_to(stage).as_posix() for path in files}
    if not {"dzc-desktop.exe", "dzc.exe", "dzc-service.exe", "skills/dzc-cli/SKILL.md", "skills/dezirclip-theme/SKILL.md"}.issubset(names):
        raise ValueError("Missing verified MyGo payload")
    if any(path.is_symlink() or not path.resolve().is_relative_to(stage) for path in stage.rglob('*')):
        raise ValueError("Payload must not contain links")
    if any(not re.fullmatch(r"[A-Za-z0-9_./-]+", name) for name in names):
        raise ValueError("Unsupported payload filename")
    compiler = shutil.which('makensis') or str(Path(os.environ.get('LOCALAPPDATA', '')) / 'tauri/NSIS/makensis.exe')
    if not Path(compiler).is_file():
        compiler = str(Path(os.environ.get('ProgramFiles(x86)', 'C:/Program Files (x86)')) / 'NSIS/makensis.exe')
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='mygo-installer-') as temporary:
        root = Path(temporary)
        install, remove = [], []
        directories = set()
        for path in files:
            relative = path.relative_to(stage)
            parent = str(relative.parent).replace('/', '\\')
            install.extend([f'SetOutPath "$INSTDIR\\{parent}"', f'File "{nsis_string(path)}"'])
            remove.append(f'Delete "$INSTDIR\\{str(relative).replace(chr(47), chr(92))}"')
            remove.extend(['${If} ${Errors}', 'SetErrorLevel 5',
                           'Abort "Could not remove a MyGo file. Close the application and retry."', '${EndIf}'])
            directories.update(relative.parents)
        for directory in sorted(directories, key=lambda p: len(p.parts), reverse=True):
            if str(directory) != '.':
                remove.append(f'RMDir "$INSTDIR\\{str(directory).replace(chr(47), chr(92))}"')
        (root / 'install.nsh').write_text('\n'.join(install), encoding='utf-8-sig')
        (root / 'remove.nsh').write_text('\n'.join(remove), encoding='utf-8-sig')
        subprocess.run([compiler, '/V2', f'/DOUTPUT={output}', f'/DVERSION={version}',
                        f'/DINSTALL_FILES={root / "install.nsh"}', f'/DREMOVE_FILES={root / "remove.nsh"}',
                        str(Path(__file__).with_name('mygo-installer.nsi'))], check=True)
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    build(args.stage, args.output, args.version)
