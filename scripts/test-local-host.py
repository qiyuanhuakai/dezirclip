"""Actual optional-service lifecycle in a new synthetic profile; no clipboard or GUI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import shutil
import sys


def verify_installed_profile_move(cli, profile, isolated_env):
    # Linux honors XDG_DATA_HOME for discovery. Use only this fixture's installed profile.
    assert sys.platform.startswith('linux') and Path(isolated_env['XDG_DATA_HOME']) == profile/'data'
    env = dict(isolated_env)
    for key in ('DEZIRCLIP_DB_PATH', 'TIEZ_DB_PATH'):
        env.pop(key, None)
    target = profile/'moved profile'
    target.mkdir()
    config = profile/'data/io.github.qiyuanhuakai.dezirclip'
    redirect = config/'datapath.txt'
    def run(*args, environment=env, success=True):
        result = subprocess.run([cli, *args], env=environment, capture_output=True, text=True, timeout=20)
        assert (result.returncode == 0) == success, (args, result.returncode, result.stderr)
        return result.stdout
    run('add', 'installed-profile-before-relocation')
    run('service', 'move-data', target, environment=isolated_env, success=False)
    for busy_env in (env, {**env, 'DEZIRCLIP_DB_PATH': str(target/'another.db')}):
        run('service', 'start', '--no-capture', '--json', environment=busy_env)
        try:
            run('service', 'move-data', target, success=False)
            assert not redirect.exists() and not (target/'clipboard.db').exists()
        finally:
            run('service', 'stop', environment=busy_env)
    run('service', 'move-data', target)
    assert Path(redirect.read_text()) == target and (config/'clipboard.db').is_file()
    assert 'installed-profile-before-relocation' in run('get', 'latest')
    run('add', 'installed-profile-after-relocation')
    assert 'installed-profile-after-relocation' in run('get', 'latest')
    old_env = {**env, 'DEZIRCLIP_DB_PATH': str(config/'clipboard.db')}
    assert 'installed-profile-before-relocation' in run('get', 'latest', environment=old_env)
    started = json.loads(run('service', 'start', '--no-capture', '--json'))
    try:
        discovered = json.loads(run('service', 'status', '--json',
            environment={**env, 'DEZIRCLIP_DB_PATH': str(target/'clipboard.db')}))
        assert discovered['state']['service_instance_id'] == started['state']['service_instance_id']
        run('service', 'status', '--json', environment=old_env, success=False)
    finally:
        run('service', 'stop')
    print('Verified offline relocation, old profile preservation and discovery of the moved profile by fresh CLI and Service processes')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--desktop-fixture', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    service = cli.with_name('dzc-service' + cli.suffix)
    report = {'kind': 'synthetic-local-host-functional', 'timing_eligible': False, 'status': 'running', 'commands': []}
    report['binaries'] = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in (cli, service)}
    temporary = tempfile.mkdtemp(prefix='dzc-local-host-')
    # Failed profiles remain available for diagnosis. Remove a successful
    # fixture only after both acknowledged service processes have exited.
    if temporary:
        profile = Path(temporary)
        report['profile'] = str(profile)
        for name in ('home', 'config', 'cache', 'data', 'runtime'):
            (profile / name).mkdir(mode=0o700)
        database = profile / 'data/clipboard.db'
        env = {**os.environ, 'HOME': str(profile/'home'), 'USERPROFILE': str(profile/'home'),
               'APPDATA': str(profile/'config'), 'LOCALAPPDATA': str(profile/'cache'),
               'XDG_CONFIG_HOME': str(profile/'config'), 'XDG_DATA_HOME': str(profile/'data'),
               'XDG_CACHE_HOME': str(profile/'cache'), 'XDG_RUNTIME_DIR': str(profile/'runtime'),
               'DEZIRCLIP_DB_PATH': str(database)}

        def run(*command, succeeds=True, json_result=False):
            result = subprocess.run([str(cli), *command], env=env, capture_output=True, text=True, encoding='utf-8', timeout=30)
            # Delegated credentials are deliberately excluded from this log.
            shown = list(command)
            if shown[:2] == ['service', 'revoke']:
                shown[-1] = '<delegated-token>'
            report['commands'].append({'arguments': shown, 'exit_code': result.returncode,
                                       'stderr': result.stderr, 'stdout': '<delegated-token>' if shown[:2] == ['service','grant-surface'] else result.stdout})
            assert (result.returncode == 0) == succeeds, (shown, result.returncode, result.stderr)
            return json.loads(result.stdout) if json_result else result

        active = False
        try:
            run('service', 'status', '--json', succeeds=False)
            assert not database.exists(), 'status must not initialize history'
            active = True
            first = run('service', 'start', '--no-capture', '--json', json_result=True)
            assert first['mode'] == 'service' and first['capture_owner'] is False
            if args.desktop_fixture:
                version = run('build-info', '--json', json_result=True)['component_version']
                for mode in ('attach', 'start'):
                    result = subprocess.run([str(args.desktop_fixture.resolve(strict=True)), '--desktop', version, mode],
                                            env=env, check=True, capture_output=True, text=True, timeout=35)
                    status = json.loads(result.stdout)
                    assert not status['capture_owner'] and status['state']['service_instance_id'] == first['state']['service_instance_id']
                report['independent_desktop_reused_owner'] = True
            again = run('service', 'start', '--no-capture', '--json', json_result=True)
            assert again['state']['service_instance_id'] == first['state']['service_instance_id']
            on = run('privacy','on','--seconds','60','--json',json_result=True)
            assert on['active'] and on['privacy_epoch'] > first['privacy']['privacy_epoch']
            assert on['expires_at_ms'] is not None
            assert run('privacy','status','--json',json_result=True) == on
            off = run('privacy','off','--json',json_result=True)
            assert not off['active'] and off['privacy_epoch'] > on['privacy_epoch']
            grant = run('service','grant-surface','--copy',json_result=True)
            assert len(grant['grant']) == 64 and grant['expires_at_ms'] > int(time.time()*1000)
            run('service','revoke',grant['grant'])
            run('service','stop')
            active = False
            run('service','status','--json',succeeds=False)
            active = True
            second = run('service','start','--no-capture','--json',json_result=True)
            assert second['state']['service_instance_id'] != first['state']['service_instance_id']
            assert second['privacy'] == off, 'privacy state must survive Host replacement'
            assert second['state']['event_sequence'] == 0, 'event sequence is per Host instance'
            run('service','stop')
            active = False
            run('service','status','--json',succeeds=False)
            if sys.platform.startswith('linux'):
                verify_installed_profile_move(cli, profile, env)
            report['status'] = 'passed'
        except Exception as error:
            report['status'] = 'failed'
            report['failure'] = repr(error)
            raise
        finally:
            if active:
                cleanup = subprocess.run([str(cli),'service','stop'],env=env,capture_output=True,text=True,encoding='utf-8',timeout=30)
                report['cleanup'] = {'exit_code': cleanup.returncode, 'stderr': cleanup.stderr}
            if args.report:
                args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        if report['status'] == 'passed':
            shutil.rmtree(profile)
    print('Actual private service start/status/privacy/grant/revoke/restart/stop passed; no native capture started.')


if __name__ == '__main__':
    main()
