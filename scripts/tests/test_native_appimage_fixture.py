import ast
from contextlib import closing
from pathlib import Path
import re
import sqlite3
import tempfile
import unittest


def load_fixture_function(name='configure_headless_fixture'):
    source = Path(__file__).resolve().parents[1] / 'test-native-appimage.py'
    tree = ast.parse(source.read_text(encoding='utf-8'))
    functions = [node for node in tree.body
                 if isinstance(node, ast.FunctionDef) and node.name in (name, 'require')]
    namespace = {'sqlite3': sqlite3, 'closing': closing, 're': re}
    exec(compile(ast.Module(body=functions, type_ignores=[]), str(source), 'exec'), namespace)
    return namespace[name]


class HeadlessFixtureTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='dezirclip-headless-fixture-')
        self.database = Path(self.directory.name) / 'clipboard.db'
        self.configure = load_fixture_function()

    def tearDown(self):
        self.directory.cleanup()

    def test_seed_records_previous_values_and_preserves_other_settings_and_history(self):
        with closing(sqlite3.connect(self.database)) as connection, connection:
            connection.executescript('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT);'
                                     'CREATE TABLE clipboard_history(content TEXT);')
            connection.executemany('INSERT INTO settings VALUES(?,?)', [
                ('app.silent_start', 'true'), ('app.disable_webview_gpu', 'false'),
                ('app.quick_paste_hotkey', 'Ctrl+Shift+V')])
            connection.execute("INSERT INTO clipboard_history VALUES('synthetic preserved entry')")
        record = self.configure(self.database)
        self.assertEqual(record['before'], {'app.silent_start': 'true', 'app.disable_webview_gpu': 'false'})
        self.assertEqual(record['after'], {'app.silent_start': 'false', 'app.disable_webview_gpu': 'true'})
        with closing(sqlite3.connect(self.database)) as connection:
            self.assertEqual(connection.execute('SELECT content FROM clipboard_history').fetchone()[0],
                             'synthetic preserved entry')
            self.assertEqual(connection.execute('SELECT value FROM settings WHERE key=?',
                                               ('app.quick_paste_hotkey',)).fetchone()[0], 'Ctrl+Shift+V')

    def test_missing_old_database_is_rejected_without_creating_a_file(self):
        with self.assertRaisesRegex(RuntimeError, 'old CLI'):
            self.configure(self.database)
        self.assertFalse(self.database.exists())

    def test_settings_failure_rolls_back_the_first_write(self):
        with closing(sqlite3.connect(self.database)) as connection, connection:
            connection.execute("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT CHECK(key!='app.disable_webview_gpu' OR value!='true'))")
            connection.executemany('INSERT INTO settings VALUES(?,?)', [
                ('app.silent_start', 'true'), ('app.disable_webview_gpu', 'false')])
        with self.assertRaises(sqlite3.IntegrityError):
            self.configure(self.database)
        with closing(sqlite3.connect(self.database)) as connection:
            self.assertEqual(dict(connection.execute('SELECT key,value FROM settings')),
                             {'app.silent_start': 'true', 'app.disable_webview_gpu': 'false'})


class BuildSetTests(unittest.TestCase):
    def setUp(self):
        self.verify = load_fixture_function('verify_build_set')
        self.revision = 'a' * 40

    def component(self, role):
        version = '0.2.0-alpha.1'
        return {'component': role, 'component_version': version,
                'source_revision': self.revision, 'build_id': version + '@' + self.revision,
                'source_state': 'verified_checkout', 'metadata_version': 1,
                'database_schema_version': 19, 'protocol_version': {'major': 1, 'minor': 0},
                'target': 'x86_64-unknown-linux-gnu',
                'linked_components': [{'component': name, 'version': version}
                                      for name in ('dzc-core', 'dzc-platform', 'dzc-protocol')]}

    def test_actual_components_must_match_the_frozen_version_set(self):
        result = self.verify(self.component('cli'), self.component('desktop'), self.revision)
        self.assertTrue(result['verified'])
        self.assertEqual(result['source_revision'], self.revision)
        with self.assertRaisesRegex(RuntimeError, 'frozen'):
            self.verify(self.component('cli'), self.component('desktop'), 'b' * 40)

    def test_unknown_inconsistent_or_different_component_metadata_is_rejected(self):
        changes = [('component', 'surface'), ('component_version', '0.2.0-alpha.0'),
                   ('build_id', None), ('build_id', 'unrelated-build'),
                   ('source_state', 'modified_checkout'), ('source_state', 'unknown'),
                   ('metadata_version', True), ('metadata_version', 2),
                   ('database_schema_version', 20), ('protocol_version', {'major': True, 'minor': 0}),
                   ('protocol_version', {'major': 1, 'minor': 1}),
                   ('target', 'aarch64-unknown-linux-gnu'), ('linked_components', [])]
        for role in ('cli', 'desktop'):
            for field, value in changes:
                with self.subTest(role=role, field=field, value=value):
                    cli, desktop = self.component('cli'), self.component('desktop')
                    (cli if role == 'cli' else desktop)[field] = value
                    with self.assertRaises(RuntimeError):
                        self.verify(cli, desktop, self.revision)
        cli, desktop = self.component('cli'), self.component('desktop')
        desktop['source_revision'] = 'b' * 40
        desktop['build_id'] = desktop['component_version'] + '@' + desktop['source_revision']
        with self.assertRaisesRegex(RuntimeError, 'different build sets'):
            self.verify(cli, desktop, '')


if __name__ == '__main__':
    unittest.main()
