import importlib.util
from pathlib import Path
import tempfile
import unittest
import json
import os
import subprocess
import sys
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("native_wayland", ROOT / "test-native-wayland.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeWaylandGuards(unittest.TestCase):
    def test_inner_and_outer_timeouts_preserve_real_partial_output_and_case(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = [sys.executable, "-c", "import sys,time; print('synthetic stdout',flush=True); print('synthetic stderr',file=sys.stderr,flush=True); time.sleep(10)"]
            for label in ["inner-case", "outer"]:
                cases = []
                with self.assertRaises(subprocess.TimeoutExpired):
                    MODULE.run_logged(root, label, command, dict(os.environ), timeout=1, cases=cases)
                self.assertIn(b"synthetic stdout", (root / (label + ".stdout.log")).read_bytes())
                self.assertIn(b"synthetic stderr", (root / (label + ".stderr.log")).read_bytes())
                receipt = json.loads((root / (label + ".result.json")).read_text())
                self.assertEqual(receipt, cases[0])
                self.assertEqual(receipt["status"], "timeout")
                self.assertEqual(receipt["command"], command)
                self.assertEqual(receipt["timeout_seconds"], 1)
                self.assertIsNone(receipt["exit_code"])

    def test_timeout_output_accepts_text_bytes_and_missing_streams(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            error = subprocess.TimeoutExpired(["synthetic-command"], .05, output="text output", stderr=b"byte error")
            with mock.patch.object(MODULE.subprocess, "run", side_effect=error), self.assertRaises(subprocess.TimeoutExpired):
                MODULE.run_logged(root, "mixed", ["synthetic-command"], None, timeout=.05)
            self.assertEqual((root / "mixed.stdout.log").read_bytes(), b"text output")
            self.assertEqual((root / "mixed.stderr.log").read_bytes(), b"byte error")
            error = subprocess.TimeoutExpired(["synthetic-command"], .05)
            with mock.patch.object(MODULE.subprocess, "run", side_effect=error), self.assertRaises(subprocess.TimeoutExpired):
                MODULE.run_logged(root, "empty", ["synthetic-command"], None, timeout=.05)
            self.assertEqual((root / "empty.stdout.log").read_bytes(), b"")
            self.assertEqual((root / "empty.stderr.log").read_bytes(), b"")

    def test_unmarked_or_broad_runtime_mount_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "lib").mkdir()
            with self.assertRaises(ValueError):
                MODULE.private_libraries(root)
        with self.assertRaises((ValueError, FileNotFoundError)):
            MODULE.private_libraries(Path("/"))

    def test_directory_is_not_an_executable_input(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                MODULE.checked_file(directory)

    def test_prepare_never_launches_a_process_or_creates_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "synthetic-input"
            binary.write_bytes(b"not executed")
            output = root / "must-not-exist"
            arguments = ["tool", "--test-binary", str(binary), "--cli", str(binary), "--compositor", str(binary),
                         "--swaymsg", str(binary), "--protocol-fixture", str(binary), "--expected-protocol", "ext", "--output", str(output)]
            with mock.patch("sys.argv", arguments), mock.patch.object(MODULE.subprocess, "run", side_effect=AssertionError("no process in preparation")), mock.patch("builtins.print"):
                MODULE.main()
            self.assertFalse(output.exists())

    def test_output_overwrite_is_rejected_before_any_subprocess(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "synthetic-input"
            binary.write_bytes(b"not executed")
            arguments = ["tool", "--test-binary", str(binary), "--cli", str(binary), "--compositor", str(binary),
                         "--swaymsg", str(binary), "--protocol-fixture", str(binary), "--expected-protocol", "wlr", "--output", str(root), "--execute"]
            with mock.patch("sys.argv", arguments), mock.patch.object(MODULE.subprocess, "run", side_effect=AssertionError("guard before launch")):
                with self.assertRaises(ValueError):
                    MODULE.main()


if __name__ == "__main__":
    unittest.main()
