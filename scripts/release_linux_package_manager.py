"""Exercise installed DEB and RPM transactions in a disposable native container."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import json
import os
from pathlib import Path
import re
import sqlite3
import tempfile

from release_identities import ROOT, inspect_machine, json_output, native_target, require, run, sha, validate_info
from release_identity_packages import linux_package_paths
from release_linux_runtime import checked_archive, profile_environment
from release_version import rpm_parts


def installed_versions(manager, logs, step):
    if manager == 'dpkg':
        result = run(['dpkg-query', '-W', '-f=${Package}\t${Version}\t${Status}\n'], logs, step)
        return {name: version for name, version, status in (line.split('\t') for line in result.splitlines())
                if status == 'install ok installed'}
    result = run(['rpm', '-qa', '--qf', '%{NAME}\t%{VERSION}-%{RELEASE}\n'], logs, step)
    return dict(line.split('\t') for line in result.splitlines())


def verified_install(manager, package_name, package, files, target, logs, step):
    version = package['tag'][1:]
    installed = version if manager == 'dpkg' else '-'.join(rpm_parts(version))
    require(installed_versions(manager, logs, step + '-registry').get(package_name) == installed,
            'Installed package-manager version differs')
    verify = ['dpkg', '--verify', package_name] if manager == 'dpkg' else ['rpm', '-V', package_name]
    require(not run(verify, logs, step + '-verify').strip(), 'Package manager found mismatched files')
    members = {}
    for role in ('desktop', 'cli', 'service'):
        path = files[role]
        require(path.is_file() and not path.is_symlink() and os.access(path, os.X_OK),
                'Missing installed program: ' + role)
        inspect_machine(path, target)
        members[role] = {'path': path.relative_to('/').as_posix(), 'sha256': sha(path)}
    require(os.access(files['launcher'], os.X_OK) and files['desktop_file'].is_file(),
            'Missing installed desktop registration')
    for role, args in (('cli', ['build-info', '--json']), ('service', ['--build-info'])):
        info = json_output(run([files[role], *args], logs, step + '-' + role))
        validate_info(info, role, target, package['source_revision'], version=version)
    return members


def retained_profile(path, marker):
    with closing(sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)) as conn:
        require(conn.execute('SELECT COUNT(*) FROM clipboard_history WHERE content=?', (marker,)).fetchone() == (1,),
                'Package transaction changed the existing history')
        require(conn.execute("SELECT value FROM settings WHERE key='app.language'").fetchone() == ('en',),
                'Package transaction changed a saved preference')


def exercise(fixture, output, manager, target, current, previous):
    mygo = current['name'].startswith('DezirClip-MyGo')
    package_name = 'dezir-clip-my-go' if mygo else 'dezir-clip'
    suffix = '.deb' if manager == 'dpkg' else '.rpm'
    for package in (previous, current):
        name = package['name']
        require(re.fullmatch(r'[a-zA-Z0-9._+-]+', name) and name.endswith(suffix)
                and name.startswith('DezirClip-MyGo') == mygo
                and re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-z]+\.?[0-9]+)?', package['tag'])
                and re.fullmatch('[0-9a-f]{40}', package['source_revision']),
                'Package transaction input identity differs')
    require(previous['name'] != current['name'] and previous['tag'] != current['tag'],
            'Package transaction requires an earlier release')
    archives = [checked_archive(fixture, package) for package in (previous, current)]
    files = linux_package_paths(Path('/'), mygo)
    require(package_name not in installed_versions(manager, output, 'initial-registry')
            and all(not path.exists() for path in files.values()), 'Expected an installation-free container')
    install = ['dpkg', '--install'] if manager == 'dpkg' else ['rpm', '--upgrade']
    run([*install, archives[0]], output, 'install-previous')
    verified_install(manager, package_name, previous, files, target, output, 'previous')
    marker = 'dzclinuxupgrade' + current['sha256'][:12]
    with tempfile.TemporaryDirectory(prefix='dzc-package-profile-') as temp:
        env = profile_environment(Path(temp) / 'profile')
        database = Path(env['DEZIRCLIP_DB_PATH'])
        run([files['cli'], 'add', marker], output, 'seed-history', env)
        with closing(sqlite3.connect(database)) as conn, conn:
            conn.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.language','en')")
        retained_profile(database, marker)

        run([*install, archives[1]], output, 'upgrade')
        members = verified_install(manager, package_name, current, files, target, output, 'candidate')
        require(marker in run([files['cli'], 'search', marker, '--metadata', '--json'], output, 'candidate-history', env),
                'Upgraded CLI could not read the original profile')
        retained_profile(database, marker)

        reinstall = install if manager == 'dpkg' else [*install, '--replacepkgs']
        run([*reinstall, archives[1]], output, 'reinstall')
        require(verified_install(manager, package_name, current, files, target, output, 'reinstalled') == members,
                'Reinstall changed the candidate program bytes')
        retained_profile(database, marker)

        remove = ['dpkg', '--purge'] if manager == 'dpkg' else ['rpm', '--erase']
        run([*remove, package_name], output, 'uninstall')
        require(package_name not in installed_versions(manager, output, 'uninstalled-registry')
                and all(not path.exists() and not path.is_symlink() for path in files.values()),
                'Package manager left an installed program or desktop entry')
        retained_profile(database, marker)
    return {'package_manager': manager, 'current': current, 'previous': previous, 'members': members,
            'installed': True, 'upgraded': True, 'reinstalled': True, 'uninstalled': True, 'profile_retained': True}


CONTAINER_RECIPES = {
    'dpkg': (
        'ubuntu:24.04',
        'apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends '
        'libwebkit2gtk-4.1-0 libgtk-3-0t64 libgstreamer1.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good '
        'libx11-6 libxi6 libxrandr2 libxtst6 libdbus-1-3 xdg-utils libayatana-appindicator3-1 python3',
    ),
    'rpm': (
        'fedora:44',
        'dnf -y --setopt=install_weak_deps=False install gtk3 webkit2gtk4.1 dbus-libs gstreamer1 '
        'gstreamer1-plugins-base gstreamer1-plugins-good libX11 libXi libXrandr libXtst xdg-utils python3',
    ),
}


def run_container(manager, fixture, results, namespaces):
    image, install_deps = CONTAINER_RECIPES[manager]
    # Only distribution dependencies enter the image. Authenticated release
    # bytes are mounted read-only when the network-isolated container runs.
    with tempfile.TemporaryDirectory(prefix='dzc-package-image-') as directory:
        work = Path(directory)
        (work / 'Dockerfile').write_text(f'FROM {image}\nRUN {install_deps}\n', encoding='utf-8')
        iid = work / 'image.id'
        run(['docker', 'build', '--iidfile', iid, work], results, manager + '-container-build', timeout=360)
        image_id = iid.read_text(encoding='utf-8').strip()
        require(re.fullmatch(r'sha256:[0-9a-f]{64}', image_id), 'Invalid package-manager image identity')
    command = [
        'docker', 'run', '--rm', '--pull=never', '--network=none', '--security-opt=no-new-privileges',
        '-e', 'HOST_NAMESPACES=' + json.dumps(namespaces),
    ]
    for local, destination, access in (
        (fixture, '/inputs', ',readonly'), (results, '/results', ''),
        (ROOT / 'scripts', '/source/scripts', ',readonly'),
        (ROOT / 'Cargo.toml', '/source/Cargo.toml', ',readonly'),
        (ROOT / 'package.json', '/source/package.json', ',readonly'),
        (ROOT / 'src-tauri/tauri.conf.json', '/source/src-tauri/tauri.conf.json', ',readonly'),
    ):
        command.extend(['--mount', f'type=bind,src={local},dst={destination}{access}'])
    run([*command, image_id, 'python3', '/source/scripts/release_linux_package_manager.py',
         manager, '/inputs', '/results'], results, manager + '-transactions', timeout=90)
    report = json_output((results / manager / 'transactions.json').read_text(encoding='utf-8'))
    return report


def package_transactions(fixture, results, target):
    namespaces = {name: os.readlink(f'/proc/self/ns/{name}') for name in ('pid', 'net', 'ipc')}
    with ThreadPoolExecutor(max_workers=len(CONTAINER_RECIPES)) as pool:
        jobs = [pool.submit(run_container, manager, fixture, results, namespaces) for manager in CONTAINER_RECIPES]
        reports = [job.result() for job in jobs]
    require(all(report['target'] == target for report in reports), 'Package manager runner target differs')
    return [item for report in reports for item in report['packages']]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manager', choices=('dpkg', 'rpm'))
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    namespace = json.loads(os.environ['HOST_NAMESPACES'])
    require(os.geteuid() == 0 and Path('/.dockerenv').is_file()
            and all(os.readlink(f'/proc/self/ns/{name}') != namespace[name] for name in ('pid', 'net', 'ipc')),
            'Package-manager transactions require a private native container')
    inputs = json.loads((args.fixture / 'linux-package-transactions-inputs.json').read_text(encoding='utf-8'))
    target = inputs['target']
    native_target(target)
    suffix = '.deb' if args.manager == 'dpkg' else '.rpm'
    groups = [item for item in inputs['packages'] if item['current']['name'].endswith(suffix)]
    require(len(groups) == 2 and {item['current']['name'].startswith('DezirClip-MyGo') for item in groups} == {True, False},
            'Expected the Tauri and MyGo packages for this manager')
    destination = args.output / args.manager
    destination.mkdir()
    results = [
        exercise(args.fixture, destination / str(index), args.manager, target, item['current'], item['previous'])
        for index, item in enumerate(groups)
    ]
    with (destination / 'transactions.json').open('x', encoding='utf-8') as stream:
        json.dump({'target': target, 'packages': results}, stream, indent=2)


if __name__ == '__main__':
    main()
