"""Run an installed Windows candidate against a verified published predecessor."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import uuid

from release_identities import ROOT, json_output, native_target, require, run, safe_member, sha
from release_identity_packages import desktop_package_files, unpack, windows_environment
from release_identity_windows_paths import assert_disposable_windows, owned


TARGET = 'windows-x86_64'
UNINSTALL = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip'
WINDOW_CHECK = r"""
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$rootPid = [int]$env:DZC_TEST_DESKTOP_PID
$name = [IO.Path]::GetFileNameWithoutExtension($env:DZC_TEST_DESKTOP)
$caption = [Windows.Automation.PropertyCondition]::new(
    [Windows.Automation.AutomationElement]::NameProperty, 'Settings')
$deadline = [Diagnostics.Stopwatch]::StartNew()
while ($deadline.Elapsed.TotalSeconds -lt 60) {
    if (!(Get-Process -Id $rootPid -ErrorAction SilentlyContinue)) { throw 'Installed desktop process exited before rendering' }
    foreach ($app in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
        $handle = $app.MainWindowHandle
        if ($app.Path -ine $env:DZC_TEST_DESKTOP -or $handle -eq [IntPtr]::Zero) { continue }
        if ($app.Id -ne $rootPid) {
            $owner = Get-CimInstance Win32_Process -Filter "ProcessId=$($app.Id)"
            if (!$owner -or $owner.ParentProcessId -ne $rootPid) { continue }
        }
        try {
            $window = [Windows.Automation.AutomationElement]::FromHandle($handle)
            $button = $window.FindFirst([Windows.Automation.TreeScope]::Descendants, $caption)
            if ($button -and $button.Current.IsEnabled -and !$button.Current.IsOffscreen) { exit 0 }
        } catch {
            $current = Get-Process -Id $app.Id -ErrorAction SilentlyContinue
            if ($current -and $current.MainWindowHandle -eq $handle) { throw }
        }
    }
    Start-Sleep -Milliseconds 100
}
throw 'The installed clipboard window did not render its Settings control'
"""


def install_location(product):
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, product, 0,
                            winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as key:
            return Path(winreg.QueryValueEx(key, 'InstallLocation')[0].strip('"')).resolve()
    except FileNotFoundError:
        return None


def role_info(root, release, logs, stage, env):
    for role, arguments in (('cli', ['build-info', '--json']), ('service', ['--build-info'])):
        name = 'dzc.exe' if role == 'cli' else 'dzc-service.exe'
        raw = run([root / name, *arguments], logs, f'{stage}-{role}', env, timeout=20)
        info = json_output(raw)
        require(info.get('component') == role and info.get('component_version') == release['tag'][1:]
                and info.get('source_revision') == release['source_revision']
                and info.get('target') == 'x86_64-pc-windows-msvc', f'{stage} installed {role} identity differs')


def launch(root, receipt, logs, env):
    if receipt['variant'] == 'tauri':
        arguments = ['node', ROOT / 'scripts/release-identity-desktop.mjs', root / 'dezirclip.exe',
                     logs.parent / 'desktop-profile', sys.executable, receipt['source_revision'], 'installed-candidate']
        if receipt['origin'] == 'published-replay':
            arguments.append(logs.parent / 'windows-candidate-inputs.json')
        info = json_output(run(arguments, logs, 'desktop-info', env, timeout=170))
        require(info.get('component') == 'desktop' and info.get('component_version') == receipt['tag'][1:]
                and info.get('source_revision') == receipt['source_revision']
                and info.get('target') == 'x86_64-pc-windows-msvc', 'Installed Tauri window identity differs')
        return
    desktop = root / 'dzc-desktop.exe'
    with (logs / 'desktop.stdout.log').open('xb') as stdout, (logs / 'desktop.stderr.log').open('xb') as stderr:
        process = subprocess.Popen([desktop], cwd=root, env=env, stdin=subprocess.DEVNULL,
                                   stdout=stdout, stderr=stderr)
        try:
            run(['powershell.exe', '-NoProfile', '-NonInteractive', '-STA', '-Command', WINDOW_CHECK],
                logs, 'desktop-window', {**env, 'DZC_TEST_DESKTOP': str(desktop),
                                         'DZC_TEST_DESKTOP_PID': str(process.pid)}, timeout=75)
        finally:
            if process.poll() is None:
                run(['taskkill.exe', '/pid', str(process.pid), '/t', '/f'], logs, 'desktop-exit', timeout=20)
            process.wait(timeout=10)
            run([root / 'dzc.exe', 'service', 'stop'], logs, 'service-exit', env, timeout=20)


def read_profile(database, marker):
    with sqlite3.connect(database) as connection:
        rows = connection.execute('SELECT content,content_type FROM clipboard_history WHERE content=?', (marker,)).fetchall()
        preferences = dict(connection.execute(
            "SELECT key,value FROM settings WHERE key IN ('app.language','app.update_channel','app.window_pinned')"))
    require(rows == [(marker, 'text')] and preferences == {
        'app.language': 'en', 'app.update_channel': 'preview', 'app.window_pinned': 'true',
    }, 'The original clipboard entry or preferences were lost')


def verify(directory, output):
    assert_disposable_windows(sys.platform, os.environ)
    native_target(TARGET)
    fixture = owned(directory, os.environ)
    receipt = json.loads((fixture / 'windows-candidate-inputs.json').read_text(encoding='utf-8'))
    require(receipt.get('schema_version') == 1 and receipt.get('target') == TARGET
            and receipt.get('variant') in ('tauri', 'mygo')
            and receipt.get('run_id') == int(os.environ['GITHUB_RUN_ID'])
            and receipt.get('run_attempt') == int(os.environ['GITHUB_RUN_ATTEMPT']), 'Invalid Windows candidate inputs')
    if receipt.get('origin') == 'release-producer':
        require(os.environ.get('GITHUB_EVENT_NAME') == 'push'
                and os.environ.get('GITHUB_REF') == 'refs/tags/' + receipt['tag']
                and os.environ.get('GITHUB_SHA') == receipt['source_revision'], 'Invalid release candidate source')
    else:
        require(receipt.get('origin') == 'published-replay' and os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
                and receipt['tag'] == 'v0.2.0', 'Invalid read-only replay inputs')
    mygo = receipt['variant'] == 'mygo'
    archives = {}
    members = {}
    for phase in ('previous', 'current'):
        identity = receipt[phase]
        name = safe_member(identity['name'])
        require('/' not in name and name.endswith('-setup.exe'), 'Invalid installer name')
        archive = owned(fixture / name, os.environ)
        require(archive.is_file() and sha(archive) == identity['sha256'], 'Staged installer digest differs')
        archives[phase] = archive
        logs = fixture / f'{phase}-archive'
        extraction = logs / 'payload'
        logs.mkdir()
        unpack(archive, extraction, logs, TARGET)
        members[phase] = desktop_package_files(extraction, TARGET, mygo)

    product_key = UNINSTALL + ('-MyGo' if mygo else '')
    installed = (Path(os.environ['LOCALAPPDATA']) / 'Programs' / 'DezirClip-MyGo' if mygo else fixture / 'installed').resolve()
    require(not installed.exists() and install_location(product_key) is None, 'A prior installation already exists')
    logs = fixture / 'installed-checks'
    env = windows_environment(logs)
    database = Path(env['DEZIRCLIP_DB_PATH'])
    install_directory = [] if mygo else ['/D=' + str(installed)]

    def check_files(phase):
        require(install_location(product_key) == installed, 'Unexpected registered installation directory')
        for member in members[phase].values():
            actual = installed / member['path']
            require(actual.is_file() and not actual.is_symlink() and sha(actual) == member['sha256'],
                    'Installed file differs from the signed payload: ' + member['path'])

    run([archives['previous'], '/S', *install_directory], logs, 'install-previous', timeout=120)
    check_files('previous')
    role_info(installed, receipt['previous'], logs, 'previous', env)
    marker = 'releaseupgradewindows' + uuid.uuid4().hex
    run([installed / 'dzc.exe', 'add', marker], logs, 'seed-history', env, timeout=20)
    with sqlite3.connect(database) as connection:
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', {
            'app.language': 'en', 'app.update_channel': 'preview', 'app.update_auto_check': 'false',
            'app.silent_start': 'false', 'app.idle_destroy_enabled': 'false', 'app.window_pinned': 'true',
            'app.ocr_enabled': 'false', 'app.classification_enabled': 'false',
        }.items())
    read_profile(database, marker)
    user_file = installed / 'user-file.txt'
    user_file.write_text(marker, encoding='utf-8')
    upgrade_options = [] if mygo else ['/UPDATE']
    run([archives['current'], '/S', *upgrade_options, *install_directory], logs, 'upgrade', timeout=120)
    check_files('current')
    role_info(installed, receipt['current'], logs, 'upgraded', env)
    read_profile(database, marker)
    launch(installed, receipt, logs, env)
    run([archives['current'], '/S', *install_directory], logs, 'reinstall', timeout=120)
    check_files('current')
    run([installed / 'uninstall.exe', '/S', '_?=' + str(installed)], logs, 'uninstall', timeout=120)
    require(install_location(product_key) is None, 'Uninstall registration remains')
    require(all(not (installed / member['path']).exists() for member in members['current'].values()),
            'Uninstall left installed executables')
    require(user_file.read_text(encoding='utf-8') == marker, 'Installer removed an unowned file')
    read_profile(database, marker)
    require(all(sha(archive) == receipt[phase]['sha256'] for phase, archive in archives.items()), 'Installer changed during execution')
    with owned(output, os.environ).open('x', encoding='utf-8') as file:
        json.dump({**receipt, 'status': 'passed', 'members': members['current']}, file, indent=2)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    verify(args.directory, args.output)
