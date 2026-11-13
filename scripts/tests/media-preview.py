"""Synthetic media only; no desktop, clipboard or user history access."""
import os
from pathlib import Path
import subprocess
import tempfile

with tempfile.TemporaryDirectory(prefix="dzc-media-fixture-") as folder:
    root = Path(folder)
    video = root / "two-colors.mp4"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=160x120:d=0.5:r=10",
        "-f", "lavfi", "-i", "color=c=red:s=160x120:d=0.5:r=10", "-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(video)], check=True, timeout=30)
    subprocess.run(["cargo", "run", "--locked", "-p", "dzc-platform", "--example", "video_preview_fixture", "--", str(video)], check=True)
    if os.name == "nt" and os.environ.get("GITHUB_ACTIONS") == "true":
        subprocess.run(["cargo", "build", "--locked", "-p", "dzc-platform", "--example", "native_crash_fixture"], check=True)
        result = subprocess.run(["target/debug/examples/native_crash_fixture.exe", str(root)], timeout=20)
        assert result.returncode != 0, "native exception must not be swallowed"
        log = (root / "service-native-exit.log").read_text()
        assert "code=0xc0000005" in log and "module=" in log and "offset=" in log, log
        assert str(root) not in log, "diagnostics must not include user paths"
        print("Native exception module and address recording passed")
