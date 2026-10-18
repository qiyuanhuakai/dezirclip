Flatpak smoke-test fixtures:

- `ocr-eng.png`: synthetic `DEZIRCLIP 12345`, rendered with DejaVu Sans.
- `ocr-chi-sim.png`: synthetic `中文测试`, rendered with Noto Sans CJK.
- `x11-paste-target.c`: disposable X11 clipboard owner and paste target. It accepts an actual Ctrl+V event and checks a UTF-8/CRLF selection transfer between its two windows.

`scripts/verify-flatpak.py` uses these in the final GNOME Platform runtime and a private Xvfb server. It does not use the user's clipboard, desktop windows, or history database.
