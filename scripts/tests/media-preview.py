"""Decode a synthetic H.264 clip with five blue frames followed by five red frames.

Regenerate two-colors.mp4 with ffmpeg using 160x120 blue/red lavfi color sources
(0.5 s each at 10 fps), the concat filter, libx264, yuv420p and +faststart.
"""
import os
from pathlib import Path
import subprocess
import tempfile

with tempfile.TemporaryDirectory(prefix="dzc-media-fixture-") as folder:
    root = Path(folder)
    video = Path(__file__).with_name("two-colors.mp4").resolve(strict=True)
    subprocess.run(["cargo", "run", "--locked", "-p", "dzc-platform", "--example", "video_preview_fixture", "--", str(video)], check=True)
    if os.name == "nt" and os.environ.get("GITHUB_ACTIONS") == "true":
        subprocess.run(["cargo", "build", "--locked", "-p", "dzc-platform", "--example", "native_crash_fixture"], check=True)
        result = subprocess.run(["target/debug/examples/native_crash_fixture.exe", str(root)], timeout=20)
        assert result.returncode != 0, "native exception must not be swallowed"
        log = (root / "service-native-exit.log").read_text()
        assert "code=0xc0000005" in log and "module=" in log and "offset=" in log, log
        assert str(root) not in log, "diagnostics must not include user paths"
        print("Native exception module and address recording passed")
