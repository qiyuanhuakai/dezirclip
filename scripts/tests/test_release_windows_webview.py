"""Pure mock policy transactions and AST checks; never call Windows APIs."""
import ast
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_windows_webview as qa


class RegistryFixture:
    def __init__(self, elevated=True, shared=False):
        self.elevated, self.shared = elevated, shared
        self.keys = {(self.view(view), r'SOFTWARE\Policies\Microsoft') for view in qa.VIEWS}
        self.values = {}
        self.writes = []
        self.token_reads = []
        self.fail_after_write = None
        self.image = 'synthetic-python-only'

    def view(self, value):
        return 0 if self.shared else value

    def token(self, pid):
        self.token_reads.append(pid)
        return {'pid': pid, 'token_is_elevated': int(self.elevated), 'token_elevation_type': 2 if self.elevated else 3,
                'executable': self.image, 'creation_time_100ns': 123}

    def key_exists(self, key, view):
        return (self.view(view), key) in self.keys

    def read(self, key, name, view):
        return copy.deepcopy(self.values.get((self.view(view), key, name), {'exists': False}))

    def create_key(self, key, view):
        index = (self.view(view), key)
        created = index not in self.keys
        self.keys.add(index)
        return created

    def write(self, key, name, view, value):
        index = (self.view(view), key, name)
        self.writes.append((index, copy.deepcopy(value)))
        if value['exists']:
            self.values[index] = copy.deepcopy(value)
        else:
            self.values.pop(index, None)
        if self.fail_after_write == len(self.writes):
            self.fail_after_write = None
            raise OSError('synthetic failure after the actual mock write')

    def empty_key(self, key, view):
        return (not any(v == self.view(view) and (path == key or path.startswith(key + '\\')) for v, path, _ in self.values)
                and not any(v == self.view(view) and path.startswith(key + '\\') for v, path in self.keys)), 123

    def delete_empty_key(self, key, view, observed):
        assert observed == 123 and self.empty_key(key, view)[0]
        self.keys.remove((self.view(view), key))


class ReleaseWebViewPolicy(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='dezirclip-pure-webview-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.profile = self.root / 'private-webview'
        self.state = self.root / 'policy.log'
        self.context = {'synthetic_unit_data_only': True}

    def configure(self, api):
        return qa.configure(api, self.state, self.root, self.profile, 31415, self.context)

    def test_non_elevated_has_no_registry_reads_or_writes(self):
        class NoRegistry(RegistryFixture):
            def key_exists(self, *args):
                raise AssertionError('Registry must not be read in the environment-only lane')
            def read(self, *args):
                raise AssertionError('Registry must not be read in the environment-only lane')
        api = NoRegistry(elevated=False)
        document = self.configure(api)
        original = self.state.read_bytes()
        self.assertEqual(document['mode'], 'unchanged_environment')
        self.assertEqual(api.token_reads, [os.getpid()])
        self.assertFalse(api.writes)
        qa.restore(api, self.state, document)
        qa.restore(api, self.state, document)
        self.assertEqual(original, self.state.read_bytes())

    def test_both_registry_views_restore_exact_original_types(self):
        api = RegistryFixture()
        for view in qa.VIEWS:
            api.keys.update((view, key) for key in qa.PARENTS)
        examples = [(1, 'original'), (2, '%UNCHANGED%'), (3, b'\x00\x01'), (4, 7), (7, ['one', 'two']), (11, 2 ** 40)]
        targets = [(view, key, name) for view in qa.VIEWS for key in qa.KEYS for name in qa.NAMES]
        for target, (kind, data) in zip(targets, examples):
            api.values[target] = {'exists': True, 'type': kind, 'data': qa.packed(data)}
        before = copy.deepcopy(api.values)
        document = self.configure(api)
        original = self.state.read_bytes()
        self.assertEqual({item['view'] for item in document['values']}, {32, 64})
        qa.restore(api, self.state, document)
        self.assertEqual(api.values, before)
        self.assertEqual(original, self.state.read_bytes())

    def test_shared_policy_views_are_not_double_written_and_restore_is_idempotent(self):
        api = RegistryFixture(shared=True)
        before_keys = copy.deepcopy(api.keys)
        document = self.configure(api)
        self.assertEqual(len(api.writes), 4)
        qa.restore(api, self.state, document)
        self.assertEqual(api.values, {})
        self.assertEqual(api.keys, before_keys)
        before = len(api.writes)
        qa.restore(api, self.state, document)
        self.assertEqual(len(api.writes), before)

    def test_partial_write_failure_retains_snapshot_and_can_restore(self):
        api = RegistryFixture(shared=True)
        api.fail_after_write = 2
        before_keys = copy.deepcopy(api.keys)
        with self.assertRaisesRegex(OSError, 'synthetic failure'):
            self.configure(api)
        document, _ = qa.read_json(self.state)
        original = self.state.read_bytes()
        qa.restore(api, self.state, document)
        self.assertEqual(api.values, {})
        self.assertEqual(api.keys, before_keys)
        self.assertEqual(self.state.read_bytes(), original)

    def test_unwritten_separate_view_is_never_restored_as_a_shared_alias(self):
        api = RegistryFixture()
        api.fail_after_write = 1
        with self.assertRaises(OSError):
            self.configure(api)
        document, _ = qa.read_json(self.state)
        item = next(value for value in document['values'] if value['view'] == 32 and value['key'] == qa.KEYS[0] and value['name'] == qa.NAMES[0])
        index = (32, item['key'], item['name'])
        concurrent = copy.deepcopy(item['after'])
        api.values[index] = concurrent
        with self.assertRaisesRegex(ValueError, 'restoration failed'):
            qa.restore(api, self.state, document)
        self.assertEqual(api.values[index], concurrent)

    def test_incomplete_late_journal_still_restores_confirmed_value_writes(self):
        api = RegistryFixture(shared=True)
        document = self.configure(api)
        with Path(str(self.state) + '.configure.log').open('ab') as stream:
            stream.write(b'{"partial":')
        with self.assertRaisesRegex(ValueError, 'restoration failed'):
            qa.restore(api, self.state, document)
        self.assertEqual(api.values, {})

    def test_concurrent_value_and_child_key_are_preserved_and_fail(self):
        api = RegistryFixture(shared=True)
        document = self.configure(api)
        index = (0, qa.KEYS[0], qa.NAMES[0])
        concurrent = {'exists': True, 'type': 1, 'data': qa.packed('separate concurrent fixture change')}
        api.values[index] = concurrent
        api.keys.add((0, qa.KEYS[1] + r'\unrelated-child'))
        with self.assertRaisesRegex(ValueError, 'restoration failed'):
            qa.restore(api, self.state, document)
        self.assertEqual(api.values[index], concurrent)
        self.assertIn((0, qa.KEYS[1] + r'\unrelated-child'), api.keys)

    def test_snapshot_cannot_restore_unrelated_keys_names_or_data(self):
        document = self.configure(RegistryFixture())
        mutations = [lambda value: value['values'][0].update(key=r'SOFTWARE\Unrelated'),
                     lambda value: value['values'][0].update(name='*'),
                     lambda value: value['values'][0]['after'].update(data=qa.packed('another port')),
                     lambda value: value.update(mode='unchanged_environment')]
        for mutate in mutations:
            changed = copy.deepcopy(document); mutate(changed)
            with self.assertRaises(ValueError):
                qa.validate_document(changed, self.root, self.context)

    def test_configure_refuses_to_overwrite_initial_snapshot(self):
        api = RegistryFixture()
        self.configure(api)
        original, writes = self.state.read_bytes(), len(api.writes)
        with self.assertRaises(ValueError):
            self.configure(api)
        self.assertEqual(self.state.read_bytes(), original)
        self.assertEqual(len(api.writes), writes)

    def test_inspect_records_the_child_token_and_preserves_configure_snapshot(self):
        api = RegistryFixture()
        document = self.configure(api)
        original = self.state.read_bytes()
        image = self.root / 'dezirclip.exe'
        api.image = str(image)
        result = qa.inspect(api, self.state, document, 12345, image)
        self.assertEqual(result['actual_process']['pid'], 12345)
        self.assertEqual(result['actual_process']['token_is_elevated'], 1)
        self.assertEqual(self.state.read_bytes(), original)
        self.assertEqual(json.loads(Path(str(self.state) + '.inspect.log').read_text())['status'], 'observed')

    def test_inspect_token_failure_is_retained(self):
        api = RegistryFixture()
        document = self.configure(api)
        def fail(pid):
            raise OSError('synthetic token query failure')
        api.token = fail
        with self.assertRaises(OSError):
            qa.inspect(api, self.state, document, 12345, self.root / 'dezirclip.exe')
        result = json.loads(Path(str(self.state) + '.inspect.log').read_text())
        self.assertEqual(result['status'], 'failed')
        self.assertIn('synthetic token query failure', result['error'])

    def test_actual_windows_api_is_constructed_only_after_real_authority(self):
        source = Path(qa.__file__).read_text(encoding='utf-8')
        tree = ast.parse(source)
        main = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'main')
        text = ast.unparse(main)
        self.assertLess(text.index('context = authority(args.source)'), text.index('api = NativeWindows()'))
        self.assertIn("os.environ.get('GITHUB_ACTIONS') == 'true'", text)
        self.assertIn("os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'", text)
        self.assertIn("sys.platform == 'win32'", text)
        self.assertNotIn('args.source == SOURCE', text)
        self.assertFalse(any(isinstance(node, ast.ImportFrom) and node.module == 'release_recovery'
                             and any(alias.name == 'SOURCE' for alias in node.names) for node in tree.body))
        self.assertIn('GetTokenInformation', source)
        self.assertNotIn('IsInRole', source)
        self.assertFalse(any(isinstance(node, ast.Import) and any(alias.name in ('winreg', 'ctypes') for alias in node.names) for node in tree.body))


if __name__ == '__main__':
    unittest.main()
