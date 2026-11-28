"""Exercise installed desktop package transactions in disposable native containers."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import tempfile

from release_identities import ROOT, inspect_machine, json_output, native_target, require, run, sha, validate_info
from release_identity_packages import linux_package_paths
from release_linux_runtime import checked_archive, profile_environment
from release_version import rpm_parts


SUFFIXES = {'dpkg': '.deb', 'rpm': '.rpm', 'pacman': '.pkg.tar.zst'}
INSTALL = {'dpkg': ['dpkg', '--install'], 'rpm': ['rpm', '--upgrade'],
           'pacman': ['pacman', '-U', '--noconfirm', '--']}
REMOVE = {'dpkg': ['dpkg', '--purge'], 'rpm': ['rpm', '--erase'],
          'pacman': ['pacman', '-R', '--noconfirm', '--']}


def is_mygo(name):
    return name.startswith(('DezirClip-MyGo', 'dezirclip-mygo-bin-'))


def arch_package_name(mygo):
    return 'dezirclip-mygo-bin' if mygo else 'dezirclip-bin'


def installed_versions(manager, logs, step):
    if manager == 'dpkg':
        result = run(['dpkg-query', '-W', '-f=${Package}\t${Version}\t${Status}\n'], logs, step)
        return {name: version for name, version, status in (line.split('\t') for line in result.splitlines())
                if status == 'install ok installed'}
    if manager == 'rpm':
        result = run(['rpm', '-qa', '--qf', '%{NAME}\t%{VERSION}-%{RELEASE}\n'], logs, step)
        return dict(line.split('\t') for line in result.splitlines())
    result = run(['pacman', '-Q'], logs, step)
    return dict(line.split() for line in result.splitlines())


def installed_version(manager, archive, package, package_name, logs, step):
    version = package['tag'][1:]
    if manager == 'dpkg':
        return version
    if manager == 'rpm':
        return '-'.join(rpm_parts(version))
    name, expected = run(['pacman', '-Qp', '--', archive], logs, step + '-metadata').split()
    require(name == package_name and archive.name == f'{name}-{expected}-x86_64.pkg.tar.zst',
            'Arch package metadata differs from its authenticated filename')
    return expected


def verified_install(manager, package_name, installed, package, files, target, logs, step):
    require(installed_versions(manager, logs, step + '-registry').get(package_name) == installed,
            'Installed package-manager version differs')
    verify = {'dpkg': ['dpkg', '--verify'], 'rpm': ['rpm', '-V'], 'pacman': ['pacman', '-Qkk']}
    output = run([*verify[manager], package_name], logs, step + '-verify').strip()
    expected = (re.fullmatch(re.escape(package_name) + r': [0-9]+ total files, 0 altered files', output)
                if manager == 'pacman' else not output)
    require(expected, 'Package manager found mismatched files')
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
        validate_info(info, role, target, package['source_revision'], version=package['tag'][1:])
    return members


def retained_profile(path, marker):
    with closing(sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)) as conn:
        require(conn.execute('SELECT COUNT(*) FROM clipboard_history WHERE content=?', (marker,)).fetchone() == (1,),
                'Package transaction changed the existing history')
        require(conn.execute("SELECT value FROM settings WHERE key='app.language'").fetchone() == ('en',),
                'Package transaction changed a saved preference')


def exercise(fixture, output, manager, target, current, previous):
    mygo = is_mygo(current['name'])
    package_name = (arch_package_name(mygo) if manager == 'pacman'
                    else 'dezir-clip-my-go' if mygo else 'dezir-clip')
    suffix = SUFFIXES[manager]
    for package in (previous, current):
        name = package['name']
        require(re.fullmatch(r'[a-zA-Z0-9._+-]+', name) and name.endswith(suffix)
                and is_mygo(name) == mygo
                and re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-z]+\.?[0-9]+)?', package['tag'])
                and re.fullmatch('[0-9a-f]{40}', package['source_revision']),
                'Package transaction input identity differs')
    require(previous['name'] != current['name'] and previous['tag'] != current['tag'],
            'Package transaction requires an earlier release')
    archives = [checked_archive(fixture, package) for package in (previous, current)]
    versions = [installed_version(manager, archive, package, package_name, output, step)
                for archive, package, step in zip(archives, (previous, current), ('previous', 'candidate'), strict=True)]
    if manager == 'pacman':
        require(run(['vercmp', *versions], output, 'version-order').strip() == '-1',
                'Arch baseline is not older than the candidate')
    files = linux_package_paths(Path('/'), mygo)
    require(package_name not in installed_versions(manager, output, 'initial-registry')
            and all(not path.exists() for path in files.values()), 'Expected an installation-free container')
    install = INSTALL[manager]
    run([*install, archives[0]], output, 'install-previous')
    previous_members = verified_install(manager, package_name, versions[0], previous, files, target, output, 'previous')
    marker = 'dzclinuxupgrade' + current['sha256'][:12]
    with tempfile.TemporaryDirectory(prefix='dzc-package-profile-') as temp:
        env = profile_environment(Path(temp) / 'profile')
        database = Path(env['DEZIRCLIP_DB_PATH'])
        run([files['cli'], 'add', marker], output, 'seed-history', env)
        with closing(sqlite3.connect(database)) as conn, conn:
            conn.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.language','en')")
        retained_profile(database, marker)

        run([*install, archives[1]], output, 'upgrade')
        members = verified_install(manager, package_name, versions[1], current, files, target, output, 'candidate')
        require(marker in run([files['cli'], 'search', marker, '--metadata', '--json'], output, 'candidate-history', env),
                'Upgraded CLI could not read the original profile')
        retained_profile(database, marker)

        reinstall = [*install, '--replacepkgs'] if manager == 'rpm' else install
        run([*reinstall, archives[1]], output, 'reinstall')
        require(verified_install(manager, package_name, versions[1], current, files, target, output, 'reinstalled') == members,
                'Reinstall changed the candidate program bytes')
        retained_profile(database, marker)

        run([*REMOVE[manager], package_name], output, 'uninstall')
        require(package_name not in installed_versions(manager, output, 'uninstalled-registry')
                and all(not path.exists() and not path.is_symlink() for path in files.values()),
                'Package manager left an installed program or desktop entry')
        retained_profile(database, marker)
    return {'package_manager': manager, 'current': current, 'previous': previous, 'members': members,
            'previous_members': previous_members,
            'installed': True, 'upgraded': True, 'reinstalled': True, 'uninstalled': True, 'profile_retained': True}


CONTAINER_RECIPES = {
    'dpkg': (
        'ubuntu:24.04',
        'apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends '
        'libwebkit2gtk-4.1-0 libgtk-3-0t64 libgstreamer1.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good '
        'libx11-6 libxi6 libxrandr2 libxtst6 libdbus-1-3 xdg-utils libayatana-appindicator3-1 python3',
    ),
    'pacman': (
        None,
        'pacman -Syu --noconfirm --needed gtk3 webkit2gtk-4.1 libayatana-appindicator xdg-utils libx11 '
        'libxi libxrandr libxtst gst-plugins-base gst-plugins-good dbus python',
    ),
    'rpm': (
        'fedora:44',
        'dnf -y --setopt=install_weak_deps=False install gtk3 webkit2gtk4.1 dbus-libs gstreamer1 '
        'gstreamer1-plugins-base gstreamer1-plugins-good libX11 libXi libXrandr libXtst xdg-utils python3',
    ),
}


def arch_image():
    image = os.environ.get('DZC_ARCH_BUILDER_IMAGE', '')
    require(re.fullmatch(r'archlinux:base-devel@sha256:[0-9a-f]{64}', image), 'Missing pinned Arch release builder')
    return image


def run_container(manager, fixture, results, namespaces):
    image, install_deps = CONTAINER_RECIPES[manager]
    if manager == 'pacman':
        image = arch_image()
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


def repack_debian(fixture, output, debs, tag, step):
    require(len(debs) == 2 and {is_mygo(deb['name']) for deb in debs} == {False, True}
            and all(deb['tag'] == tag for deb in debs), 'Expected both signed Debian archives for Arch repackaging')
    output.mkdir()
    mount = [
        '--mount', f'type=bind,src={fixture},dst=/debs,readonly',
        '--mount', f'type=bind,src={ROOT / "scripts/bundle-archlinux.sh"},dst=/bundle.sh,readonly',
        '--mount', f'type=bind,src={output},dst=/output',
    ]
    ordered = sorted(debs, key=lambda item: is_mygo(item['name']))
    inputs = [Path('/debs') / checked_archive(fixture, deb).name for deb in ordered]
    run(['docker', 'run', '--rm', '--network=none', '--user', f'{os.getuid()}:{os.getgid()}',
         *mount, arch_image(), 'bash', '/bundle.sh', tag, *inputs, '/output'],
        output, step + '-build', timeout=180)
    packages = {}
    for deb in ordered:
        archives = [path for path in output.glob(arch_package_name(is_mygo(deb['name'])) + '-*.pkg.tar.zst')
                    if path.is_file() and not path.is_symlink()]
        require(len(archives) == 1, 'Arch repackaging did not produce one expected binary archive')
        path, = archives
        packages[is_mygo(deb['name'])] = {
            'name': path.name, 'tag': deb['tag'], 'source_revision': deb['source_revision'],
            'size': path.stat().st_size, 'sha256': sha(path), 'debian_source': deb,
        }
    return packages


def run_arch_transactions(fixture, results, runtime, deb_pairs, namespaces):
    arch = results / 'arch-inputs'
    archives = arch / 'archives'
    archives.mkdir(parents=True)
    previous_debs = [pair['previous'] for pair in deb_pairs]
    previous_dir = arch / 'previous-repack'
    previous = repack_debian(fixture, previous_dir, previous_debs, previous_debs[0]['tag'], 'baseline')
    if runtime['origin'] == 'published-replay':
        current_dir = arch / 'current-repack'
        current = repack_debian(fixture, current_dir, [pair['current'] for pair in deb_pairs], runtime['tag'], 'replay')
    else:
        current_dir = fixture
        inputs = [item for item in runtime['packages'] if item['name'].endswith(SUFFIXES['pacman'])]
        require(len(inputs) == 2 and {is_mygo(item['name']) for item in inputs} == {False, True},
                'Expected both exact Arch candidate archives')
        current = {is_mygo(item['name']): {**item, 'tag': runtime['tag'], 'source_revision': runtime['source_revision']}
                   for item in inputs}

    for packages, directory in ((previous, previous_dir), (current, current_dir)):
        for item in packages.values():
            source = checked_archive(directory, item)
            with source.open('rb') as stream, (archives / source.name).open('xb') as staged:
                shutil.copyfileobj(stream, staged, length=1024 * 1024)
    manifest = {'target': runtime['target'], 'packages': [
        {'current': current[mygo], 'previous': previous[mygo]} for mygo in (False, True)
    ]}
    (archives / 'linux-package-transactions-inputs.json').write_text(json.dumps(manifest), encoding='utf-8')
    return run_container('pacman', archives, results, namespaces)


def package_transactions(fixture, results, runtime):
    namespaces = {name: os.readlink(f'/proc/self/ns/{name}') for name in ('pid', 'net', 'ipc')}
    target = runtime['target']
    with ThreadPoolExecutor(max_workers=len(CONTAINER_RECIPES)) as pool:
        dpkg = pool.submit(run_container, 'dpkg', fixture, results, namespaces)
        rpm = pool.submit(run_container, 'rpm', fixture, results, namespaces)
        jobs = [dpkg, rpm]
        if target == 'linux-x86_64':
            inputs = json_output((fixture / 'linux-package-transactions-inputs.json').read_text(encoding='utf-8'))
            deb_pairs = [item for item in inputs['packages'] if item['current']['name'].endswith(SUFFIXES['dpkg'])]
            require(len(deb_pairs) == 2 and {is_mygo(item['current']['name']) for item in deb_pairs} == {False, True},
                    'Expected the two authenticated Debian transaction inputs')
            jobs.append(pool.submit(run_arch_transactions, fixture, results, runtime, deb_pairs, namespaces))
        reports = [job.result() for job in jobs]
    require(all(report['target'] == target for report in reports), 'Package manager runner target differs')
    debs = {item['previous']['name']: item for item in reports[0]['packages']}
    transactions = [item for report in reports for item in report['packages']]
    for item in transactions:
        if item['package_manager'] == 'pacman':
            source = item['previous']['debian_source']
            deb = debs[source['name']]
            require(source == deb['previous'] and item['previous_members'] == deb['previous_members'],
                    'Installed Arch baseline differs from the authenticated Debian release')
    return transactions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manager', choices=SUFFIXES)
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
    suffix = SUFFIXES[args.manager]
    groups = [item for item in inputs['packages'] if item['current']['name'].endswith(suffix)]
    require(len(groups) == 2 and {is_mygo(item['current']['name']) for item in groups} == {True, False},
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
