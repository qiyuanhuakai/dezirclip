"""Synthetic runtime and namespace fixtures; no GUI, clipboard, or timing samples."""
from contextlib import redirect_stdout
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("native_capture", Path(__file__).resolve().parents[1] / "native_capture.py")
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


def details(prefix):
    executable = prefix / "bin/python3.13"
    module = prefix / "lib/python3.13/_sqlite3.so"
    return {"python": "3.13.15 fixture", "python_version_info": [3, 13, 15], "sqlite": "3.53.1",
            "sqlite_source_id": "controlled-source-id", "executable": str(executable),
            "prefix": str(prefix), "base_prefix": str(prefix),
            "key_files": {str(path): {"sha256": NATIVE.digest(path), "bytes": path.stat().st_size} for path in (executable, module)}}


class BaselineContractTests(unittest.TestCase):
    def test_alpha1_baseline_requires_its_exact_build_proof_and_id_only_event(self):
        revision = '90ba9960744b6cd57caaaf597f68533a7e41be45'
        contract = NATIVE.baseline_contract({'source_revision': revision}, revision)
        self.assertEqual(contract, {'source_revision': revision, 'event': 'clipboard-captured'})
        for wrong in ('802808b06e6b795064c5065d1d880bae9b193b9a', revision[:-1], '', None):
            with self.subTest(wrong=wrong), self.assertRaises(RuntimeError):
                NATIVE.baseline_contract({'source_revision': wrong}, revision)

    def test_historical_default_keeps_its_original_event_and_rejects_unregistered_baselines(self):
        revision = '802808b06e6b795064c5065d1d880bae9b193b9a'
        self.assertEqual(NATIVE.baseline_contract({'source_revision': revision}, revision)['event'], 'clipboard-updated')
        for selected in ('master', '90ba996', '0' * 40):
            with self.subTest(selected=selected), self.assertRaises(RuntimeError):
                NATIVE.baseline_contract({'source_revision': selected}, selected)


class RuntimeTests(unittest.TestCase):
    def fixture(self):
        directory = tempfile.TemporaryDirectory(prefix="dzc-native-runtime-")
        self.addCleanup(directory.cleanup)
        root = Path(directory.name).resolve()
        prefix = root / "standalone"
        (prefix / "bin").mkdir(parents=True)
        (prefix / "lib/python3.13").mkdir(parents=True)
        (prefix / "bin/python3.13").write_bytes(b"synthetic interpreter, never executed")
        (prefix / "lib/python3.13/_sqlite3.so").write_bytes(b"synthetic SQLite, never loaded")
        return root, prefix

    def test_host_probe_uses_only_the_explicit_prefix_and_clean_python_environment(self):
        root, prefix = self.fixture()
        observed = details(prefix)
        with patch.object(NATIVE.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(observed), "")) as run, \
             patch.dict(os.environ, {"PYTHONPATH": "/host/private/modules", "PYTHONHOME": "/host/home", "LD_LIBRARY_PATH": "/host/libs"}):
            runtime = NATIVE.inspect_python_runtime(prefix, root / "native_capture.py")
        arguments = run.call_args.args[0]
        self.assertEqual(arguments, [str(prefix / "bin/python3.13"), "-s", str(root / "native_capture.py"), "--runtime-probe"])
        environment = run.call_args.kwargs["env"]
        self.assertEqual(environment["HOME"], "/nonexistent")
        self.assertEqual(environment["PYTHONHOME"], str(prefix))
        for key in ("PYTHONNOUSERSITE", "PYTHONDONTWRITEBYTECODE", "PYTHONSAFEPATH"):
            self.assertEqual(environment[key], "1")
        self.assertNotIn("PYTHONPATH", environment)
        self.assertNotIn("LD_LIBRARY_PATH", environment)
        self.assertEqual(runtime["details"], observed)
        NATIVE.validate_runtime(runtime["details"])

    def test_runtime_cannot_resolve_its_key_files_or_interpreter_outside_the_prefix(self):
        root, prefix = self.fixture()
        outsider = root / "host-sqlite.so"
        outsider.write_bytes(b"not a standalone dependency")
        original = details(prefix)
        escaped = copy.deepcopy(original)
        escaped["key_files"][str(outsider)] = {"sha256": NATIVE.digest(outsider), "bytes": outsider.stat().st_size}
        for changed in (escaped, dict(original, executable=str(outsider)), dict(original, base_prefix=str(root))):
            with self.subTest(changed=changed), \
                 patch.object(NATIVE.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(changed), "")), \
                 self.assertRaises(RuntimeError):
                NATIVE.inspect_python_runtime(prefix, root / "native_capture.py")

    def test_default_host_probe_checks_inside_system_python_even_when_caller_is_newer(self):
        root, prefix = self.fixture()
        system = root / "system-python"
        system.write_bytes(b"old system interpreter, never executed")
        observed = dict(details(prefix), executable=str(system), sqlite="3.45.1")
        with patch.object(NATIVE, "SYSTEM_PYTHON", system), patch.object(NATIVE.sys, "executable", str(prefix / "bin/python3.13")), \
             patch.object(NATIVE.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(observed), "")) as run:
            runtime = NATIVE.inspect_python_runtime(None, root / "native_capture.py")
        self.assertEqual(run.call_args.args[0][0], str(system))
        self.assertNotIn("PYTHONHOME", run.call_args.kwargs["env"])
        with self.assertRaisesRegex(RuntimeError, "SQLite >= 3.50.4"):
            NATIVE.validate_runtime(runtime["details"])

    def test_host_home_is_not_accepted_as_a_standalone_prefix(self):
        with patch.object(NATIVE.subprocess, "run") as run, self.assertRaisesRegex(RuntimeError, "standalone prefix"):
            NATIVE.inspect_python_runtime(Path.home(), Path("unused.py"))
        run.assert_not_called()

    def test_builtin_sqlite_origin_is_bound_to_interpreter_libraries_without_inventing_a_module_file(self):
        _, prefix = self.fixture()
        self.assertEqual(NATIVE.module_location(SimpleNamespace(__spec__=SimpleNamespace(origin="built-in"))), "built-in")
        self.assertEqual(NATIVE.module_location(SimpleNamespace(__spec__=SimpleNamespace(origin="frozen"))), "frozen")
        self.assertEqual(NATIVE.module_location(SimpleNamespace(__file__=str(prefix / "lib/python3.13/_sqlite3.so"))), str(prefix / "lib/python3.13/_sqlite3.so"))
        with self.assertRaisesRegex(RuntimeError, "verifiable file"):
            NATIVE.module_location(SimpleNamespace(__spec__=SimpleNamespace(origin=None)))

    def test_failed_probe_retains_stdout_stderr_before_any_cases(self):
        root, prefix = self.fixture()
        evidence = root / "probe-failure"
        with patch.object(NATIVE.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, "partial probe output", "synthetic runtime failed")), \
             self.assertRaisesRegex(RuntimeError, "synthetic runtime failed"):
            NATIVE.inspect_python_runtime(prefix, root / "native_capture.py", evidence)
        self.assertEqual((evidence / "stdout.log").read_text(), "partial probe output")
        self.assertEqual((evidence / "stderr.log").read_text(), "synthetic runtime failed")

    def test_version_gate_and_source_or_file_changes_fail_closed(self):
        _, prefix = self.fixture()
        original = details(prefix)
        for version in ("3.50.4", "3.53.1", "3.54.0"):
            NATIVE.validate_runtime(dict(original, sqlite=version))
        for changed in (dict(original, sqlite="3.50.3"), dict(original, sqlite="3.45.1"),
                        dict(original, sqlite="unknown"), dict(original, python_version_info=[3, 12, 9]),
                        dict(original, sqlite_source_id=""), dict(original, key_files={})):
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                NATIVE.validate_runtime(changed)
        altered = copy.deepcopy(original)
        altered["key_files"][str(prefix / "bin/python3.13")]["sha256"] = "0" * 64
        for changed in (altered, dict(original, sqlite_source_id="different-source")):
            with self.subTest(changed=changed), self.assertRaisesRegex(RuntimeError, "host preflight proof"):
                NATIVE.validate_runtime(changed, original)


class NamespaceTests(unittest.TestCase):
    def test_runtime_is_one_readonly_mount_with_all_existing_namespace_and_home_guards(self):
        with tempfile.TemporaryDirectory(prefix="dzc-native-namespace-") as temporary:
            root = Path(temporary).resolve()
            prefix = str(root / "standalone")
            runtime = {"prefix": prefix, "executable": str(Path(prefix) / "bin/python3.13")}
            plan = {"mode": "isolation", "python_runtime": runtime}
            block = {"directory": "isolation", "name": "isolation"}
            with patch.dict(os.environ, {"HOME": "/host/private-home", "PYTHONPATH": "/host/private-modules"}):
                arguments = NATIVE.sandbox(root, plan, block, {}, None)
            for option in ("--unshare-user", "--unshare-pid", "--unshare-net", "--unshare-ipc", "--unshare-uts",
                           "--die-with-parent", "--new-session", "--clearenv"):
                self.assertIn(option, arguments)
            self.assertEqual(set(NATIVE.NAMESPACES), {"user", "pid", "net", "ipc", "uts", "mnt"})
            binds = [arguments[index + 1:index + 3] for index, value in enumerate(arguments) if value == "--ro-bind"]
            self.assertEqual([pair for pair in binds if prefix in pair], [[prefix, prefix]])
            self.assertEqual([arguments[index + 1:index + 3] for index, value in enumerate(arguments) if value == "--bind"],
                             [[str(root / "isolation"), str(root / "isolation")]])
            self.assertNotIn("/host/private-home", arguments)
            self.assertNotIn("/host/private-modules", arguments)
            environment = {arguments[index + 1]: arguments[index + 2] for index, value in enumerate(arguments) if value == "--setenv"}
            self.assertEqual(environment["HOME"], "/home/fixture")
            self.assertEqual(environment["PYTHONHOME"], prefix)
            self.assertEqual(environment["PYTHONNOUSERSITE"], "1")
            self.assertEqual(environment["PYTHONDONTWRITEBYTECODE"], "1")
            self.assertNotIn("PYTHONPATH", environment)
            self.assertNotIn("LD_LIBRARY_PATH", environment)
            command = arguments[arguments.index("/usr/bin/dbus-run-session") + 2:]
            self.assertEqual(command[:3], [runtime["executable"], "-s", "/fixture-bin/native_capture.py"])

    def test_system_runtime_keeps_no_extra_mount_or_pythonhome(self):
        with tempfile.TemporaryDirectory(prefix="dzc-native-namespace-") as temporary:
            root = Path(temporary).resolve()
            arguments = NATIVE.sandbox(root, {"mode": "isolation", "python_runtime": {"prefix": None, "executable": "/usr/bin/python3"}},
                                       {"directory": "isolation", "name": "isolation"}, {}, None)
            self.assertNotIn("PYTHONHOME", arguments)
            self.assertIn("/usr/bin/python3", arguments)

    def test_inside_runtime_mismatch_is_retained_and_stops_before_database_or_gui(self):
        with tempfile.TemporaryDirectory(prefix="dzc-native-inside-") as temporary:
            root = Path(temporary).resolve()
            (root / "isolation").mkdir()
            expected = {"python": "fixture", "python_version_info": [3, 13, 15], "sqlite": "3.53.1",
                        "sqlite_source_id": "fixture-source", "key_files": {"fixture": {"sha256": "1" * 64}}}
            plan = {"mode": "functional", "python_runtime": {"details": expected},
                    "host_namespaces": {name: "outside" for name in NATIVE.NAMESPACES}}
            block = {"directory": "isolation", "name": "isolation"}
            for actual in (dict(expected, sqlite="3.45.1"), dict(expected, sqlite_source_id="changed")):
                with self.subTest(actual=actual), patch.object(NATIVE, "validate_root"), \
                     patch.object(NATIVE, "namespaces", return_value={name: "inside" for name in NATIVE.NAMESPACES}), \
                     patch.object(NATIVE, "runtime_details", return_value=actual), \
                     patch.object(NATIVE, "seed_database") as seed, patch.object(NATIVE, "Driver") as driver, \
                     patch.object(NATIVE.os, "getloadavg", return_value=(0, 0, 0), create=True), \
                     patch.dict(os.environ, {"HOME": "/home/fixture", "TAURI_WEBVIEW_AUTOMATION": "true", "DISPLAY": ":123",
                                             "DBUS_SESSION_BUS_ADDRESS": "isolated", "WAYLAND_DISPLAY": ""}), \
                     redirect_stdout(io.StringIO()), self.assertRaises(RuntimeError):
                    NATIVE.inside(root, plan, block)
                seed.assert_not_called()
                driver.assert_not_called()
                evidence = json.loads((root / "isolation/block.json").read_text())
                self.assertEqual(evidence["status"], "failed")
                self.assertFalse(evidence["timing_eligible"])
                self.assertEqual(evidence["python_runtime"], actual)
                self.assertEqual(evidence["samples"], [])


if __name__ == "__main__":
    unittest.main()
