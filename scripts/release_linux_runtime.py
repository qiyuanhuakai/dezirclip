"""Run the shipped Linux desktop and both bundled helpers in disposable X11 sessions."""
import argparse
from contextlib import contextmanager, closing
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import time
from urllib.request import Request, urlopen

from release_identities import ROOT, TARGETS, json_output, native_target, require, run, sha, validate_info
from release_identity_packages import desktop_package_files, unpack

RECEIPT = 'linux-runtime-inputs.json'
NAMESPACES = ('user', 'pid', 'net', 'ipc')
FORMATS = ('.deb', '.rpm', '.AppImage', '.pkg.tar.zst')


def load_inputs(fixture):
    receipt = fixture / RECEIPT
    require(receipt.is_file() and not receipt.is_symlink() and receipt.stat().st_size <= 32768,
            'Missing Linux runtime inputs')
    value = json.loads(receipt.read_text(encoding='utf-8'))
    require(value['schema_version'] == 1 and value['origin'] in ('published-replay', 'release-producer')
            and re.fullmatch('[0-9a-f]{40}', value['source_revision'])
            and re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-z]+\.?[0-9]+)?', value['tag'])
            and value['target'] in ('linux-x86_64', 'linux-aarch64'), 'Linux runtime input identity differs')
    native_target(value['target'])
    names = [package['name'] for package in value['packages']]
    expected = 8 if value['origin'] == 'release-producer' and value['target'] == 'linux-x86_64' else 6
    require(len(names) == len(set(names)) == expected, 'Wrong Linux package count')
    for name in names:
        require(name == Path(name).name and re.fullmatch(r'[a-zA-Z0-9._+-]+', name)
                and name.endswith(FORMATS), 'Unsafe Linux package name')
    return value


def checked_archive(fixture, package):
    path = fixture / package['name']
    require(path.is_file() and not path.is_symlink() and path.stat().st_size == package['size']
            and sha(path) == package['sha256'], 'Linux archive differs from authenticated inputs')
    return path


def profile_environment(profile):
    result = os.environ.copy()
    profile.mkdir()
    for variable, name in (('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache'), ('XDG_CONFIG_HOME', 'config')):
        path = profile / name
        path.mkdir()
        result[variable] = str(path)
    # Place the fixture at the real Tauri profile path as well as the CLI override.
    database = profile / 'data/io.github.qiyuanhuakai.dezirclip/clipboard.db'
    database.parent.mkdir()
    result.update(HOME=str(profile), DEZIRCLIP_DB_PATH=str(database))
    return result


def settings(database):
    with closing(sqlite3.connect(database)) as conn, conn:
        conn.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)', {
            'app.language': 'en', 'app.theme': 'retro', 'app.silent_start': 'false',
            'app.window_pinned': 'true', 'app.window_width': '430', 'app.window_height': '500',
            'app.idle_destroy_enabled': 'false', 'app.update_auto_check': 'false',
            'app.disable_webview_gpu': 'true', 'app.ocr_enabled': 'false',
        }.items())


@contextmanager
def owned_process(command, log, env):
    with log.open('xb') as stream:
        process = subprocess.Popen([str(part) for part in command], stdin=subprocess.DEVNULL,
                                   stdout=stream, stderr=subprocess.STDOUT, env=env)
        try:
            yield process
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


def executable_digest(pid):
    try:
        return sha(Path('/proc') / str(pid) / 'exe')
    except OSError:
        return None


def desktop_pid(name, digest):
    for path in Path('/proc').glob('[0-9]*/exe'):
        try:
            if Path(os.readlink(path)).name == name and executable_digest(path.parent.name) == digest:
                return int(path.parent.name)
        except OSError:
            continue
    return None


def wait_for(probe, message, process, seconds=25):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        require(process.poll() is None, message + ': process exited')
        observed = probe()
        if observed:
            return observed
        time.sleep(.2)
    raise ValueError(message)


def native_marker(marker, digest):
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi

    nodes = [Atspi.get_desktop(0)]
    for node in nodes:
        try:
            if node is None:
                continue
            name = node.get_name() or ''
            text = Atspi.Text.get_text(node, 0, -1) if 'Text' in node.get_interfaces() else ''
            if marker in name or marker in (text or ''):
                pid = node.get_process_id()
                if executable_digest(pid) == digest:
                    return pid
            nodes.extend(node.get_child_at_index(index) for index in range(min(128, node.get_child_count())))
            if len(nodes) > 4096:
                break
        except (AttributeError, RuntimeError):
            continue
    return None


def webdriver_call(port, route, payload=None, method=None):
    body = json.dumps(payload).encode() if payload is not None else None
    request = Request(f'http://127.0.0.1:{port}/{route}', data=body, method=method,
                      headers={'Content-Type': 'application/json'})
    with urlopen(request, timeout=15) as response:
        value = json_output(response.read(64 * 1024 + 1).decode())
    return value['value']


def tauri_smoke(launcher, info, marker, digest, logs, env):
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    with owned_process(['/tools/WebKitWebDriver', '--host=local', f'--port={port}'],
                       logs / 'webdriver.log', env) as driver:
        def ready():
            try:
                return webdriver_call(port, 'status')
            except OSError:
                return None

        wait_for(ready, 'WebKitWebDriver did not start', driver)
        session = webdriver_call(port, 'session', {'capabilities': {'alwaysMatch': {
            'webkitgtk:browserOptions': {'binary': str(launcher)}
        }}})
        route = 'session/' + session['sessionId']
        try:
            def rendered():
                return webdriver_call(port, route + '/execute/sync', {'script': '''
                    return typeof window.__TAURI_INTERNALS__?.invoke === "function" &&
                        document.body?.innerText.includes(arguments[0]);
                ''', 'args': [marker]})

            try:
                wait_for(rendered, 'Final Tauri WebView did not render its synthetic history row', driver)
            except ValueError:
                state = webdriver_call(port, route + '/execute/sync', {'script': '''
                    return {url: location.href, ready: document.readyState, hidden: document.hidden,
                        bridge: typeof window.__TAURI_INTERNALS__?.invoke,
                        body: document.body?.innerText.slice(0, 1500)};
                ''', 'args': []})
                (logs / 'webview-state.json').write_text(json.dumps(state), encoding='utf-8')
                raise
            details = webdriver_call(port, route + '/execute/async', {'script': '''
                const done=arguments[arguments.length-1];
                window.__TAURI_INTERNALS__.invoke("get_build_info").then(done, error=>done({error:String(error)}));
            ''', 'args': []})
            validate_info(details, 'desktop', info['target'], info['source_revision'], version=info['tag'][1:])
            require(desktop_pid('dezirclip', digest) is not None, 'Tauri ran different desktop bytes')
        finally:
            webdriver_call(port, route, method='DELETE')


def exercise(fixture, result_dir, index):
    parent = json.loads(os.environ['HOST_NAMESPACES'])
    require(all(os.readlink(f'/proc/self/ns/{name}') != parent[name] for name in NAMESPACES),
            'Linux runtime test requires private user, process and network namespaces')
    receipt = load_inputs(fixture)
    package = receipt['packages'][index]
    archive = checked_archive(fixture, package)
    logs = result_dir / f'archive-{index}'
    logs.mkdir()
    root = Path('/tmp/unpacked')
    target, source, version = receipt['target'], receipt['source_revision'], receipt['tag'][1:]
    mygo = archive.name.startswith(('DezirClip-MyGo', 'dezirclip-mygo-bin-'))
    unpack(archive, root, logs, target)
    members = desktop_package_files(root, target, mygo)
    cli = root / members['cli']['path']
    service = root / members['service']['path']
    launcher = root / 'usr/bin' / ('dezirclip-mygo' if mygo else 'dezirclip')
    env = profile_environment(Path('/tmp/profile'))
    marker = 'dzcfinalarchive' + package['sha256'][:12]
    for role, command in (('cli', [cli, 'build-info', '--json']), ('service', [service, '--build-info'])):
        validate_info(json_output(run(command, logs, role, env)), role, target, source, version=version)
    run([cli, 'add', marker], logs, 'seed-history', env)
    settings(Path(env['DEZIRCLIP_DB_PATH']))

    started = json_output(run([cli, 'service', 'start', '--no-capture', '--json'], logs, 'start-service', env))
    endpoint = json_output(run([cli, 'service', 'desktop-endpoint'], logs, 'desktop-endpoint', env))
    require(started['mode'] == 'service' and executable_digest(endpoint['pid']) == members['service']['sha256'],
            'Packaged CLI started a different Service executable')
    status = json_output(run([cli, 'service', 'status', '--json'], logs, 'service-status', env))
    require(status['state']['service_instance_id'] == started['state']['service_instance_id'], 'Service did not preserve its instance')
    if not mygo:
        run([cli, 'service', 'stop'], logs, 'stop-service-before-embedded-host', env)

    appimage = archive.name.endswith('.AppImage')
    if appimage:
        launcher = Path('/tmp') / archive.name
        shutil.copyfile(archive, launcher)
        launcher.chmod(0o700)
        require(sha(launcher) == package['sha256'], 'Launched AppImage differs from the verified archive')

    with owned_process(['openbox', '--sm-disable'], logs / 'openbox.log', env):
        if mygo:
            info = json_output(run([launcher, '--build-info'], logs, 'mygo-build-info', env))
            require(info.get('component') == 'mygo-desktop' and info.get('component_version') == version
                    and info.get('source_revision') == source and info.get('target') == TARGETS[target]
                    and info.get('source_state') == 'verified_checkout', 'MyGo desktop build identity differs')
            with owned_process([launcher, '--show'], logs / 'mygo-desktop.log', env) as desktop:
                wait_for(lambda: native_marker(marker, members['desktop']['sha256']),
                         'Final MyGo window did not render its synthetic history row', desktop)
                visible_status = json_output(run([cli, 'service', 'status', '--json'], logs, 'mygo-service-status', env))
                require(visible_status['state']['service_instance_id'] == started['state']['service_instance_id'],
                        'MyGo replaced the independently verified Service')
        else:
            tauri_smoke(launcher, receipt, marker, members['desktop']['sha256'], logs, env)
    require(marker in run([cli, 'search', marker, '--metadata', '--json'], logs, 'retained-history', env),
            'Final desktop did not preserve its synthetic history')
    return {**package, 'members': members, 'runtime': 'appimage' if appimage else 'extracted-native-launcher',
            'history_rendered': True, 'service_verified': True}


def sandbox(fixture, result_dir, driver, bsdtar, index):
    arguments = ['bwrap', '--unshare-user', '--unshare-pid', '--unshare-net', '--unshare-ipc',
                 '--die-with-parent', '--new-session', '--clearenv',
                 '--dev', '/dev', '--proc', '/proc', '--tmpfs', '/tmp', '--tmpfs', '/run',
                 '--ro-bind', '/usr', '/usr', '--ro-bind', '/etc', '/etc',
                 '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/sbin', '/sbin',
                 '--symlink', 'usr/lib', '/lib', '--symlink', 'usr/lib64', '/lib64',
                 '--ro-bind', ROOT / 'scripts', '/source/scripts',
                 '--ro-bind', ROOT / 'Cargo.toml', '/source/Cargo.toml',
                 '--ro-bind', ROOT / 'package.json', '/source/package.json',
                 '--ro-bind', ROOT / 'src-tauri/tauri.conf.json', '/source/src-tauri/tauri.conf.json',
                 '--ro-bind', fixture, '/inputs', '--bind', result_dir, '/results',
                 '--ro-bind', driver, '/tools/WebKitWebDriver',
                 '--ro-bind', bsdtar, '/tools/bsdtar', '--chdir', '/tmp']
    variables = {
        'PATH': '/usr/bin:/bin:/tools', 'HOME': '/tmp', 'XDG_RUNTIME_DIR': '/tmp/runtime', 'GDK_BACKEND': 'x11',
        'GIO_USE_VFS': 'local', 'GTK_A11Y': 'atspi', 'NO_AT_BRIDGE': '0', 'LIBGL_ALWAYS_SOFTWARE': '1',
        'APPIMAGE_EXTRACT_AND_RUN': '1', 'TAURI_WEBVIEW_AUTOMATION': 'true', 'WEBKIT_DISABLE_DMABUF_RENDERER': '1',
        'WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS': '1',
        'HOST_NAMESPACES': json.dumps({name: os.readlink(f'/proc/self/ns/{name}') for name in NAMESPACES}),
    }
    for key, value in variables.items():
        arguments.extend(['--setenv', key, value])
    arguments.extend(['/bin/sh', '-c',
        'mkdir -m 700 "$XDG_RUNTIME_DIR" && exec xvfb-run -a -e /dev/stderr dbus-run-session -- /usr/bin/python3 "$@"',
        'linux-runtime', '/source/scripts/release_linux_runtime.py', '--inside', '/inputs', '/results', str(index)])
    run(arguments, result_dir, f'sandbox-{index}', timeout=100)
    return json_output((result_dir / f'archive-{index}/runtime.json').read_text(encoding='utf-8'))


def main():
    if len(sys.argv) == 5 and sys.argv[1] == '--inside':
        fixture, output, index = Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4])
        report = exercise(fixture, output, index)
        with (output / f'archive-{index}/runtime.json').open('x', encoding='utf-8') as stream:
            json.dump(report, stream)
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--driver', type=Path, default=Path('/usr/bin/WebKitWebDriver'))
    parser.add_argument('--bsdtar', type=Path, default=Path('/usr/bin/bsdtar'))
    arguments = parser.parse_args()
    fixture, output = arguments.fixture.resolve(strict=True), arguments.output.resolve()
    inputs = load_inputs(fixture)
    output.mkdir(mode=0o700)
    driver = arguments.driver.resolve(strict=True)
    bsdtar = arguments.bsdtar.resolve(strict=True)
    reports = []
    for index, package in enumerate(inputs['packages']):
        print(f"Verify native Linux runtime for {package['name']}", file=sys.stderr, flush=True)
        reports.append(sandbox(fixture, output, driver, bsdtar, index))
    with (output / 'linux-runtime.json').open('x', encoding='utf-8') as report:
        json.dump({**inputs, 'status': 'passed', 'packages': reports}, report, indent=2)


if __name__ == '__main__':
    main()
