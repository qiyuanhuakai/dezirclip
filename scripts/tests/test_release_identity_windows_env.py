"""Pure private-path contracts; no Windows KnownFolder API or product executes."""
import ast
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_identity_packages as packages
import release_identity_windows_paths as paths


class WindowsPrivatePathTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dzc-windows-paths-pure-')
        self.root = Path(self.temporary.name).resolve()
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.temporary.cleanup)
        self.logs = self.root / 'asset-1'
        self.environment = {'SYSTEMROOT': 'synthetic-system-root', 'PATH': 'synthetic-path',
                            'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
                            'GITHUB_REPOSITORY': 'qiyuanhuakai/dezirclip', 'RUNNER_TEMP': str(self.root),
                            'GITHUB_SHA': '1' * 40, 'GITHUB_RUN_ID': '12345', 'GITHUB_RUN_ATTEMPT': '2',
                            'DZC_RELEASE_RECOVERY': '1', 'DZC_RELEASE_TOOLING_SOURCE': '1' * 40}

    def build_environment(self, values=None):
        with patch.dict(packages.os.environ, values or self.environment, clear=True):
            return packages.windows_environment(self.logs)

    def test_profile_layout_aligns_appdata_local_and_database_with_private_home(self):
        environment = self.build_environment()
        home = self.logs / 'profile/home'
        expected = paths.profile_layout(self.logs / 'profile')
        self.assertEqual(Path(environment['USERPROFILE']), home)
        self.assertEqual(Path(environment['HOME']), home)
        self.assertEqual(Path(environment['APPDATA']), home / 'AppData/Roaming')
        self.assertEqual(Path(environment['LOCALAPPDATA']), home / 'AppData/Local')
        self.assertEqual(Path(environment['TEMP']), home / 'AppData/Local/Temp')
        self.assertEqual(environment['TEMP'], environment['TMP'])
        self.assertEqual(Path(environment['DEZIRCLIP_DB_PATH']).parent, expected['APPDATA'] / 'io.github.qiyuanhuakai.dezirclip')
        for value in expected.values():
            self.assertTrue(value.is_dir())
            self.assertTrue(value.resolve().is_relative_to(self.logs / 'profile'))

    def test_only_actual_non_sensitive_runner_context_is_forwarded(self):
        inherited = {**self.environment, 'GH_TOKEN': 'synthetic-token', 'GITHUB_TOKEN': 'synthetic-token',
                     'ACTIONS_RUNTIME_TOKEN': 'synthetic-token', 'TAURI_SIGNING_PRIVATE_KEY': 'synthetic-private',
                     'GITHUB_EVENT_NAME': 'workflow_dispatch',
                     'GITHUB_WORKFLOW_REF': 'qiyuanhuakai/dezirclip/.github/workflows/release.yml@refs/heads/master'}
        environment = self.build_environment(inherited)
        for name in ['GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_EVENT_NAME', 'GITHUB_WORKFLOW_REF',
                     'DZC_RELEASE_RECOVERY', 'DZC_RELEASE_TOOLING_SOURCE']:
            self.assertEqual(environment[name], inherited[name])
        for name in ['GH_TOKEN', 'GITHUB_TOKEN', 'ACTIONS_RUNTIME_TOKEN', 'TAURI_SIGNING_PRIVATE_KEY']:
            self.assertNotIn(name, environment)
        environment = self.build_environment({'SYSTEMROOT': 'synthetic', 'GITHUB_ACTIONS': 'false', 'RUNNER_ENVIRONMENT': 'local'})
        self.assertEqual(environment['GITHUB_ACTIONS'], 'false')
        self.assertEqual(environment['RUNNER_ENVIRONMENT'], 'local')
        self.assertNotIn('GITHUB_REPOSITORY', environment)

    def test_synthetic_runtime_results_require_real_success_existing_owned_exact_path(self):
        self.build_environment()
        profile = self.logs / 'profile'
        expected = paths.profile_layout(profile)['APPDATA']
        result = paths.assess_folder('roaming', {'hresult': 0, 'runtime_path': str(expected)}, expected, profile)
        self.assertEqual(result['status'], 'passed')
        self.assertEqual((result['exists'], result['owned'], result['matches_expected']), (True, True, True))
        for observation in [{'hresult': -2147024894, 'hresult_hex': '0x80070002', 'runtime_path': None},
                            {'hresult': 0, 'runtime_path': str(self.root)},
                            {'hresult': 0, 'runtime_path': str(expected / 'missing')}]:
            result = paths.assess_folder('roaming', observation, expected, profile)
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['runtime_path'], observation['runtime_path'])
        self.assertFalse((expected / 'missing').exists())

    def test_failed_path_preflight_blocks_every_native_role_call(self):
        with patch.dict(packages.os.environ, self.environment, clear=True), patch.object(packages, 'run', side_effect=ValueError('synthetic preflight rejection')) as run:
            with self.assertRaisesRegex(ValueError, 'synthetic preflight rejection'):
                packages.windows_info({}, self.logs, 'windows-x86_64', '0' * 40)
        self.assertEqual(run.call_count, 1)
        command = run.call_args.args[0]
        self.assertEqual(Path(command[1]).name, 'release_identity_windows_paths.py')
        self.assertEqual(Path(command[command.index('--output') + 1]), self.root / 'windows-known-folders-asset-1.json')
        self.assertEqual(run.call_args.args[2], 'known-folders')

    def test_failed_standalone_preflight_prevents_the_first_cli_execution(self):
        assets = self.root / 'download'
        output = self.root / 'receipt'
        assets.mkdir()
        output.mkdir()
        (assets / 'synthetic.zip').write_bytes(b'pure archive placeholder, never extracted or executed')
        arguments = SimpleNamespace(target='windows-x86_64', source='0' * 40, assets=assets, out=output)
        report = {'standalone': [], 'containers': []}
        with patch.dict(packages.os.environ, self.environment, clear=True), \
             patch.object(packages, 'expected_assets', return_value=('synthetic.zip', [])), \
             patch.object(packages, 'standalone'), patch.object(packages, 'inspect_machine'), \
             patch.object(packages, 'sha', return_value='f' * 64), \
             patch.object(packages, 'run', side_effect=ValueError('synthetic standalone path refusal')) as run:
            with self.assertRaisesRegex(ValueError, 'synthetic standalone path refusal'):
                packages.collect(arguments, report)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(Path(run.call_args.args[0][1]).name, 'release_identity_windows_paths.py')
        self.assertEqual(report, {'standalone': [], 'containers': []})

    def test_standalone_and_container_use_distinct_fresh_root_diagnostics(self):
        with patch.dict(packages.os.environ, self.environment, clear=True), patch.object(packages, 'run', return_value='') as run:
            environments = [packages.windows_preflight(self.root / name, '0' * 40) for name in ['asset-0', 'asset-1']]
        outputs = [Path(call.args[0][call.args[0].index('--output') + 1]) for call in run.call_args_list]
        self.assertEqual(outputs, [self.root / 'windows-known-folders-asset-0.json', self.root / 'windows-known-folders-asset-1.json'])
        self.assertNotEqual(environments[0]['USERPROFILE'], environments[1]['USERPROFILE'])

    def test_pure_hosted_prefix_and_owned_path_checks_refuse_other_contexts(self):
        paths.assert_disposable_windows('win32', self.environment)
        for platform, values in [('linux', self.environment), ('win32', {**self.environment, 'GITHUB_ACTIONS': 'false'}),
                                 ('win32', {**self.environment, 'RUNNER_ENVIRONMENT': 'self-hosted'}),
                                 ('win32', {**self.environment, 'GITHUB_REPOSITORY': 'other/repository'})]:
            with self.assertRaises(ValueError):
                paths.assert_disposable_windows(platform, values)
        self.assertEqual(paths.owned(self.logs, self.environment), self.logs)
        with self.assertRaises(ValueError):
            paths.owned(self.root.parent / 'outside-synthetic-fixture', self.environment)

    def test_ctypes_query_is_only_statically_inspected_and_uses_flags_zero(self):
        source = Path(paths.__file__).read_text(encoding='utf-8')
        tree = ast.parse(source)
        query = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'query_known_folder')
        self.assertIsInstance(query.body[0], ast.Expr)
        self.assertEqual(query.body[0].value.func.id, 'assert_disposable_windows')
        calls = [node for node in ast.walk(query) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'get_folder']
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0].args[1].value, 0)
        self.assertNotIn('SHSetKnownFolderPath', source)
        self.assertNotIn('winreg', source)


if __name__ == '__main__':
    unittest.main()
