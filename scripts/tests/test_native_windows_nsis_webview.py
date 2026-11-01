"""Pure file/fake-policy adapter checks; no Windows API or registry execution."""
import ast
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_windows_nsis_webview as adapter
from test_release_windows_webview import RegistryFixture


class NsisWebViewPolicy(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dezirclip-pure-nsis-policy-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        if not hasattr(Path, 'is_junction'):
            mock = patch.object(Path, 'is_junction', return_value=False, create=True)
            mock.start()
            self.addCleanup(mock.stop)
        self.source = 'a' * 40
        self.environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted',
            'GITHUB_REPOSITORY': adapter.REPOSITORY, 'GITHUB_SHA': self.source,
            'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_EVENT_NAME': 'pull_request'}
        self.context = adapter.ci_context(self.source, self.environment, self.source)
        self.sequence = 0
        self.session = self.make_session()

    def make_session(self, scenario='OwnedUpgrade', label='initial'):
        self.sequence += 1
        identifier = format(self.sequence, '032x')
        fixture = self.root / ('dezirclip-upgrade-' + identifier)
        policy_root = self.root / ('dezirclip-nsis-webview-' + identifier)
        fixture.mkdir()
        policy_root.mkdir()
        (fixture / 'installed').mkdir()
        (fixture / 'installed/dezirclip.exe').write_bytes(b'Nonexecuted synthetic identity file')
        return adapter.Session(self.root, fixture, policy_root, scenario, label, 9222, self.context)

    def test_ci_authority_binds_checkout_source_run_attempt_and_event(self):
        self.assertEqual(self.context['source_revision'], self.source)
        for name, value in [('GITHUB_ACTIONS', 'false'), ('RUNNER_ENVIRONMENT', 'self-hosted'),
                            ('GITHUB_REPOSITORY', 'other/repository'), ('GITHUB_SHA', 'b' * 40),
                            ('GITHUB_RUN_ID', '0'), ('GITHUB_RUN_ATTEMPT', ''), ('GITHUB_EVENT_NAME', 'release')]:
            changed = dict(self.environment, **{name: value})
            with self.subTest(name=name), self.assertRaises(ValueError):
                adapter.ci_context(self.source, changed, self.source)
        with self.assertRaises(ValueError):
            adapter.ci_context(self.source, self.environment, 'b' * 40)

    def test_exact_sibling_roots_and_gui_labels_only(self):
        for scenario, label in [('LegacyUninstall', 'initial'), ('DeniedGuiUpgrade', 'upgraded'),
                                ('OldToNew', 'initial'), ('OwnedExternalEdit', 'upgraded')]:
            with self.subTest(scenario=scenario), self.assertRaises(ValueError):
                adapter.boundaries(self.root, self.session.fixture_root, self.session.root, scenario, label, 9222)
        for fixture, root, port in [(self.session.fixture_root, self.root, 9222),
                                    (self.root, self.session.root, 9222),
                                    (self.session.fixture_root, self.session.root, 9223)]:
            with self.assertRaises(ValueError):
                adapter.boundaries(self.root, fixture, root, 'OwnedUpgrade', 'initial', port)
        unrelated = self.root / ('dezirclip-nsis-webview-' + 'f' * 32)
        unrelated.mkdir()
        with self.assertRaises(ValueError):
            adapter.boundaries(self.root, self.session.fixture_root, unrelated, 'OwnedUpgrade', 'initial', 9222)

    def test_non_elevated_lane_has_zero_registry_reads_or_writes(self):
        class NoRegistry(RegistryFixture):
            def key_exists(self, *args):
                raise AssertionError('No registry access in the standard-token lane')
            def read(self, *args):
                raise AssertionError('No registry access in the standard-token lane')
        api = NoRegistry(elevated=False)
        document = self.session.configure(api)
        self.assertEqual(document['mode'], 'unchanged_environment')
        self.assertEqual(api.token_reads, [os.getpid()])
        self.session.restore(api)
        self.assertEqual(api.writes, [])

    def test_elevated_lane_reuses_exact_original_transaction(self):
        api = RegistryFixture(shared=False)
        before = copy.deepcopy(api.keys)
        document = self.session.configure(api)
        self.assertEqual(document['mode'], 'elevated_per_app_hklm')
        self.assertEqual(len(document['values']), 8)
        self.assertEqual({item['name'] for item in document['values']}, set(adapter.policy.NAMES))
        self.assertEqual({item['view'] for item in document['values']}, {32, 64})
        self.session.restore(api)
        self.assertEqual(api.values, {})
        self.assertEqual(api.keys, before)

    def test_partial_configure_failure_retains_snapshot_for_exact_restore(self):
        api = RegistryFixture(shared=True)
        before = copy.deepcopy(api.keys)
        api.fail_after_write = 2
        with self.assertRaises(OSError):
            self.session.configure(api)
        self.assertTrue(self.session.state.is_file())
        self.assertTrue(Path(str(self.session.state) + '.configure.log').is_file())
        self.session.restore(api)
        self.assertEqual(api.values, {})
        self.assertEqual(api.keys, before)

    def test_any_old_label_receipt_refuses_configure_before_api_or_state_write(self):
        for suffix in adapter.RECEIPT_SUFFIXES:
            session = self.make_session()
            old = Path(str(session.state) + suffix)
            old.write_bytes(b'Existing unrelated Label evidence')
            api = RegistryFixture()
            with self.subTest(suffix=suffix), self.assertRaises(ValueError):
                session.configure(api)
            self.assertEqual(api.token_reads, [])
            self.assertEqual(api.writes, [])
            self.assertEqual(old.read_bytes(), b'Existing unrelated Label evidence')
            self.assertFalse(session.profile.exists())

    def test_context_label_and_profile_tampering_cannot_restore(self):
        api = RegistryFixture()
        self.session.configure(api)
        original = self.session.state.read_bytes()
        for mutate in (lambda d: d['source_authority'].update(run_attempt='2'),
                       lambda d: d['source_authority'].update(label='upgraded'),
                       lambda d: d.update(profile=str(self.session.fixture_root / 'outside')),
                       lambda d: d.update(port=9223)):
            value = json.loads(original)
            mutate(value)
            self.session.state.write_text(json.dumps(value), encoding='utf-8')
            before = len(api.writes)
            with self.assertRaises(ValueError):
                self.session.restore(api)
            self.assertEqual(len(api.writes), before)
        self.session.state.write_bytes(original)
        self.session.restore(api)

    def test_initial_and_upgraded_have_fresh_states_and_share_browser(self):
        api = RegistryFixture()
        self.session.configure(api)
        self.session.restore(api)
        marker = self.session.profile / 'synthetic-retained-marker'
        marker.write_bytes(b'Keep browser content; never execute it')
        upgraded = adapter.Session(self.root, self.session.fixture_root, self.session.root,
            'OwnedUpgrade', 'upgraded', 9222, self.context)
        self.assertEqual(upgraded.profile, self.session.profile)
        self.assertNotEqual(upgraded.state, self.session.state)
        upgraded.configure(api)
        upgraded.restore(api)
        self.assertEqual(marker.read_bytes(), b'Keep browser content; never execute it')

    def test_original_desktop_token_is_recorded_with_exact_generation(self):
        api = RegistryFixture()
        self.session.configure(api)
        api.image = str(self.session.executable)
        result = self.session.inspect(api, 12345, '123')
        self.assertEqual(result['status'], 'observed')
        self.assertEqual(result['actual_process']['creation_time_100ns'], 123)
        self.assertTrue(Path(str(self.session.state) + '.inspect.log').is_file())
        self.session.restore(api)

    def test_changed_creation_image_or_elevation_fails_inspection(self):
        for change in ('creation', 'image', 'elevation'):
            session = self.make_session()
            api = RegistryFixture()
            session.configure(api)
            api.image = str(session.executable)
            expected = '124' if change == 'creation' else '123'
            if change == 'image':
                api.image = str(session.fixture_root / 'other.exe')
            if change == 'elevation':
                api.elevated = False
            with self.subTest(change=change), self.assertRaises(ValueError):
                session.inspect(api, 12345, expected)
            receipt = json.loads(Path(str(session.state) + '.inspect.log').read_text())
            self.assertEqual(receipt['status'], 'failed')
            session.restore(api)

    def test_token_input_is_typed_and_restricted_to_one_pid(self):
        api = RegistryFixture()
        for pid, created in [(True, '123'), (0, '123'), (12345, None), (12345, '0'),
                             (12345, '01'), (12345, str(2**64))]:
            with self.assertRaises(ValueError):
                adapter.OriginalDesktopToken(api, pid, created, self.session.executable)
        bound = adapter.OriginalDesktopToken(api, 12345, '123', self.session.executable)
        with self.assertRaises(ValueError):
            bound.token(12346)
        self.assertEqual(api.token_reads, [])

    def test_only_four_original_receipts_are_copied_byte_for_byte(self):
        api = RegistryFixture()
        self.session.configure(api)
        api.image = str(self.session.executable)
        self.session.inspect(api, 12345, '123')
        self.session.restore(api)
        (self.session.profile / 'not-an-evidence-member').write_bytes(b'Never copy browser storage')
        result = self.session.preserve()
        self.assertEqual(len(result['copied']), 4)
        self.assertEqual(result['not_created'], [])
        self.assertFalse(result['browser_profile_copied'])
        for suffix in adapter.RECEIPT_SUFFIXES:
            source = Path(str(self.session.state) + suffix)
            self.assertEqual((self.session.fixture_root / source.name).read_bytes(), source.read_bytes())
        self.assertFalse((self.session.fixture_root / 'browser').exists())

    def test_preserve_missing_receipts_is_explicit_and_never_overwrites(self):
        result = self.session.preserve()
        self.assertEqual(result['copied'], [])
        self.assertEqual(len(result['not_created']), 4)
        api = RegistryFixture()
        self.session.configure(api)
        destination = self.session.fixture_root / self.session.state.name
        destination.write_bytes(b'Existing original evidence')
        with self.assertRaises(FileExistsError):
            self.session.preserve()
        self.assertEqual(destination.read_bytes(), b'Existing original evidence')
        self.session.restore(api)

    def test_successful_preservation_requires_every_original_receipt_before_copy(self):
        api = RegistryFixture()
        self.session.configure(api)
        self.session.restore(api)
        with self.assertRaisesRegex(ValueError, 'all four original'):
            self.session.preserve(require_complete=True)
        self.assertFalse((self.session.fixture_root / self.session.state.name).exists())

    def test_required_receipt_disappearance_during_copy_still_fails(self):
        api = RegistryFixture()
        self.session.configure(api)
        api.image = str(self.session.executable)
        self.session.inspect(api, 12345, '123')
        self.session.restore(api)
        missing = Path(str(self.session.state) + '.inspect.log')
        original_exists = Path.exists
        def observed_exists(path):
            return False if path == missing else original_exists(path)
        with patch.object(Path, 'exists', observed_exists), self.assertRaisesRegex(ValueError, 'disappeared during preservation'):
            self.session.preserve(require_complete=True)

    def test_concurrent_policy_change_remains_preserved_and_failed(self):
        api = RegistryFixture(shared=True)
        self.session.configure(api)
        index = (0, adapter.policy.KEYS[0], adapter.policy.NAMES[0])
        concurrent = {'exists': True, 'type': 1, 'data': adapter.policy.packed('synthetic-concurrent-change')}
        api.values[index] = concurrent
        with self.assertRaises(ValueError):
            self.session.restore(api)
        self.assertEqual(api.values[index], concurrent)

    def test_main_guards_precede_native_api_construction(self):
        source = Path(adapter.__file__).read_text(encoding='utf-8')
        tree = ast.parse(source)
        main = next(item for item in tree.body if isinstance(item, ast.FunctionDef) and item.name == 'main')
        text = ast.unparse(main)
        self.assertLess(text.index("sys.platform == 'win32'"), text.index('policy.NativeWindows()'))
        self.assertLess(text.index('ci_context('), text.index('policy.NativeWindows()'))
        self.assertLess(text.index('Session('), text.index('policy.NativeWindows()'))
        self.assertNotIn('policy.authority(', text)
        self.assertNotIn('WinDLL(', source)
        self.assertNotIn('winreg', source)


if __name__ == '__main__':
    unittest.main()
