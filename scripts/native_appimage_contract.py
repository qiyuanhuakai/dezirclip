"""Shared data and fixture checks for isolated AppImage upgrade acceptance."""
from contextlib import closing
import re
import sqlite3


OLD_RELEASES = {
    '0.1.20': 'da4abe1af820226ccf0cb8c40c8d7512b5ae3d7de89babcd1acecfd1ece9f0f0',
    '0.2.0-alpha.1': '660d8d73d9b719b5c7b92f471024a715eb61fa471b56cba7364edfa1c2464628',
    '0.2.0-alpha.2': '2f1d91a027b447a2bfe259f29c896b62abda2a24bd5d1fda22bd831e376f81af',
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def configure_headless_fixture(database):
    if not database.is_file():
        raise RuntimeError('The old CLI must create the synthetic database first.')
    requested = {'app.silent_start': 'false', 'app.disable_webview_gpu': 'true'}
    with closing(sqlite3.connect(database)) as connection, connection:
        before = {}
        for key in requested:
            row = connection.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
            before[key] = row[0] if row else None
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', requested.items())
        after = dict(connection.execute('SELECT key,value FROM settings WHERE key IN (?,?)', tuple(requested)))
    return {'before': before, 'after': after,
            'scope': 'Existing user settings selected only in a synthetic headless Xvfb profile; product defaults are unchanged.'}


def verify_build_set(cli_info, desktop_info, service_info, expected_revision, version):
    components = [('cli', cli_info), ('desktop', desktop_info), ('service', service_info)]
    for role, info in components:
        names = ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')
        if role == 'service':
            names += ('dzc-service',)
        linked = [{'component': name, 'version': version} for name in names]
        require(isinstance(info, dict), 'Component build metadata must be an object.')
        require(info.get('component') == role and info.get('component_version') == version,
                'Component role or version differs from the candidate package.')
        revision = info.get('source_revision')
        require(isinstance(revision, str) and re.fullmatch('[0-9a-f]{40}', revision)
                and info.get('build_id') == version + '@' + revision,
                'Component build identity is missing or inconsistent.')
        require(info.get('source_state') == 'verified_checkout',
                'Native package acceptance requires a verified, clean source checkout.')
        require(not expected_revision or revision == expected_revision.lower(),
                'Component source differs from the frozen acceptance revision.')
        require(type(info.get('metadata_version')) is int and info['metadata_version'] == 1
                and type(info.get('database_schema_version')) is int and info['database_schema_version'] == 24,
                'Component metadata or database schema version differs.')
        protocol = info.get('protocol_version')
        require(isinstance(protocol, dict) and protocol == {'major': 1, 'minor': 0}
                and all(type(protocol.get(key)) is int for key in ('major', 'minor')),
                'Component protocol version differs.')
        require(info.get('target') == 'x86_64-unknown-linux-gnu' and info.get('linked_components') == linked,
                'Component target or linked version set differs.')
    require(len({info['build_id'] for _, info in components}) == 1,
            'Actual GUI, bundled CLI and service belong to different build sets.')
    return {'build_id': cli_info['build_id'], 'source_revision': cli_info['source_revision'],
            'components': ['desktop', 'cli', 'service'], 'verified': True,
            'service_started': False}


def verify_previous_alpha(info, version='0.2.0-alpha.1'):
    contracts = {
        '0.2.0-alpha.1': ('90ba9960744b6cd57caaaf597f68533a7e41be45', 21,
                          ('dzc-core', 'dzc-platform', 'dzc-protocol')),
        '0.2.0-alpha.2': ('037f40cf014cd4bb3dce4a0a0c5cbcaee23affae', 24,
                          ('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')),
    }
    require(version in contracts, 'Unknown published alpha baseline.')
    revision, schema, names = contracts[version]
    require(isinstance(info, dict) and info.get('component') == 'cli'
            and info.get('component_version') == version,
            'The previous alpha must be the selected published CLI.')
    require(info.get('source_revision') == revision and info.get('build_id') == version + '@' + revision
            and info.get('source_state') == 'verified_checkout',
            'Previous alpha source identity differs from the published release.')
    require(type(info.get('metadata_version')) is int and info['metadata_version'] == 1
            and type(info.get('database_schema_version')) is int and info['database_schema_version'] == schema,
            'Previous alpha metadata or database schema differs.')
    protocol = info.get('protocol_version')
    require(isinstance(protocol, dict) and protocol == {'major': 1, 'minor': 0}
            and all(type(protocol.get(key)) is int for key in ('major', 'minor')),
            'Previous alpha protocol differs.')
    linked = [{'component': name, 'version': version} for name in names]
    require(info.get('target') == 'x86_64-unknown-linux-gnu' and info.get('linked_components') == linked,
            'Previous alpha component set differs.')
    return {'source_revision': revision, 'database_schema_version': schema, 'verified': True}


def appimage_scenarios(scenario, candidate_version):
    inside = ['/usr/bin/python3', '/fixture-bin/acceptance.py']
    desktop = ['/usr/bin/xvfb-run', '-a', '-s', '-screen 0 1280x900x24', '/usr/bin/dbus-run-session', '--']
    return [
        ('old', [*inside, 'old', candidate_version], False),
        ('read-only', ['/fixture-bin/updater', str(scenario), '--read-only'], True),
        ('replace', ['/fixture-bin/updater', str(scenario)], False),
        ('new', [*desktop, *inside, 'new', candidate_version], False),
        ('new-read-only', [*desktop, *inside, 'new-read-only', candidate_version], False),
    ]
