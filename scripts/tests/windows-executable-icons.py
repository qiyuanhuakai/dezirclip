"""Verify the actual Windows icon resources without launching the executables."""
import ctypes
from ctypes import wintypes
from pathlib import Path
import sys


def verify(path):
    shell = ctypes.WinDLL('shell32')
    shell.ExtractIconExW.argtypes = [wintypes.LPCWSTR, ctypes.c_int, ctypes.POINTER(wintypes.HICON), ctypes.POINTER(wintypes.HICON), wintypes.UINT]
    shell.ExtractIconExW.restype = wintypes.UINT
    user = ctypes.WinDLL('user32')
    user.DestroyIcon.argtypes = [wintypes.HICON]
    large, small = wintypes.HICON(), wintypes.HICON()
    try:
        count = shell.ExtractIconExW(str(path.resolve(strict=True)), 0, ctypes.byref(large), ctypes.byref(small), 1)
        if count < 1 or not large or not small:
            raise RuntimeError(f'{path.name}: missing usable large/small Windows icons')
        print(f'{path.name}: large and small icons extracted')
    finally:
        for icon in [large, small]:
            if icon:
                user.DestroyIcon(icon)


if __name__ == '__main__':
    for name in sys.argv[1:]:
        verify(Path(name))
