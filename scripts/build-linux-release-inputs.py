"""Supply source-bound Linux release ELFs on an explicitly requested hosted CI run."""
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import time
import tomllib

TARGET = 'x86_64-unknown-linux-gnu'
REPOSITORY = 'qiyuanhuakai/dezirclip'
ROOT = Path(__file__).resolve().parents[1]
PATCH = 'mygo-v0.6.4-no-startup-cleanup-v1'
MYGO_SOURCE = '6255ea40139d06e0762a7c4a32626148ba410478'
MAX_BINARY = 256 * 1024 * 1024
MAX_METADATA = 32 * 1024 * 1024
MAX_JSON = 1024 * 1024
BUILD_TIMEOUT_SECONDS = 3000
BUILD_WAIT_SECONDS = 30
COMMAND = ['cargo', 'build', '--release', '--locked', '--manifest-path', 'src-tauri/Cargo.toml',
           '--features', 'tauri/custom-protocol', '--bin', 'dezirclip', '--message-format=json']
RELEASE_PROFILE = {'lto': True, 'codegen-units': 1, 'opt-level': 's', 'strip': True, 'panic': 'abort'}


def require(value, message):
    if not value:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path, maximum):
    path = Path(path)
    require(path.is_file() and not path.is_symlink() and path.absolute() == path.resolve(strict=True),
            'A direct regular build input is required: ' + str(path))
    before = path.stat()
    require(0 <= before.st_size <= maximum, 'Build input exceeds its declared bound')
    with path.open('rb') as stream:
        data = stream.read(maximum + 1)
    after = path.stat()
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
            and len(data) == before.st_size, 'Build input changed during reading')
    return data


def json_value(data):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'Duplicate JSON member')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError('Nonfinite JSON')))


def save(path, data):
    with path.open('xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    require(regular(path, max(len(data), 1)) == data, 'Output readback differs')
    return {'path': path.name, 'bytes': len(data), 'sha256': digest(data)}


def document(path, value):
    data = (json.dumps(value, indent=2, ensure_ascii=True, allow_nan=False) + '\n').encode()
    require(len(data) <= MAX_JSON, 'Producer document exceeds its declared bound')
    return save(path, data)


def small(command, root):
    result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=30, check=False)
    require(len(result.stdout) + len(result.stderr) <= MAX_JSON, 'Command metadata exceeds its bound')
    require(result.returncode == 0, 'Command failed: ' + repr(command) + ': ' + result.stderr.decode('utf-8', 'replace'))
    return result.stdout, result.stderr


def checkout(root, environment):
    revision = small(['git', 'rev-parse', 'HEAD'], root)[0].decode('utf-8').strip()
    require(revision == environment['GITHUB_SHA'], 'Actual Git HEAD differs from the CI source')
    require(not small(['git', 'status', '--porcelain=v1', '--untracked-files=no'], root)[0].strip(),
            'Tracked checkout must remain clean')
    return revision


def authorize(root, environment, platform):
    require(platform == 'linux' and environment.get('RUNNER_ARCH') == 'X64'
            and environment.get('GITHUB_ACTIONS') == 'true'
            and environment.get('RUNNER_ENVIRONMENT') == 'github-hosted'
            and environment.get('GITHUB_REPOSITORY') == REPOSITORY
            and environment.get('GITHUB_JOB') == 'tauri'
            and environment.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
            and environment.get('DEZIRCLIP_LINUX_RELEASE_INPUTS') == 'true',
            'Only the explicit hosted Linux x64 tauri dispatch may supply release inputs')
    require(re.fullmatch('[a-f0-9]{40}', environment.get('GITHUB_SHA', ''))
            and all(re.fullmatch('[1-9][0-9]*', environment.get(name, ''))
                    for name in ('GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT')), 'Immutable CI identity required')
    workspace = Path(environment['GITHUB_WORKSPACE']).resolve(strict=True)
    temporary = Path(environment['RUNNER_TEMP']).resolve(strict=True)
    require(root.resolve(strict=True) == workspace and temporary.is_dir(), 'Original CI workspace and temporary root required')
    event = Path(environment['GITHUB_EVENT_PATH'])
    require(event.resolve(strict=True).is_relative_to(temporary), 'Original runner dispatch event required')
    inputs = json_value(regular(event, MAX_JSON)).get('inputs', {})
    requested = inputs.get('linux_release_inputs')
    require(requested is True or type(requested) is str and requested == 'true', 'Actual dispatch input must be true')
    require(not environment.get('CARGO_BUILD_TARGET') and not environment.get('CARGO_TARGET_DIR')
            and not environment.get('RUSTFLAGS') and not environment.get('CARGO_ENCODED_RUSTFLAGS')
            and not environment.get('CARGO_BUILD_RUSTFLAGS')
            and not environment.get('CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS')
            and not any(name.startswith('CARGO_PROFILE_RELEASE_') for name in environment),
            'The existing native host release profile must not be overridden')
    revision = checkout(root, environment)
    return temporary, {'source_revision': revision, 'source_state': 'verified_checkout', 'target': TARGET,
                       'run_id': environment['GITHUB_RUN_ID'], 'run_attempt': environment['GITHUB_RUN_ATTEMPT'],
                       'repository': REPOSITORY, 'job': 'tauri', 'event_name': 'workflow_dispatch'}


def elf(data):
    require(len(data) >= 64 and data[:7] == b'\x7fELF\x02\x01\x01'
            and int.from_bytes(data[18:20], 'little') == 62
            and int.from_bytes(data[16:18], 'little') in (2, 3), 'An original x86_64 Linux ELF is required')


def frontend(root):
    directory = root / 'dist/web'
    require(directory.is_dir() and not directory.is_symlink(), 'The existing frontend build is required')
    paths = sorted(directory.rglob('*'))
    require(len(paths) <= 4096, 'Frontend member bound exceeded')
    result, total = [], 0
    for path in paths:
        require(not path.is_symlink(), 'Frontend links are not allowed')
        if path.is_dir():
            continue
        data = regular(path, MAX_METADATA)
        total += len(data)
        require(total <= MAX_BINARY, 'Frontend byte bound exceeded')
        result.append({'path': path.relative_to(root).as_posix(), 'bytes': len(data), 'sha256': digest(data)})
    require(any(item['path'] == 'dist/web/index.html' and item['bytes'] > 0 for item in result), 'Actual frontend index is missing')
    return result


def surface(root, identity, version):
    binary = root / 'artifacts/dzc-surface' / TARGET / 'dzc-surface'
    data = regular(binary, MAX_BINARY)
    elf(data)
    proof_data = regular(Path(str(binary) + '.proof.json'), MAX_JSON)
    proof = json_value(proof_data)
    expected = {'component_version': version, 'source_revision': identity['source_revision'],
                'source_state': 'verified_checkout', 'build_id': version + '@' + identity['source_revision'],
                'target': TARGET, 'toolkit_patch_id': PATCH}
    require(type(proof.get('schema_version')) is int and proof['schema_version'] == 1
            and proof.get('kind') == 'surface-build' and proof.get('expected') == expected
            and proof.get('sha256') == digest(data)
            and Path(proof['executable']).resolve() == binary.resolve()
            and proof.get('identity_verification') == 'executed_native_build_info'
            and proof.get('toolchain') == 'go version go1.27.3 linux/amd64', 'Existing Surface build proof differs')
    info = proof.get('build_info', {})
    require(all(info.get(name) == value for name, value in expected.items())
            and info.get('component') == 'surface' and info.get('mygo_version') == 'v0.6.4'
            and info.get('mygo_source_revision') == MYGO_SOURCE
            and info.get('protocol_version') == {'major': 1, 'minor': 0}
            and all(type(info['protocol_version'].get(name)) is int for name in ('major', 'minor')),
            'Existing Surface runtime identity differs')
    patch = proof.get('toolkit_patch', {})
    require(all(patch.get(name) == value for name, value in {
        'id': PATCH, 'strategy': 'isolated_module_copy', 'module': 'github.com/egoist/mygo',
        'version': 'v0.6.4', 'source_revision': MYGO_SOURCE, 'changed_files': ['app.go'],
        'module_sum': 'h1:VzCqZKgvXcym5cSYKoC6IkTgcVeGZquPiui/a3oE36o=',
        'module_go_mod_sum': 'h1:dVav53PwF2xEotjf4oA5lTY2Jp2exgzyjLhbSXhbMwE=',
        'upstream_app_sha256': 'af53a7f7c0dbe36d42edc6de34f9f7735569ae53170715264af6618b99e281b9'}.items()),
        'Existing Surface toolkit patch differs')
    inputs = proof.get('inputs')
    require(type(inputs) is list and 0 < len(inputs) <= 4096, 'Surface source input set is missing')
    seen = set()
    for item in inputs:
        path = Path(item['path'])
        require(not path.is_absolute() and '..' not in path.parts and path.as_posix() not in seen,
                'Surface source reference is unsafe or duplicate')
        seen.add(path.as_posix())
        require(digest(regular(root / path, MAX_METADATA)) == item['sha256'], 'Surface source bytes changed')
    require({'frontends/dzc-surface/go.mod', 'frontends/dzc-surface/go.sum', 'scripts/build-surface.mjs',
             'scripts/mygo-surface-patch.mjs', 'package.json', 'Cargo.toml', 'Cargo.lock'} <= seen,
            'Surface proof omits required source inputs')
    return data, proof_data


def compiler_artifact(data, root):
    require(data.endswith(b'\n') and len(data) <= MAX_METADATA, 'Complete bounded Cargo JSON is required')
    messages = [json_value(line) for line in data.splitlines() if line.strip()]
    finished = [item for item in messages if item.get('reason') == 'build-finished']
    require(len(finished) == 1 and finished[0].get('success') is True and messages[-1] == finished[0],
            'Cargo did not finish successfully')
    candidates = [item for item in messages if item.get('reason') == 'compiler-artifact'
                  and item.get('target', {}).get('name') == 'dezirclip'
                  and item.get('target', {}).get('kind') == ['bin']]
    require(len(candidates) == 1, 'Exactly one original Desktop compiler artifact is required')
    value = candidates[0]
    profile = value['profile']
    require(Path(value['manifest_path']).resolve() == (root / 'src-tauri/Cargo.toml').resolve()
            and Path(value['executable']).resolve() == (root / 'target/release/dezirclip').resolve()
            and value['target']['crate_types'] == ['bin'] and profile['opt_level'] == 's'
            and profile['debug_assertions'] is False and profile['test'] is False,
            'Cargo artifact is not the original release Desktop')
    return value


class BuildLog:
    def __init__(self, output):
        self.files = {}
        self.bytes, self.first_error = 0, None
        try:
            for name in ('stdout', 'stderr'):
                self.files[name] = (output / ('cargo-' + name + '.log')).open('xb')
        except BaseException:
            self.close()
            raise

    def receive(self, name, data):
        if self.first_error is not None:
            return
        remaining = MAX_METADATA - self.bytes
        try:
            require(self.files[name].write(data[:remaining]) == min(len(data), remaining), 'Short Cargo metadata write')
            self.bytes += min(len(data), remaining)
            require(len(data) <= remaining, 'Combined raw Cargo metadata exceeded 32 MiB')
        except BaseException as error:
            self.first_error = str(error) or type(error).__name__

    def close(self):
        for stream in self.files.values():
            try:
                stream.flush()
                os.fsync(stream.fileno())
            except BaseException as error:
                if self.first_error is None:
                    self.first_error = str(error) or type(error).__name__
            finally:
                try:
                    stream.close()
                except BaseException as error:
                    if self.first_error is None:
                        self.first_error = str(error) or type(error).__name__


def build(root, output):
    logs, process, selector = BuildLog(output), None, None
    first_error, code, ended = None, None, set()
    def remember(error):
        nonlocal first_error
        if first_error is None:
            first_error = str(error) or type(error).__name__
    try:
        started = time.monotonic()
        selector = selectors.DefaultSelector()
        process = subprocess.Popen(COMMAND, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        for name in ('stdout', 'stderr'):
            selector.register(getattr(process, name), selectors.EVENT_READ, name)
        while selector.get_map():
            require(time.monotonic() - started < BUILD_TIMEOUT_SECONDS, 'Release build metadata deadline exceeded')
            for key, event in selector.select(timeout=.25):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if chunk:
                    logs.receive(key.data, chunk)
                    require(logs.first_error is None, logs.first_error)
                else:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    ended.add(key.data)
        require(ended == {'stdout', 'stderr'}, 'Both original build streams must reach EOF')
        code = process.wait(timeout=BUILD_WAIT_SECONDS)
        require(code == 0, 'Original release Cargo command failed: exit=' + str(code))
    except BaseException as error:
        remember(error)
    finally:
        if process is not None:
            running = True
            try:
                running = process.poll() is None
            except BaseException as error:
                remember(error)
            if running:
                try:
                    process.kill()
                except BaseException as error:
                    remember(error)
                try:
                    process.wait(timeout=BUILD_WAIT_SECONDS)
                except BaseException as error:
                    remember(error)
            for name in ('stdout', 'stderr'):
                try:
                    stream = getattr(process, name)
                    if stream is not None:
                        stream.close()
                except BaseException as error:
                    remember(error)
        if selector is not None:
            try:
                selector.close()
            except BaseException as error:
                remember(error)
        try:
            logs.close()
        except BaseException as error:
            remember(error)
    require(first_error is None and logs.first_error is None, first_error or logs.first_error)
    return {'compiler_artifact': compiler_artifact(regular(output / 'cargo-stdout.log', MAX_METADATA), root),
            'exit_code': code, 'streams_complete': {name: name in ended for name in ('stdout', 'stderr')},
            'first_error': None, 'combined_log_bytes': logs.bytes,
            'timeout_seconds': BUILD_TIMEOUT_SECONDS, 'wait_seconds': BUILD_WAIT_SECONDS}


def produce():
    root, environment = ROOT, os.environ
    temporary, identity = authorize(root, environment, sys.platform)
    output = temporary / ('dezirclip-linux-release-inputs-' + identity['run_id'] + '-' + identity['run_attempt'])
    require(not output.exists(), 'A fresh release-input output directory is required')
    output.mkdir()
    try:
        package = json_value(regular(root / 'package.json', MAX_JSON))
        version = package['version']
        release = tomllib.loads(regular(root / 'Cargo.toml', MAX_JSON).decode('utf-8'))['profile']['release']
        require(all(type(release.get(name)) is type(value) and release[name] == value
                    for name, value in RELEASE_PROFILE.items()), 'Original release profile differs')
        config = json_value(regular(root / 'src-tauri/tauri.conf.json', MAX_JSON))
        require(config['build']['frontendDist'] == '../dist/web', 'Original embedded frontend path differs')
        web = frontend(root)
        helper, helper_proof = surface(root, identity, version)
        source_inputs = {name: digest(regular(root / name, MAX_JSON))
                         for name in ('Cargo.toml', 'Cargo.lock', 'package.json', 'src-tauri/tauri.conf.json')}
        tools = {}
        for label, command in [('rustc', ['rustc', '-vV']), ('cargo-version', ['cargo', '--version'])]:
            stdout, stderr = small(command, root)
            tools[label] = {'command': command, 'stdout': save(output / (label + '.stdout.log'), stdout),
                            'stderr': save(output / (label + '.stderr.log'), stderr)}
            if label == 'rustc':
                require(re.search(r'(?m)^host: ' + re.escape(TARGET) + r'$', stdout.decode('utf-8')), 'Native Linux x64 Rust host required')
        build_result = build(root, output)
        require(type(build_result['exit_code']) is int and build_result['exit_code'] == 0
                and build_result['streams_complete'] == {'stdout': True, 'stderr': True}
                and all(type(value) is bool for value in build_result['streams_complete'].values())
                and build_result['first_error'] is None, 'Original build did not close both streams successfully')
        artifact = build_result['compiler_artifact']
        raw_logs = {name: regular(output / ('cargo-' + name + '.log'), MAX_METADATA) for name in ('stdout', 'stderr')}
        require(sum(map(len, raw_logs.values())) == build_result['combined_log_bytes'] <= MAX_METADATA
                and compiler_artifact(raw_logs['stdout'], root) == artifact, 'Original build metadata differs')
        checkout(root, environment)
        require(frontend(root) == web and surface(root, identity, version) == (helper, helper_proof),
                'Existing frontend or Surface inputs changed during release compilation')
        require(source_inputs == {name: digest(regular(root / name, MAX_JSON)) for name in source_inputs},
                'Release source inputs changed during compilation')
        desktop = regular(root / 'target/release/dezirclip', MAX_BINARY)
        elf(desktop)
        binary = save(output / 'dezirclip', desktop)
        helper_record = save(output / 'dzc-surface', helper)
        helper_record['proof'] = save(output / 'dzc-surface.proof.json', helper_proof)
        proof = {**identity, 'schema_version': 1, 'kind': 'gha-linux-release-input-build',
                 'producer': 'existing-ci-tauri-linux-x64-opt-in', 'profile': 'release', 'binary': binary,
                 'runtime_executed_by_producer': False, 'identity_verification': 'cargo_compiler_artifact',
                 'namespace_offline_builder_receipt': False}
        binary['proof'] = document(output / 'dezirclip.proof.json', proof)
        metadata = document(output / 'build-metadata.json', {**identity, 'command': COMMAND, 'build': build_result,
            'toolchain': tools, 'frontend': web, 'release_profile': release,
            'source_inputs': source_inputs,
            'raw_cargo_logs': {name: {'bytes': len(data), 'sha256': digest(data)} for name, data in raw_logs.items()}})
        checksums = ''.join(digest(regular(path, MAX_BINARY)) + '  ' + path.name + '\n'
                            for path in sorted(output.iterdir()) if path.is_file())
        checksum_record = save(output / 'SHA256SUMS', checksums.encode())
        pending = output / 'inputs.pending.json'
        document(pending, {**identity, 'status': 'complete_input_pair', 'profile': 'release',
                 'desktop': binary, 'surface': helper_record, 'build_metadata': metadata, 'checksums': checksum_record,
                 'checksum_scope': 'All payload/proof/metadata files; inputs.json and SHA256SUMS are excluded.',
                 'functional_acceptance': None, 'idle_acceptance': None, 'quiet_window': None,
                 'performance_acceptance': None, 'phase6_pass': False})
        require(not (output / 'inputs.json').exists(), 'Final input marker already exists')
        os.rename(pending, output / 'inputs.json')
        return output
    except BaseException as error:
        try:
            document(output / 'failure.json', {**identity, 'status': 'failed', 'complete_input_pair': False,
                'first_error': str(error)[:8192] or type(error).__name__,
                'first_error_truncated': len(str(error)) > 8192, 'phase6_pass': False})
        except BaseException as receipt_error:
            print('Release input producer first error: ' + (str(error)[:8192] or type(error).__name__)
                  + '; failure receipt error: ' + str(receipt_error)[:8192], file=sys.stderr, flush=True)
        raise


if __name__ == '__main__':
    produce()
