from contextlib import closing
from functools import partial
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from native_appimage_contract import (OLD_RELEASES, appimage_scenarios, configure_headless_fixture,
                                     verify_build_set, verify_previous_alpha)
from release_version import release_version

VERSION = release_version(SCRIPTS.parent)


class HeadlessFixtureTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='dezirclip-headless-fixture-')
        self.database = Path(self.directory.name) / 'clipboard.db'

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
        record = configure_headless_fixture(self.database)
        self.assertEqual(record['before'], {'app.silent_start': 'true', 'app.disable_webview_gpu': 'false'})
        self.assertEqual(record['after'], {'app.silent_start': 'false', 'app.disable_webview_gpu': 'true'})
        with closing(sqlite3.connect(self.database)) as connection:
            self.assertEqual(connection.execute('SELECT content FROM clipboard_history').fetchone()[0],
                             'synthetic preserved entry')
            self.assertEqual(connection.execute('SELECT value FROM settings WHERE key=?',
                                               ('app.quick_paste_hotkey',)).fetchone()[0], 'Ctrl+Shift+V')

    def test_missing_old_database_is_rejected_without_creating_a_file(self):
        with self.assertRaisesRegex(RuntimeError, 'old CLI'):
            configure_headless_fixture(self.database)
        self.assertFalse(self.database.exists())

    def test_settings_failure_rolls_back_the_first_write(self):
        with closing(sqlite3.connect(self.database)) as connection, connection:
            connection.execute("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT CHECK(key!='app.disable_webview_gpu' OR value!='true'))")
            connection.executemany('INSERT INTO settings VALUES(?,?)', [
                ('app.silent_start', 'true'), ('app.disable_webview_gpu', 'false')])
        with self.assertRaises(sqlite3.IntegrityError):
            configure_headless_fixture(self.database)
        with closing(sqlite3.connect(self.database)) as connection:
            self.assertEqual(dict(connection.execute('SELECT key,value FROM settings')),
                             {'app.silent_start': 'true', 'app.disable_webview_gpu': 'false'})


class BuildSetTests(unittest.TestCase):
    def setUp(self):
        self.verify = partial(verify_build_set, version=VERSION)
        self.revision = 'a' * 40

    def component(self, role):
        version = VERSION
        names = ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')
        if role == 'service':
            names += ('dzc-service',)
        return {'component': role, 'component_version': version,
                'source_revision': self.revision, 'build_id': version + '@' + self.revision,
                'source_state': 'verified_checkout', 'metadata_version': 1,
                'database_schema_version': 24, 'protocol_version': {'major': 1, 'minor': 0},
                'target': 'x86_64-unknown-linux-gnu',
                'linked_components': [{'component': name, 'version': version}
                                      for name in names]}

    def test_actual_components_must_match_the_frozen_version_set(self):
        result = self.verify(self.component('cli'), self.component('desktop'), self.component('service'), self.revision)
        self.assertTrue(result['verified'])
        self.assertEqual(result['source_revision'], self.revision)
        self.assertEqual(result['components'], ['desktop', 'cli', 'service'])
        self.assertFalse(result['service_started'])
        with self.assertRaisesRegex(RuntimeError, 'frozen'):
            self.verify(self.component('cli'), self.component('desktop'), self.component('service'), 'b' * 40)

    def test_unknown_inconsistent_or_different_component_metadata_is_rejected(self):
        changes = [('component', 'surface'), ('component_version', '0.2.0-alpha.0'),
                   ('build_id', None), ('build_id', 'unrelated-build'),
                   ('source_state', 'modified_checkout'), ('source_state', 'unknown'),
                   ('metadata_version', True), ('metadata_version', 2),
                   ('database_schema_version', 19), ('database_schema_version', 20),
                   ('database_schema_version', 23), ('database_schema_version', True),
                   ('database_schema_version', 22), ('database_schema_version', 25),
                   ('protocol_version', {'major': True, 'minor': 0}),
                   ('protocol_version', {'major': 1, 'minor': 1}),
                   ('target', 'aarch64-unknown-linux-gnu'), ('linked_components', [])]
        for role in ('cli', 'desktop', 'service'):
            for field, value in changes:
                with self.subTest(role=role, field=field, value=value):
                    components = {name: self.component(name) for name in ('cli', 'desktop', 'service')}
                    components[role][field] = value
                    with self.assertRaises(RuntimeError):
                        self.verify(components['cli'], components['desktop'], components['service'], self.revision)
        cli, desktop = self.component('cli'), self.component('desktop')
        desktop['source_revision'] = 'b' * 40
        desktop['build_id'] = desktop['component_version'] + '@' + desktop['source_revision']
        with self.assertRaisesRegex(RuntimeError, 'different build sets'):
            self.verify(cli, desktop, self.component('service'), '')

    def test_inventory_is_independent_of_agreeing_self_reports(self):
        for names in [('dzc-core', 'dzc-platform', 'dzc-protocol'),
                      ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host', 'dzc-host'),
                      ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host', 'dzc-surface')]:
            components = [self.component(role) for role in ('cli', 'desktop', 'service')]
            for info in components:
                info['linked_components'] = [{'component': name, 'version': info['component_version']} for name in names]
            with self.subTest(names=names), self.assertRaisesRegex(RuntimeError, 'linked version set'):
                self.verify(*components, self.revision)
        service = self.component('service')
        service['source_revision'] = 'b' * 40
        service['build_id'] = service['component_version'] + '@' + service['source_revision']
        with self.assertRaisesRegex(RuntimeError, 'different build sets'):
            self.verify(self.component('cli'), self.component('desktop'), service, '')


class PreviousAlphaTests(unittest.TestCase):
    def test_previous_alpha_is_bound_to_the_published_source_schema_and_inventory(self):
        revision = '90ba9960744b6cd57caaaf597f68533a7e41be45'
        version = '0.2.0-alpha.1'
        original = {'component': 'cli', 'component_version': version,
                    'source_revision': revision, 'build_id': version + '@' + revision,
                    'source_state': 'verified_checkout', 'metadata_version': 1,
                    'database_schema_version': 21, 'protocol_version': {'major': 1, 'minor': 0},
                    'target': 'x86_64-unknown-linux-gnu',
                    'linked_components': [{'component': name, 'version': version}
                                          for name in ('dzc-core', 'dzc-platform', 'dzc-protocol')]}
        self.assertTrue(verify_previous_alpha(original)['verified'])
        for field, value in [('component_version', '0.2.0-alpha.2'), ('source_revision', 'a' * 40),
                             ('build_id', None), ('source_state', 'modified_checkout'),
                             ('metadata_version', True), ('database_schema_version', 24),
                             ('database_schema_version', True), ('target', 'aarch64-unknown-linux-gnu'),
                             ('protocol_version', {'major': True, 'minor': 0}),
                             ('linked_components', [])]:
            with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                verify_previous_alpha({**original, field: value})

    def test_published_alpha2_keeps_its_exact_source_schema_and_host_inventory(self):
        version = '0.2.0-alpha.2'
        revision = '037f40cf014cd4bb3dce4a0a0c5cbcaee23affae'
        original = {'component': 'cli', 'component_version': version,
                    'source_revision': revision, 'build_id': version + '@' + revision,
                    'source_state': 'verified_checkout', 'metadata_version': 1,
                    'database_schema_version': 24, 'protocol_version': {'major': 1, 'minor': 0},
                    'target': 'x86_64-unknown-linux-gnu',
                    'linked_components': [{'component': name, 'version': version}
                                          for name in ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')]}
        result = verify_previous_alpha(original, version)
        self.assertEqual(result, {'source_revision': revision, 'database_schema_version': 24, 'verified': True})
        changes = [('component', 'desktop'), ('component_version', VERSION),
                   ('source_revision', 'a' * 40), ('build_id', None),
                   ('source_state', 'modified_checkout'), ('metadata_version', True),
                   ('database_schema_version', 21), ('database_schema_version', True),
                   ('target', 'aarch64-unknown-linux-gnu'),
                   ('protocol_version', {'major': True, 'minor': 0}),
                   ('linked_components', original['linked_components'][:-1]),
                   ('linked_components', original['linked_components'] + original['linked_components'][-1:])]
        for field, value in changes:
            with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                verify_previous_alpha({**original, field: value}, version)
        with self.assertRaisesRegex(RuntimeError, 'Unknown'):
            verify_previous_alpha(original, '0.2.0-alpha.0')

    def test_old_release_hashes_include_the_real_public_alpha2_bytes(self):
        self.assertEqual(OLD_RELEASES, {
            '0.1.20': 'da4abe1af820226ccf0cb8c40c8d7512b5ae3d7de89babcd1acecfd1ece9f0f0',
            '0.2.0-alpha.1': '660d8d73d9b719b5c7b92f471024a715eb61fa471b56cba7364edfa1c2464628',
            '0.2.0-alpha.2': '2f1d91a027b447a2bfe259f29c896b62abda2a24bd5d1fda22bd831e376f81af',
        })

    def test_each_inner_command_receives_the_candidate_version(self):
        scenario = Path('/fixture/scenario')
        commands = appimage_scenarios(scenario, VERSION)
        self.assertEqual([label for label, _, _ in commands], ['old', 'read-only', 'replace', 'new', 'new-read-only'])
        for label, arguments, readonly in commands:
            with self.subTest(label=label):
                self.assertEqual(readonly, label == 'read-only')
                if label in ('old', 'new', 'new-read-only'):
                    self.assertEqual(arguments[-4:], ['/usr/bin/python3', '/fixture-bin/acceptance.py', label, VERSION])
                else:
                    self.assertEqual(arguments[:2], ['/fixture-bin/updater', str(scenario)])


if __name__ == '__main__':
    unittest.main()
