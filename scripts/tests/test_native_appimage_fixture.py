import ast
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest


def load_fixture_function():
    source = Path(__file__).resolve().parents[1] / 'test-native-appimage.py'
    tree = ast.parse(source.read_text(encoding='utf-8'))
    function = next(node for node in tree.body
                    if isinstance(node, ast.FunctionDef) and node.name == 'configure_headless_fixture')
    namespace = {'sqlite3': sqlite3, 'closing': closing}
    exec(compile(ast.Module(body=[function], type_ignores=[]), str(source), 'exec'), namespace)
    return namespace['configure_headless_fixture']


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


if __name__ == '__main__':
    unittest.main()
