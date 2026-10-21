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
    def private_prefix(self, parent):
        prefix = parent / "dezirclip-private-runtime"
        prefix.mkdir()
        (prefix / "lib").mkdir()
        (prefix / "dezirclip-isolated-compositor-prefix").write_text(MODULE.MARKER)
        return prefix

    def symlink(self, link, target, directory=False):
        try:
            link.symlink_to(target, target_is_directory=directory)
        except OSError as error:
            self.skipTest("Symlink creation is unavailable on this host: " + str(error))

    def test_runtime_directory_links_are_checked_before_regular_files(self):
        for nested in (False, True):
            with self.subTest(nested=nested), tempfile.TemporaryDirectory() as directory:
                parent = Path(directory).resolve(strict=True)
                prefix = self.private_prefix(parent)
                outside = parent / "outside-library"
                outside.mkdir()
                (outside / "synthetic.so").write_bytes(b"must not be read")
                link = prefix / "lib/alias" if nested else prefix / "lib"
                if not nested:
                    link.rmdir()
                self.symlink(link, outside, directory=True)
                actual_digest = MODULE.digest
                def confined_digest(path):
                    if not Path(path).resolve().is_relative_to(prefix):
                        raise AssertionError("escape must fail before hashing its bytes")
                    return actual_digest(path)
                with mock.patch.object(MODULE, "PRIVATE_RUNTIME_PARENT", parent), \
                     mock.patch.object(MODULE, "digest", side_effect=confined_digest):
                    with self.assertRaisesRegex(ValueError, "escapes"):
                        MODULE.private_libraries(prefix)

    def test_in_tree_directory_links_are_explicitly_rejected_instead_of_omitted(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve(strict=True)
            prefix = self.private_prefix(parent)
            actual = prefix / "actual"
            actual.mkdir()
            (actual / "synthetic.so").write_bytes(b"same namespace")
            self.symlink(prefix / "lib/alias", actual, directory=True)
            with mock.patch.object(MODULE, "PRIVATE_RUNTIME_PARENT", parent), \
                 self.assertRaisesRegex(ValueError, "directory symlinks"):
                MODULE.private_libraries(prefix)

    def test_allowed_file_links_hash_bytes_and_retargeting_while_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve(strict=True)
            prefix = self.private_prefix(parent)
            actual = prefix / "lib/synthetic.so.1"
            actual.write_bytes(b"first bytes")
            link = prefix / "lib/synthetic.so"
            self.symlink(link, Path("synthetic.so.1"))
            with mock.patch.object(MODULE, "PRIVATE_RUNTIME_PARENT", parent):
                _, first = MODULE.private_libraries(prefix)
                actual.write_bytes(b"second bytes")
                _, second = MODULE.private_libraries(prefix)
                self.assertNotEqual(first, second)
                other = prefix / "lib/synthetic.so.2"
                other.write_bytes(b"second bytes")
                _, before_retarget = MODULE.private_libraries(prefix)
                link.unlink()
                self.symlink(link, Path("synthetic.so.2"))
                _, after_retarget = MODULE.private_libraries(prefix)
                self.assertNotEqual(before_retarget, after_retarget)
                key = "runtime/" + str(link.relative_to(prefix))
                self.assertEqual(after_retarget[key]["resolved_path"], str(other.resolve()))
                outside = parent / "outside.so"
                outside.write_bytes(b"outside bytes")
                link.unlink()
                self.symlink(link, outside)
                with self.assertRaisesRegex(ValueError, "escapes"):
                    MODULE.private_libraries(prefix)

    def test_library_link_guards_reach_their_assertions_under_a_temporary_root_alias(self):
        temporary_directory = tempfile.TemporaryDirectory
        with temporary_directory() as directory:
            root = Path(directory).resolve(strict=True)
            actual = root / "actual-temporary-root"
            actual.mkdir()
            alias = root / "alias-temporary-root"
            self.symlink(alias, actual, directory=True)
            self.assertNotEqual(alias, alias.resolve(strict=True))
            def under_alias(*args, **kwargs):
                return temporary_directory(*args, dir=alias, **kwargs)
            # Exercise the original three test bodies, including confinement
            # before hashing, byte changes, retargeting and directory rejection.
            with mock.patch.object(tempfile, "TemporaryDirectory", side_effect=under_alias):
                self.test_runtime_directory_links_are_checked_before_regular_files()
                self.test_in_tree_directory_links_are_explicitly_rejected_instead_of_omitted()
                self.test_allowed_file_links_hash_bytes_and_retargeting_while_escape_is_rejected()

    def test_disconnect_wait_timeout_keeps_partial_output_case_and_stops_owned_children(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / "runtime"
            runtime.mkdir()
            compositor = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"],
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            command = [sys.executable, "-c", "import sys,time,pathlib; pathlib.Path(sys.argv[1]).write_text('ready'); print('disconnect stdout',flush=True); print('disconnect stderr',file=sys.stderr,flush=True); time.sleep(30)", str(runtime / "disconnect-ready")]
            cases = []
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    MODULE.run_disconnect(root, runtime, command, compositor, dict(os.environ), cases, timeout=.1, ready_timeout=5)
            finally:
                MODULE.stop_owned(compositor)
            record = json.loads((root / "disconnect.result.json").read_text())
            self.assertEqual(cases, [record])
            self.assertEqual(record["status"], "timeout")
            self.assertEqual(record["phase"], "disconnect_wait")
            self.assertEqual(record["command"], command)
            self.assertEqual(record["timeout_seconds"], .1)
            self.assertIsNone(record["exit_code"])
            self.assertIsNotNone(record["final_exit_code"])
            self.assertIsNotNone(compositor.poll())
            self.assertIn(b"disconnect stdout", (root / "disconnect.stdout.log").read_bytes())
            self.assertIn(b"disconnect stderr", (root / "disconnect.stderr.log").read_bytes())

    def test_disconnect_early_exit_is_also_a_structured_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = [sys.executable, "-c", "import sys; print('early failure',flush=True); sys.exit(7)"]
            cases = []
            with self.assertRaisesRegex(RuntimeError, "without private owner readiness"):
                MODULE.run_disconnect(root, root, command, None, dict(os.environ), cases, ready_timeout=5)
            self.assertEqual(cases[0]["status"], "failed")
            self.assertEqual(cases[0]["phase"], "readiness")
            self.assertEqual(cases[0]["exit_code"], 7)
            self.assertEqual(cases[0]["final_exit_code"], 7)
            self.assertIn(b"early failure", (root / "disconnect.stdout.log").read_bytes())

    def test_disconnect_success_keeps_its_exact_command_and_completed_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = [sys.executable, "-c", "import pathlib,sys,time;pathlib.Path(sys.argv[1]).write_text('ready');time.sleep(.05)", str(root / "disconnect-ready")]
            cases = []
            MODULE.run_disconnect(root, root, command, None, dict(os.environ), cases, ready_timeout=5)
            self.assertEqual(cases[0]["status"], "completed")
            self.assertEqual(cases[0]["command"], command)
            self.assertEqual(cases[0]["exit_code"], 0)
            self.assertEqual(cases[0]["final_exit_code"], 0)

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
