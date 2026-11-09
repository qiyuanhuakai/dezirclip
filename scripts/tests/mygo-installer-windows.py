"""Exercise the actual MyGo installer only on disposable Windows CI runners."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import winreg

KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip-MyGo'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(installer, stage):
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise RuntimeError('Installer execution requires a disposable GitHub-hosted runner')
    root = Path(os.environ['LOCALAPPDATA']) / 'Programs/DezirClip-MyGo'
    if root.exists():
        raise RuntimeError('Refusing to modify a pre-existing MyGo installation')
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY):
            raise RuntimeError('Pre-existing MyGo registration')
    except FileNotFoundError:
        pass
    subprocess.run([str(installer), '/S'], check=True, timeout=90)
    expected = {path.relative_to(stage): digest(path) for path in stage.rglob('*') if path.is_file()}

    def assert_payload():
        assert all(digest(root / relative) == value for relative, value in expected.items())
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as key:
            assert winreg.QueryValueEx(key, 'InstallLocation')[0] == str(root)

    assert_payload()
    shortcut = Path(os.environ['APPDATA']) / 'Microsoft/Windows/Start Menu/Programs/DezirClip MyGo/DezirClip MyGo.lnk'
    assert shortcut.is_file()
    subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-STA', '-File',
                    str(Path(__file__).with_name('mygo-desktop-windows.ps1')),
                    '-Desktop', str(root / 'dzc-desktop.exe')], check=True, timeout=240)
    preserved = root / 'user-preserved.txt'
    preserved.write_text('Keep user data', encoding='utf-8')
    subprocess.run([str(installer), '/S'], check=True, timeout=90)
    assert_payload()
    # A held executable must prevent both replacement and removal, without killing processes.
    import ctypes
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p,
                                  ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    kernel.CreateFileW.restype = ctypes.c_void_p
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.CreateFileW(str(root / 'dzc-service.exe'), 0x80000000, 0, None, 3, 0, None)
    assert handle not in (None, ctypes.c_void_p(-1).value)
    uninstaller = root / 'uninstall.exe'
    try:
        assert subprocess.run([str(installer), '/S'], timeout=90).returncode != 0
        assert subprocess.run([str(uninstaller), '/S', '_?=' + str(root)], timeout=90).returncode != 0
    finally:
        kernel.CloseHandle(handle)
    assert_payload()
    subprocess.run([str(uninstaller), '/S', '_?=' + str(root)], check=True, timeout=90)
    assert all(not (root / relative).exists() for relative in expected)
    assert preserved.read_text(encoding='utf-8') == 'Keep user data'
    assert not shortcut.exists()
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY):
            raise AssertionError('Uninstall registration remains')
    except FileNotFoundError:
        pass
    subprocess.run([str(installer), '/S'], check=True, timeout=90)
    assert_payload()
    assert preserved.read_text(encoding='utf-8') == 'Keep user data'
    subprocess.run([str(uninstaller), '/S', '_?=' + str(root)], check=True, timeout=90)
    assert all(not (root / relative).exists() for relative in expected)
    assert preserved.read_text(encoding='utf-8') == 'Keep user data'
    # Reproduce the on-disk state of an interrupted install: durable directory
    # ownership and a partial executable, without an uninstall registration.
    assert (root / 'mygo-installation.ini').is_file()
    (root / 'dzc.exe').write_bytes(b'interrupted payload')
    subprocess.run([str(installer), '/S'], check=True, timeout=90)
    assert_payload()
    assert preserved.read_text(encoding='utf-8') == 'Keep user data'
    subprocess.run([str(uninstaller), '/S', '_?=' + str(root)], check=True, timeout=90)
    print('MYGO_INSTALL_RECOVERY_UPGRADE_LOCK_UNINSTALL_REINSTALL_PASSED', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--installer', type=Path, required=True)
    parser.add_argument('--stage', type=Path, required=True)
    args = parser.parse_args()
    verify(args.installer.resolve(strict=True), args.stage.resolve(strict=True))
