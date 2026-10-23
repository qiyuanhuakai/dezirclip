"""Synthetic runtime and namespace fixtures; no GUI, clipboard, or timing samples."""
from contextlib import redirect_stdout
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
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


class CaptureEventContractTests(unittest.TestCase):
    revision = '90ba9960744b6cd57caaaf597f68533a7e41be45'
    host = '55da2846-8e07-48e5-a36f-7a6e9b7e0a94'

    def metadata(self, host=False):
        names = ['dzc-core', 'dzc-platform', 'dzc-protocol'] + (['dzc-host'] if host else [])
        return {'source_revision': self.revision, 'linked_components': [{'component': name} for name in names]}

    def test_actual_build_inventory_selects_strict_id_or_id_and_original_host(self):
        legacy = NATIVE.capture_event_contract('clipboard-updated')
        baseline = NATIVE.capture_event_contract('clipboard-captured', self.metadata(), self.revision)
        hosted = NATIVE.capture_event_contract('clipboard-captured', self.metadata(True), self.revision, {'state': {'service_instance_id': self.host}})
        self.assertIsNone(legacy['keys'])
        self.assertEqual(baseline['keys'], ['id'])
        self.assertIsNone(baseline['host_instance_id'])
        self.assertEqual(hosted['keys'], ['host_instance_id', 'id'])
        self.assertEqual(hosted['host_instance_id'], self.host)

    def test_missing_or_changed_build_and_host_identities_fail_before_listening(self):
        for changed in (None, {}, self.metadata() | {'source_revision': '0' * 40},
                        self.metadata() | {'linked_components': []},
                        self.metadata() | {'linked_components': [{'component': 'dzc-core'}] * 3},
                        self.metadata() | {'linked_components': [{'component': 'unknown'}]},
                        self.metadata() | {'linked_components': None}):
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                NATIVE.capture_event_contract('clipboard-captured', changed, self.revision)
        for status in (None, {}, {'state': None}, {'state': {'service_instance_id': ''}}, {'state': {'service_instance_id': 'other'}}):
            with self.subTest(status=status), self.assertRaises(RuntimeError):
                NATIVE.capture_event_contract('clipboard-captured', self.metadata(True), self.revision, status)

    def test_final_listener_snapshot_rejects_a_late_violation_after_valid_samples(self):
        events = [{'id': 7, 'keys': ['host_instance_id', 'id'], 'host_instance_id': self.host}]
        self.assertEqual(NATIVE.validated_listener_events({'events': events, 'error': None}), events)
        for snapshot in (None, {}, {'events': None}, {'events': events, 'error': 'Capture event came from a different Host instance'},
                         {'events': events, 'error': 'Capture event violated its metadata contract'}):
            with self.subTest(snapshot=snapshot), self.assertRaises(RuntimeError):
                NATIVE.validated_listener_events(snapshot)

    def test_real_listener_rejects_extra_payload_or_other_host_without_arming_a_hit(self):
        node = shutil.which('node')
        if not node:
            self.skipTest('Node runtime is required to execute the measurement listener')
        baseline = NATIVE.capture_event_contract('clipboard-captured', self.metadata(), self.revision)
        hosted = NATIVE.capture_event_contract('clipboard-captured', self.metadata(True), self.revision, {'state': {'service_instance_id': self.host}})
        scenarios = [
            {'contract': baseline, 'payload': {'id': 7}, 'valid': True},
            {'contract': hosted, 'payload': {'id': 7, 'host_instance_id': self.host}, 'valid': True},
            {'contract': baseline, 'payload': {'id': 7, 'host_instance_id': self.host}, 'valid': False},
            {'contract': baseline, 'payload': {'id': 7, 'content': 'synthetic body'}, 'valid': False},
            {'contract': hosted, 'payload': {'id': 7}, 'valid': False},
            {'contract': hosted, 'payload': {'id': 7, 'host_instance_id': 'different-host'}, 'valid': False},
            {'contract': hosted, 'payload': {'id': 7, 'host_instance_id': self.host, 'content': 'synthetic body'}, 'valid': False},
            *({'contract': hosted, 'payload': {'id': value, 'host_instance_id': self.host}, 'valid': False} for value in (0, -1, '7', None, 1.5, 9007199254740992)),
        ]
        script = '''
const fs=require('node:fs'), assert=require('node:assert/strict');
(async()=>{
 const {listener,scenarios}=JSON.parse(fs.readFileSync(0,'utf8'));
 for(const scenario of scenarios){
  let callback;
  global.window={__TAURI_INTERNALS__:{transformCallback(fn){callback=fn;return 1},invoke(name,args){assert.equal(name,'plugin:event|listen');assert.equal(args.event,scenario.contract.event);return Promise.resolve(1)}}};
  await new Promise(done=>new Function(listener)(scenario.contract,done));
  window.__nativeCapturePerf.arm={expectedId:7,start:performance.now()-10,hit:null};
  callback({payload:scenario.payload});
  const state=window.__nativeCapturePerf;
  assert.equal(Boolean(state.arm.hit),scenario.valid);
  assert.equal(Boolean(state.error),!scenario.valid);
  assert.equal(state.events.length,scenario.valid?1:0);
  if(scenario.valid) assert(state.arm.hit.elapsed_ms>0);
 }
})().catch(error=>{console.error(error);process.exitCode=1});
'''
        result = subprocess.run([node, '-e', script], input=json.dumps({'listener': NATIVE.CAPTURE_LISTENER, 'scenarios': scenarios}), text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)


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
