"""Standard-library fakes only; no Cargo, Go, product or native execution."""
import copy
import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('linux_release_inputs', Path(__file__).parents[1] / 'build-linux-release-inputs.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def synthetic_elf():
    data = bytearray(64)
    data[:7], data[16:18], data[18:20] = b'\x7fELF\x02\x01\x01', b'\x03\x00', b'\x3e\x00'
    return bytes(data)


class Inputs(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dezirclip-pure-linux-inputs-')
        self.addCleanup(self.temporary.cleanup)
        parent = Path(self.temporary.name).resolve()
        self.root, self.fake_runner = parent / 'repo', parent / 'runner'
        self.root.mkdir(); self.fake_runner.mkdir()
        self.source, self.version, self.calls = 'a' * 40, '0.2.0-alpha.5', []
        self.event = self.fake_runner / 'event.json'
        self.event.write_text(json.dumps({'inputs': {'linux_release_inputs': 'true'}}), encoding='utf-8')
        self.environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
            'RUNNER_ARCH': 'X64', 'GITHUB_REPOSITORY': MODULE.REPOSITORY, 'GITHUB_JOB': 'tauri',
            'GITHUB_EVENT_NAME': 'workflow_dispatch', 'DEZIRCLIP_LINUX_RELEASE_INPUTS': 'true',
            'GITHUB_SHA': self.source, 'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1',
            'GITHUB_WORKSPACE': str(self.root), 'RUNNER_TEMP': str(self.fake_runner), 'GITHUB_EVENT_PATH': str(self.event)}
        self.identity = {'source_revision': self.source, 'source_state': 'verified_checkout', 'target': MODULE.TARGET}
        self.put('package.json', json.dumps({'version': self.version}).encode())
        self.put('Cargo.toml', b'[profile.release]\nlto = true\ncodegen-units = 1\nopt-level = "s"\nstrip = true\npanic = "abort"\n')
        self.put('Cargo.lock', b'version = 4\n')
        self.put('src-tauri/tauri.conf.json', b'{"build":{"frontendDist":"../dist/web"}}')
        self.put('src-tauri/Cargo.toml', b'[package]\nname="synthetic-desktop"\n')
        self.put('dist/web/index.html', b'<html>synthetic existing frontend</html>')
        for name in ['frontends/dzc-surface/go.mod', 'frontends/dzc-surface/go.sum',
                     'scripts/build-surface.mjs', 'scripts/mygo-surface-patch.mjs']:
            self.put(name, b'synthetic source, never executed\n')
        self.helper = self.root / 'artifacts/dzc-surface' / MODULE.TARGET / 'dzc-surface'
        self.helper.parent.mkdir(parents=True)
        self.helper.write_bytes(synthetic_elf())
        self.proof_path = Path(str(self.helper) + '.proof.json')
        expected = {'component_version': self.version, 'source_revision': self.source,
            'source_state': 'verified_checkout', 'build_id': self.version + '@' + self.source,
            'target': MODULE.TARGET, 'toolkit_patch_id': MODULE.PATCH}
        paths = ['frontends/dzc-surface/go.mod', 'frontends/dzc-surface/go.sum', 'scripts/build-surface.mjs',
                 'scripts/mygo-surface-patch.mjs', 'package.json', 'Cargo.toml', 'Cargo.lock']
        self.proof = {'schema_version': 1, 'kind': 'surface-build', 'expected': expected,
            'executable': str(self.helper), 'sha256': MODULE.digest(self.helper.read_bytes()),
            'identity_verification': 'executed_native_build_info', 'toolchain': 'go version go1.27.3 linux/amd64',
            'build_info': {**expected, 'component': 'surface', 'mygo_version': 'v0.6.4',
                           'mygo_source_revision': MODULE.MYGO_SOURCE, 'protocol_version': {'major': 1, 'minor': 0}},
            'toolkit_patch': {'id': MODULE.PATCH, 'strategy': 'isolated_module_copy', 'module': 'github.com/egoist/mygo',
                'version': 'v0.6.4', 'source_revision': MODULE.MYGO_SOURCE, 'changed_files': ['app.go'],
                'module_sum': 'h1:VzCqZKgvXcym5cSYKoC6IkTgcVeGZquPiui/a3oE36o=',
                'module_go_mod_sum': 'h1:dVav53PwF2xEotjf4oA5lTY2Jp2exgzyjLhbSXhbMwE=',
                'upstream_app_sha256': 'af53a7f7c0dbe36d42edc6de34f9f7735569ae53170715264af6618b99e281b9'},
            'inputs': [{'path': name, 'sha256': MODULE.digest((self.root / name).read_bytes())} for name in paths]}
        self.write_proof(self.proof)
        runner = patch.object(MODULE, 'small', side_effect=self.fake_run)
        runner.start()
        self.addCleanup(runner.stop)

    def put(self, name, data):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def write_proof(self, proof):
        self.proof_path.write_text(json.dumps(proof), encoding='utf-8')

    def fake_run(self, command, root):
        self.calls.append(command)
        self.assertEqual(root, self.root)
        if command == ['git', 'rev-parse', 'HEAD']: return (self.source + '\n').encode(), b''
        if command == ['git', 'status', '--porcelain=v1', '--untracked-files=no']: return b'', b''
        if command == ['rustc', '-vV']: return b'rustc synthetic\nhost: x86_64-unknown-linux-gnu\n', b''
        if command == ['cargo', '--version']: return b'cargo synthetic\n', b''
        raise AssertionError('Unexpected command: ' + repr(command))

    def artifact(self):
        return {'reason': 'compiler-artifact', 'manifest_path': str(self.root / 'src-tauri/Cargo.toml'),
            'target': {'name': 'dezirclip', 'kind': ['bin'], 'crate_types': ['bin']},
            'executable': str(self.root / 'target/release/dezirclip'),
            'profile': {'opt_level': 's', 'debug_assertions': False, 'test': False}, 'fresh': False}

    def cargo_json(self, artifact=None, success=True):
        return (json.dumps(artifact or self.artifact()) + '\n' + json.dumps({'reason': 'build-finished', 'success': success}) + '\n').encode()

    def compile(self, root, output):
        self.assertEqual(root, self.root)
        self.put('target/release/dezirclip', synthetic_elf())
        (output / 'cargo-stdout.log').write_bytes(self.cargo_json())
        (output / 'cargo-stderr.log').write_bytes(b'original synthetic cargo stderr\x00\n')
        return {'compiler_artifact': MODULE.compiler_artifact(self.cargo_json(), root), 'exit_code': 0,
                'streams_complete': {'stdout': True, 'stderr': True}, 'first_error': None,
                'combined_log_bytes': len(self.cargo_json()) + len(b'original synthetic cargo stderr\x00\n'),
                'timeout_seconds': MODULE.BUILD_TIMEOUT_SECONDS, 'wait_seconds': MODULE.BUILD_WAIT_SECONDS}

    def supply(self, compile_release=None):
        with patch.object(MODULE, 'ROOT', self.root), patch.object(MODULE.os, 'environ', dict(self.environment)), \
                patch.object(MODULE.sys, 'platform', 'linux'), patch.object(MODULE, 'build', side_effect=compile_release or self.compile):
            return MODULE.produce()

    def test_complete_pair_reuses_surface_and_records_release_metadata_without_acceptance(self):
        original_proof = self.proof_path.read_bytes()
        output = self.supply()
        manifest = json.loads((output / 'inputs.json').read_text(encoding='utf-8'))
        self.assertEqual(manifest['status'], 'complete_input_pair')
        self.assertEqual((output / 'dzc-surface.proof.json').read_bytes(), original_proof)
        self.assertEqual((output / 'dezirclip').read_bytes(), synthetic_elf())
        proof = json.loads((output / 'dezirclip.proof.json').read_text(encoding='utf-8'))
        self.assertEqual(proof['profile'], 'release')
        self.assertEqual(proof['source_revision'], self.source)
        self.assertEqual(proof['identity_verification'], 'cargo_compiler_artifact')
        self.assertFalse(proof['runtime_executed_by_producer'])
        self.assertFalse(proof['namespace_offline_builder_receipt'])
        self.assertFalse(manifest['phase6_pass'])
        self.assertTrue(all(manifest[name] is None for name in ['functional_acceptance', 'idle_acceptance', 'quiet_window', 'performance_acceptance']))
        self.assertEqual((output / 'cargo-stderr.log').read_bytes(), b'original synthetic cargo stderr\x00\n')
        for line in (output / 'SHA256SUMS').read_text(encoding='utf-8').splitlines():
            expected, name = line.split('  ', 1)
            self.assertEqual(MODULE.digest((output / name).read_bytes()), expected)
        self.assertFalse((output / 'failure.json').exists())
        self.assertFalse(any(command[0] in ('pnpm', 'go') for command in self.calls))

    def test_wrong_context_and_release_overrides_reject_before_production(self):
        for key, value in [('GITHUB_ACTIONS', 'false'), ('RUNNER_ARCH', 'ARM64'), ('RUNNER_ENVIRONMENT', 'self-hosted'),
                           ('GITHUB_REPOSITORY', 'other/repo'), ('GITHUB_JOB', 'other'), ('GITHUB_EVENT_NAME', 'push'),
                           ('DEZIRCLIP_LINUX_RELEASE_INPUTS', 'false'), ('GITHUB_SHA', 'bad'), ('GITHUB_RUN_ID', '0'),
                           ('CARGO_PROFILE_RELEASE_OPT_LEVEL', '3'), ('RUSTFLAGS', '-Copt-level=3'),
                           ('CARGO_BUILD_RUSTFLAGS', '-Copt-level=3'),
                           ('CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS', '-Copt-level=3'),
                           ('CARGO_TARGET_DIR', 'other')]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.authorize(self.root, {**self.environment, key: value}, 'linux')
        with self.assertRaises(ValueError): MODULE.authorize(self.root, self.environment, 'win32')

    def test_dispatch_event_false_numeric_and_unknown_source_or_dirty_checkout_reject(self):
        for value in [False, 'false', 1, None]:
            self.event.write_text(json.dumps({'inputs': {'linux_release_inputs': value}}), encoding='utf-8')
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.authorize(self.root, self.environment, 'linux')
        self.event.write_text('{"inputs":{"linux_release_inputs":true}}', encoding='utf-8')
        for mismatch in ('source', 'dirty'):
            def run(command, root):
                if command[1:3] == ['rev-parse', 'HEAD'] and mismatch == 'source': return b'b' * 40, b''
                if command[1] == 'status' and mismatch == 'dirty': return b' M tracked\n', b''
                return self.fake_run(command, root)
            with patch.object(MODULE, 'small', side_effect=run), self.assertRaises(ValueError):
                MODULE.authorize(self.root, self.environment, 'linux')

    def test_surface_rejects_old_identity_unverified_runtime_wrong_toolkit_or_bytes(self):
        changes = [lambda p: p['expected'].update(source_revision='b' * 40),
            lambda p: p['expected'].update(source_state='modified_checkout'),
            lambda p: p['expected'].update(target='aarch64-unknown-linux-gnu'),
            lambda p: p.update(sha256='b' * 64), lambda p: p.update(identity_verification='cross_target_not_executed'),
            lambda p: p.update(toolchain='go version go1.27.3 linux/arm64'),
            lambda p: p['toolkit_patch'].update(id='other'), lambda p: p['build_info'].update(mygo_source_revision='b' * 40),
            lambda p: p['build_info'].update(protocol_version={'major': True, 'minor': 0}), lambda p: p.update(inputs=[])]
        for change in changes:
            proof = copy.deepcopy(self.proof); change(proof); self.write_proof(proof)
            with self.assertRaises(ValueError): MODULE.surface(self.root, self.identity, self.version)
        self.write_proof(self.proof)
        self.helper.write_bytes(b'not an ELF')
        with self.assertRaises(ValueError): MODULE.surface(self.root, self.identity, self.version)

    def test_debug_wrong_manifest_duplicate_and_incomplete_cargo_metadata_reject(self):
        for field, value in [('opt_level', '0'), ('debug_assertions', True), ('test', True)]:
            item = self.artifact(); item['profile'][field] = value
            with self.assertRaises(ValueError): MODULE.compiler_artifact(self.cargo_json(item), self.root)
        item = self.artifact(); item['executable'] = str(self.root / 'target/debug/dezirclip')
        with self.assertRaises(ValueError): MODULE.compiler_artifact(self.cargo_json(item), self.root)
        item = self.artifact(); item['manifest_path'] = str(self.root / 'other/Cargo.toml')
        with self.assertRaises(ValueError): MODULE.compiler_artifact(self.cargo_json(item), self.root)
        for data in [self.cargo_json(success=False), self.cargo_json()[:-1], b'{}\n',
                     json.dumps(self.artifact()).encode() + b'\n' + self.cargo_json()]:
            with self.assertRaises(ValueError): MODULE.compiler_artifact(data, self.root)

    def test_failure_retains_first_error_and_raw_stderr_without_complete_pair(self):
        def failed(root, output):
            (output / 'cargo-stdout.log').write_bytes(b'partial raw metadata\n')
            (output / 'cargo-stderr.log').write_bytes(b'original error\x00\n')
            raise RuntimeError('synthetic original compile failure')
        with self.assertRaisesRegex(RuntimeError, 'original compile failure'): self.supply(failed)
        output = next(self.fake_runner.glob('dezirclip-linux-release-inputs-*'))
        failure = json.loads((output / 'failure.json').read_text(encoding='utf-8'))
        self.assertFalse(failure['complete_input_pair'])
        self.assertEqual(failure['first_error'], 'synthetic original compile failure')
        self.assertEqual((output / 'cargo-stderr.log').read_bytes(), b'original error\x00\n')
        self.assertFalse((output / 'inputs.json').exists())

    def test_existing_output_refused_without_overwriting(self):
        output = self.fake_runner / 'dezirclip-linux-release-inputs-123-1'; output.mkdir()
        (output / 'original').write_bytes(b'retain')
        with self.assertRaises(ValueError): self.supply()
        self.assertEqual([path.name for path in output.iterdir()], ['original'])

    def test_real_producer_entry_has_no_injected_runner_and_rejects_before_candidate_reads_or_spawn(self):
        with patch.object(MODULE.os, 'environ', {}), patch.object(MODULE, 'regular') as read, \
                patch.object(MODULE.subprocess, 'Popen') as spawn, patch.object(MODULE, 'small') as command:
            with self.assertRaises(ValueError): MODULE.produce()
        read.assert_not_called(); spawn.assert_not_called(); command.assert_not_called()
        self.assertEqual(MODULE.produce.__code__.co_argcount, 0)

    def test_incomplete_build_streams_or_changed_existing_inputs_never_complete_pair(self):
        def incomplete(root, output):
            result = self.compile(root, output)
            result['streams_complete']['stderr'] = False
            return result
        with self.assertRaisesRegex(ValueError, 'both streams'): self.supply(incomplete)
        output = next(self.fake_runner.glob('dezirclip-linux-release-inputs-*'))
        self.assertFalse((output / 'inputs.json').exists())
        self.assertTrue((output / 'failure.json').is_file())

    def test_changed_frontend_after_compile_remains_unqualified(self):
        def changed(root, output):
            result = self.compile(root, output)
            self.put('dist/web/index.html', b'changed during build')
            return result
        with self.assertRaisesRegex(ValueError, 'inputs changed'): self.supply(changed)
        output = next(self.fake_runner.glob('dezirclip-linux-release-inputs-*'))
        self.assertFalse((output / 'inputs.json').exists())
        self.assertTrue((output / 'failure.json').is_file())

    def test_pending_readback_or_publish_failure_plus_failure_receipt_error_cannot_leave_final_marker(self):
        original = MODULE.document
        for attempt, failure in enumerate(('readback', 'rename'), 1):
            self.environment['GITHUB_RUN_ATTEMPT'] = str(attempt)
            def failed(path, value):
                if path.name == 'failure.json': raise OSError('synthetic failure receipt error')
                result = original(path, value)
                if path.name == 'inputs.pending.json' and failure == 'readback':
                    raise OSError('synthetic pending readback failure')
                return result
            errors = io.StringIO()
            with self.subTest(failure=failure), patch.object(MODULE, 'document', side_effect=failed), \
                    patch.object(MODULE.os, 'rename', side_effect=OSError('synthetic rename failure')) as publish, \
                    patch.object(MODULE.sys, 'stderr', errors):
                with self.assertRaisesRegex(OSError, failure): self.supply()
            if failure == 'readback': publish.assert_not_called()
            else: publish.assert_called_once()
            output = self.fake_runner / ('dezirclip-linux-release-inputs-123-' + str(attempt))
            self.assertFalse((output / 'inputs.json').exists())
            self.assertTrue((output / 'inputs.pending.json').is_file())
            self.assertFalse((output / 'failure.json').exists())
            self.assertIn('failure receipt error', errors.getvalue())

    def test_atomic_publication_is_last_and_does_not_validate_after_final_marker_exists(self):
        original_rename, original_regular, published = MODULE.os.rename, MODULE.regular, []
        def read(path, maximum):
            if published: raise OSError('Unexpected verification after publication')
            return original_regular(path, maximum)
        def publish(source, destination):
            self.assertEqual(source.name, 'inputs.pending.json')
            self.assertEqual(destination.name, 'inputs.json')
            self.assertFalse(destination.exists())
            self.assertEqual(json.loads(source.read_text(encoding='utf-8'))['status'], 'complete_input_pair')
            original_rename(source, destination)
            published.append(True)
        with patch.object(MODULE, 'regular', side_effect=read), patch.object(MODULE.os, 'rename', side_effect=publish):
            output = self.supply()
        self.assertEqual(published, [True])
        self.assertTrue((output / 'inputs.json').is_file())
        self.assertFalse((output / 'inputs.pending.json').exists())

    def test_failure_receipt_io_error_does_not_replace_the_original_build_failure(self):
        def failed(root, output): raise RuntimeError('synthetic primary build error')
        output = io.StringIO()
        with patch.object(MODULE, 'document', side_effect=OSError('synthetic receipt error')), \
                patch.object(MODULE.sys, 'stderr', output):
            with self.assertRaisesRegex(RuntimeError, 'primary build error'): self.supply(failed)
        self.assertIn('primary build error', output.getvalue())
        self.assertIn('receipt error', output.getvalue())

    def test_build_loop_exit_one_survives_selector_close_failure_and_logs_close_with_only_fakes(self):
        class Stream:
            def __init__(self, fd): self.fd, self.closed = fd, False
            def fileno(self): return self.fd
            def close(self): self.closed = True
        class Process:
            def __init__(self, code):
                self.stdout, self.stderr, self.code, self.result, self.kills = Stream(91), Stream(92), code, None, 0
            def wait(self, timeout): self.result = self.code; return self.result
            def poll(self): return self.result
            def kill(self): self.kills += 1; self.result = -9
        class Selector:
            def __init__(self): self.items = {}
            def register(self, stream, event, name): self.items[stream.fd] = SimpleNamespace(fileobj=stream, data=name)
            def get_map(self): return self.items
            def select(self, timeout): return [(key, 1) for key in list(self.items.values())]
            def unregister(self, stream): del self.items[stream.fd]
            def close(self):
                if code: raise OSError('synthetic selector close failure')
        for code in (0, 1):
            with self.subTest(exit_code=code):
                output = self.fake_runner / ('build-loop-' + str(code)); output.mkdir()
                process, selector = Process(code), Selector()
                original_log, saved_logs = MODULE.BuildLog, []
                def log(root):
                    value = original_log(root); saved_logs.append(value); return value
                data = {91: [self.cargo_json(), b''], 92: [b'original stderr\x00\n', b'']}
                with patch.object(MODULE.subprocess, 'Popen', return_value=process) as spawn, \
                        patch.object(MODULE.selectors, 'DefaultSelector', return_value=selector), \
                        patch.object(MODULE.os, 'read', side_effect=lambda fd, maximum: data[fd].pop(0)), \
                        patch.object(MODULE, 'BuildLog', side_effect=log):
                    if code:
                        with self.assertRaisesRegex(ValueError, 'exit=1'): MODULE.build(self.root, output)
                    else:
                        result = MODULE.build(self.root, output)
                        self.assertEqual(result['streams_complete'], {'stdout': True, 'stderr': True})
                        self.assertEqual(result['exit_code'], 0)
                spawn.assert_called_once_with(MODULE.COMMAND, cwd=self.root,
                    stdout=MODULE.subprocess.PIPE, stderr=MODULE.subprocess.PIPE)
                self.assertEqual((output / 'cargo-stderr.log').read_bytes(), b'original stderr\x00\n')
                self.assertTrue(process.stdout.closed and process.stderr.closed)
                self.assertTrue(all(stream.closed for stream in saved_logs[0].files.values()))
                self.assertEqual(process.kills, 0)

    def test_build_timeout_survives_poll_failure_and_still_closes_both_pipes_and_logs(self):
        class Stream:
            def __init__(self): self.closed = False
            def fileno(self): return 91
            def close(self): self.closed = True
        process = SimpleNamespace(stdout=Stream(), stderr=Stream())
        process.poll = lambda: (_ for _ in ()).throw(OSError('synthetic poll failure'))
        calls = []
        process.kill = lambda: calls.append('kill')
        process.wait = lambda timeout: calls.append(('wait', timeout)) or -9
        selector = SimpleNamespace(register=lambda *args: None, get_map=lambda: {1: SimpleNamespace(fileobj=Stream())},
                                   unregister=lambda stream: None, close=lambda: None)
        output = self.fake_runner / 'timeout'; output.mkdir()
        original_log, saved_logs = MODULE.BuildLog, []
        def log(root):
            value = original_log(root); saved_logs.append(value); return value
        with patch.object(MODULE.subprocess, 'Popen', return_value=process), \
                patch.object(MODULE.selectors, 'DefaultSelector', return_value=selector), \
                patch.object(MODULE.time, 'monotonic', side_effect=[0, MODULE.BUILD_TIMEOUT_SECONDS + 1]), \
                patch.object(MODULE.os, 'read') as read, patch.object(MODULE, 'BuildLog', side_effect=log):
            with self.assertRaisesRegex(ValueError, 'deadline exceeded'): MODULE.build(self.root, output)
        read.assert_not_called()
        self.assertEqual(calls, ['kill', ('wait', MODULE.BUILD_WAIT_SECONDS)])
        self.assertTrue(process.stdout.closed and process.stderr.closed)
        self.assertTrue(all(stream.closed for stream in saved_logs[0].files.values()))

    def test_stderr_registration_failure_closes_both_pipes_and_kill_failure_still_waits(self):
        class Stream:
            def __init__(self): self.closed = False
            def close(self): self.closed = True
        calls = []
        process = SimpleNamespace(stdout=Stream(), stderr=Stream(), poll=lambda: None)
        def kill():
            calls.append('kill')
            raise OSError('synthetic kill failure')
        process.kill, process.wait = kill, lambda timeout: calls.append(('wait', timeout)) or -9
        def register(stream, event, name):
            calls.append(('register', name))
            if name == 'stderr': raise OSError('synthetic stderr registration failure')
        selector = SimpleNamespace(register=register, close=lambda: calls.append('selector-close'))
        output = self.fake_runner / 'register-failure'; output.mkdir()
        original_log, saved_logs = MODULE.BuildLog, []
        def log(root):
            value = original_log(root); saved_logs.append(value); return value
        with patch.object(MODULE.subprocess, 'Popen', return_value=process), \
                patch.object(MODULE.selectors, 'DefaultSelector', return_value=selector), \
                patch.object(MODULE.os, 'read') as read, patch.object(MODULE, 'BuildLog', side_effect=log):
            with self.assertRaisesRegex(ValueError, 'stderr registration failure'): MODULE.build(self.root, output)
        read.assert_not_called()
        self.assertIn(('wait', MODULE.BUILD_WAIT_SECONDS), calls)
        self.assertIn('selector-close', calls)
        self.assertTrue(process.stdout.closed and process.stderr.closed)
        self.assertTrue(all(stream.closed for stream in saved_logs[0].files.values()))

    def test_log_quota_preserves_exact_prefix_and_permanent_first_error(self):
        output = self.fake_runner / 'logs'; output.mkdir()
        with patch.object(MODULE, 'MAX_METADATA', 8):
            logs = MODULE.BuildLog(output)
            logs.receive('stderr', b'raw\x00')
            logs.receive('stdout', b'123456')
            first = logs.first_error
            logs.receive('stderr', b'ignored')
            logs.close()
        self.assertEqual((output / 'cargo-stderr.log').read_bytes(), b'raw\x00')
        self.assertEqual((output / 'cargo-stdout.log').read_bytes(), b'1234')
        self.assertEqual(logs.first_error, first)
        self.assertIn('32 MiB', first)

    def test_workflow_is_default_false_and_supplies_only_after_original_linux_checks(self):
        text = (Path(__file__).parents[2] / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        input_block = text[text.index('      linux_release_inputs:'):text.index('      windows_surface_performance:')]
        self.assertIn('type: boolean', input_block); self.assertIn('default: false', input_block)
        self.assertIn('      - name: Test optional Linux release input contracts without building\n'
                      '        run: python -B -m unittest discover -s scripts/tests -p test_linux_release_inputs.py -v', text)
        start = text.index('      - name: Supply optional Linux x64 release inputs')
        end = text.index('      - name: Record Linux x64 disk after main checks', start)
        block = text[start:end]
        self.assertLess(text.index('      - name: Verify actual private Linux release receipt mounts'), start)
        for line in block.splitlines():
            if line.strip().startswith('if:'):
                for condition in ["github.event_name == 'workflow_dispatch'", 'inputs.linux_release_inputs',
                                  "runner.os == 'Linux'", "runner.arch == 'X64'", "matrix.name == 'linux-x86_64'"]:
                    self.assertIn(condition, line)
        self.assertNotIn('continue-on-error', block)
        self.assertNotIn('pnpm', block)
        self.assertEqual(MODULE.COMMAND, ['cargo', 'build', '--release', '--locked', '--manifest-path',
            'src-tauri/Cargo.toml', '--features', 'tauri/custom-protocol', '--bin', 'dezirclip', '--message-format=json'])


if __name__ == '__main__':
    unittest.main()
