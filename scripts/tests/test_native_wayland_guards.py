import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("native_wayland", ROOT / "test-native-wayland.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeWaylandGuards(unittest.TestCase):
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
