"""Run the production adapter's std-only, deterministic timing partition tests."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


@unittest.skipUnless(shutil.which("rustc"), "The deterministic Rust timer checks require rustc")
class CaptureGuardTimingTests(unittest.TestCase):
    def test_guard_excludes_publish_and_preserves_callback_reentrancy(self):
        source = Path(__file__).resolve().parents[1] / "capture/exclusive_guard.rs"
        with tempfile.TemporaryDirectory(prefix="dzc-exclusive-guard-") as directory:
            binary = Path(directory) / ("guard-tests.exe" if os.name == "nt" else "guard-tests")
            subprocess.run(["rustc", "--edition=2021", "--test", str(source), "-o", str(binary)],
                           check=True, capture_output=True, text=True)
            result = subprocess.run([str(binary), "--nocapture"], check=True,
                                    capture_output=True, text=True)
            self.assertIn("4 passed; 0 failed", result.stdout)


if __name__ == "__main__":
    unittest.main()
