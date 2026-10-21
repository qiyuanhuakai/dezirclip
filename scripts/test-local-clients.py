"""Real CLI/MCP clients against an explicit no-capture service and synthetic DB."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    service = cli.with_name('dzc-service' + cli.suffix)
    profile = Path(tempfile.mkdtemp(prefix='dzc-local-clients-'))
    for name in ('home', 'config', 'cache', 'data', 'runtime'):
        (profile / name).mkdir(mode=0o700)
    env = {**os.environ, 'HOME': str(profile/'home'), 'USERPROFILE': str(profile/'home'),
           'APPDATA': str(profile/'config'), 'LOCALAPPDATA': str(profile/'cache'),
           'XDG_CONFIG_HOME': str(profile/'config'), 'XDG_DATA_HOME': str(profile/'data'),
           'XDG_CACHE_HOME': str(profile/'cache'), 'XDG_RUNTIME_DIR': str(profile/'runtime'),
           'DEZIRCLIP_DB_PATH': str(profile/'data/clipboard.db'),
           'DISPLAY': '', 'WAYLAND_DISPLAY': '', 'DEZIRCLIP_HOST_GRANT': ''}
    report = {'kind': 'synthetic-cli-mcp-host-functional', 'timing_eligible': False,
              'status': 'running', 'profile': str(profile), 'commands': [], 'mcp': [],
              'binaries': {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                           for path in (cli, service)}}

    def run(*arguments, success=True):
        result = subprocess.run([str(cli), *arguments], env=env, capture_output=True,
                                text=True, encoding='utf-8', timeout=30)
        hidden = arguments[:2] in (('service', 'grant-mcp'), ('service', 'revoke'))
        report['commands'].append({'arguments': list(arguments[:-1])+['<grant>']
                                  if arguments[:2] == ('service', 'revoke') else arguments,
                                  'exit_code': result.returncode,
                                  'stdout': '<grant>' if hidden else result.stdout,
                                  'stderr': result.stderr})
        assert (result.returncode == 0) == success, (arguments[:2], result.stderr)
        return result.stdout

    def mcp(name, arguments, grant=None):
        messages = [
            {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2025-11-25', 'capabilities': {},
                'clientInfo': {'name': 'synthetic-host-fixture', 'version': '1'}}},
            {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
            {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call',
             'params': {'name': name, 'arguments': arguments}}]
        result = subprocess.run([str(cli), 'mcp', '--host'],
                                input=''.join(json.dumps(item)+'\n' for item in messages),
                                env={**env, 'DEZIRCLIP_HOST_GRANT': grant or ''},
                                capture_output=True, text=True, encoding='utf-8', timeout=30)
        report['mcp'].append({'tool': name, 'arguments': arguments, 'grant_present': bool(grant),
                              'exit_code': result.returncode, 'stdout': result.stdout,
                              'stderr': result.stderr})
        assert result.returncode == 0, result.stderr
        responses = [json.loads(line) for line in result.stdout.splitlines()]
        response = next(item for item in responses if item.get('id') == 2)
        assert 'error' not in response, response
        return response['result']

    active = False
    try:
        run('add', 'synthetic ordinary note')
        ordinary = json.loads(run('list', '--json'))[0]['id']
        active = True
        started = json.loads(run('service', 'start', '--no-capture', '--json'))
        assert started['capture_owner'] is False
        listed = mcp('clipboard_list', {'limit': 20})
        assert not listed.get('isError', False), listed
        entries = json.loads(listed['content'][0]['text'])['entries']
        assert [item['id'] for item in entries] == [ordinary]
        assert 'content' not in entries[0]
        assert mcp('clipboard_get', {'id': ordinary})['isError'] is True
        grant = json.loads(run('service', 'grant-mcp'))['grant']
        got = mcp('clipboard_get', {'id': ordinary}, grant)
        assert not got.get('isError', False), got
        assert json.loads(got['content'][0]['text'])['content'] == 'synthetic ordinary note'
        run('service', 'revoke', grant)
        assert mcp('clipboard_get', {'id': ordinary}, grant)['isError'] is True
        on = json.loads(run('privacy', 'on', '--json'))
        run('add', 'synthetic refused note', success=False)
        assert len(json.loads(run('list', '--json'))) == 1
        run('add', 'synthetic confirmed note', '--confirm-persistence')
        run('edit', str(ordinary), 'synthetic refused edit', success=False)
        run('edit', str(ordinary), 'synthetic confirmed edit', '--confirm-persistence')
        assert json.loads(run('privacy', 'status', '--json')) == on
        run('privacy', 'off', '--json')
        assert len(json.loads(run('list', '--json'))) == 2
        run('service', 'stop')
        active = False
        unavailable = mcp('clipboard_list', {'limit': 20})
        assert unavailable['isError'] is True, 'Host MCP must not fall back to direct DB'
        report['status'] = 'passed'
    except Exception as error:
        report['failure'] = repr(error)
        report['status'] = 'failed'
        raise
    finally:
        if active:
            try:
                run('service', 'stop')
            except Exception as error:
                report['cleanup_failure'] = repr(error)
        args.report.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print('Actual synthetic CLI/MCP grant, confirmation, revocation and no-fallback checks passed.')


if __name__ == '__main__':
    main()
