"""Collect actual identities from final release bytes on matching disposable runners."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import threading

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {'linux-x86_64': 'x86_64-unknown-linux-gnu', 'linux-aarch64': 'aarch64-unknown-linux-gnu',
           'windows-x86_64': 'x86_64-pc-windows-msvc', 'windows-aarch64': 'aarch64-pc-windows-msvc'}
ROLES = {'desktop': 'dezirclip', 'cli': 'dzc', 'service': 'dzc-service'}
VERSION = '0.2.0-alpha.2'
FIELDS = {'metadata_version', 'component', 'component_version', 'build_id', 'source_revision', 'source_state',
          'revision_origin', 'target', 'protocol_version', 'database_schema_version', 'linked_components'}

def require(ok, message):
    if not ok:
        raise ValueError(message)

def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()

def validate_info(info, role, target, revision):
    require(isinstance(info, dict) and set(info) == FIELDS, 'BuildInfo fields differ')
    require(info['component'] == role and info['component_version'] == VERSION, 'BuildInfo role/version differs')
    require(info['source_revision'] == revision and info['build_id'] == VERSION + '@' + revision, 'BuildInfo source differs')
    require(info['target'] == TARGETS[target] and info['source_state'] == 'verified_checkout'
            and info['revision_origin'] in ('explicit_checkout', 'local_git'), 'BuildInfo target/provenance differs')
    require(type(info['metadata_version']) is int and info['metadata_version'] == 1
            and type(info['database_schema_version']) is int and info['database_schema_version'] == 24, 'BuildInfo schema differs')
    require(info['protocol_version'] == {'major': 1, 'minor': 0}
            and all(type(v) is int for v in info['protocol_version'].values()), 'BuildInfo protocol differs')
    names = ['dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host'] + (['dzc-service'] if role == 'service' else [])
    require(isinstance(info['linked_components'], list) and len(info['linked_components']) == len(names), 'BuildInfo inventory count differs')
    require(all(isinstance(item, dict) and set(item) == {'component', 'version'} and item['version'] == VERSION
                for item in info['linked_components']), 'BuildInfo linked version differs')
    require(sorted(item['component'] for item in info['linked_components']) == sorted(names), 'BuildInfo inventory differs')
    return info

def expected_assets(target):
    extension = 'zip' if target.startswith('windows-') else 'tar.gz'
    standalone = f'dzc-{VERSION}-{target}.{extension}'
    if target.startswith('windows-'):
        arch = 'arm64' if target.endswith('aarch64') else 'x64'
        return standalone, [f'DezirClip_{VERSION}_{arch}-setup.exe']
    arm = target.endswith('aarch64')
    return standalone, [f'DezirClip_{VERSION}_{"aarch64" if arm else "amd64"}.AppImage',
                        f'DezirClip_{VERSION}_{"arm64" if arm else "amd64"}.deb',
                        f'DezirClip-0.2.0-0.alpha.2.{"aarch64" if arm else "x86_64"}.rpm',
                        f'DezirClip-{VERSION}-{target}.flatpak']

def native_target(target, machine=None, system=None):
    machine = (machine or platform.machine()).lower()
    system = (system or platform.system()).lower()
    arch = {'amd64': 'x86_64', 'x86_64': 'x86_64', 'arm64': 'aarch64', 'aarch64': 'aarch64'}.get(machine)
    require(target == f'{system}-{arch}', 'Receipt execution requires the actual matching native runner')
    if system == 'windows':
        require(os.environ.get('PROCESSOR_ARCHITEW6432', '').upper() not in ('ARM64', 'AMD64'), 'Emulated Python is not native receipt execution')

def inspect_machine(path, target):
    with Path(path).open('rb') as source:
        head = source.read(64)
        if target.startswith('windows-'):
            require(head[:2] == b'MZ', 'Expected actual PE executable')
            offset = struct.unpack_from('<I', head, 60)[0]
            require(64 <= offset <= Path(path).stat().st_size - 26, 'Invalid PE offset')
            source.seek(offset)
            pe = source.read(26)
            require(pe[:4] == b'PE\0\0' and struct.unpack_from('<H', pe, 24)[0] == 0x20b, 'Expected PE64 executable')
            actual = struct.unpack_from('<H', pe, 4)[0]
            wanted = 0xaa64 if target.endswith('aarch64') else 0x8664
        else:
            require(head[:7] == b'\x7fELF\x02\x01\x01', 'Expected actual ELF64 executable')
            actual = struct.unpack_from('<H', head, 18)[0]
            wanted = 0xb7 if target.endswith('aarch64') else 0x3e
        require(actual == wanted, 'Packaged machine differs from native target')
    return actual

def safe_member(name):
    require(isinstance(name, str) and name and '\\' not in name and '\0' not in name
            and not name.startswith('/') and ':' not in name and all(part not in ('', '.', '..') for part in name.split('/')), 'Unsafe archive member')
    return name

def run(command, directory, label, env=None, timeout=180):
    directory.mkdir(parents=True, exist_ok=True)
    overflow=threading.Event()
    with (directory / (label + '.stdout.log')).open('xb') as out, (directory / (label + '.stderr.log')).open('xb') as err:
        child=subprocess.Popen([str(v) for v in command],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
        def pump(source,destination):
            total=0
            try:
                while True:
                    block=source.read(64*1024)
                    if not block:break
                    allowed=min(len(block),max(0,8*1024*1024-total))
                    destination.write(block[:allowed]);total+=len(block)
                    if total>8*1024*1024:
                        overflow.set();child.kill();break
            finally:source.close()
        threads=[threading.Thread(target=pump,args=(child.stdout,out),daemon=True),threading.Thread(target=pump,args=(child.stderr,err),daemon=True)]
        for thread in threads:thread.start()
        expired=False
        try:child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            expired=True;child.kill();child.wait(timeout=5)
        for thread in threads:thread.join(timeout=5)
        require(not any(thread.is_alive() for thread in threads),label+' pipe-drain deadline')
    require(not expired,label+' execution deadline; retained original logs')
    require(not overflow.is_set(),label+' output exceeds bound; retained bounded original logs')
    require(child.returncode==0,label+' failed; retained original logs')
    return (directory / (label + '.stdout.log')).read_text(encoding='utf-8-sig')

def find_members(directory, target):
    extension = '.exe' if target.startswith('windows-') else ''
    result = {}
    for role, name in ROLES.items():
        paths = [p for p in directory.rglob('*') if p.name.lower() == name + extension and p.is_file()]
        require(len(paths) == 1, f'Expected exactly one final packaged {role}')
        path = paths[0]
        require(not path.is_symlink() and path.resolve().is_relative_to(directory.resolve()), 'Packaged executable escapes extraction root')
        inspect_machine(path, target)
        result[role] = path
    return result

def source_boundary(target, revision, out):
    require(os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'Final identities require disposable Actions runners')
    require(os.environ.get('GITHUB_REPOSITORY') == 'qiyuanhuakai/dezirclip', 'Only the original receipt repository is supported')
    require(re.fullmatch('[0-9a-f]{40}', revision), 'Exact final source required')
    from release_recovery import authority
    provenance = authority(revision, ROOT)
    require(provenance['mode'] in ('tag', 'recovery') and provenance['source_revision'] == revision and provenance['tag'] == 'v' + VERSION,
            'Receipt release authority differs')
    if provenance['mode'] == 'tag':
        require(os.environ.get('GITHUB_REF') == 'refs/tags/v' + VERSION
                and os.environ.get('GITHUB_SHA') == revision
                and subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip() == revision,
                'Exact final tag checkout required')
    require(not subprocess.check_output(['git', 'status', '--porcelain=v1', '--untracked-files=no'], cwd=ROOT, text=True).strip(),
            'Receipt helper checkout differs')
    native_target(target)
    boundary = Path(os.environ['RUNNER_TEMP']).resolve()
    require(out.resolve().is_relative_to(boundary) and out.name.startswith('dezirclip-release-identities-') and not out.exists(), 'New owned receipt output required')
    out.mkdir(mode=0o700)
    return provenance

def json_output(text):
    require(len(text.encode()) <= 64 * 1024, 'BuildInfo exceeds bound')
    return json.loads(text)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS, required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--job-id', type=int, required=True)
    parser.add_argument('--preseed-helper', type=Path)
    args = parser.parse_args()
    provenance = source_boundary(args.target, args.source, args.out)
    require(args.job_id > 0, 'Actual Actions job id required')
    report = {'schema_version': 1, 'status': 'running', 'tag': 'v' + VERSION, 'source_revision': args.source,
              'workflow_source_revision': provenance['workflow_source_revision'], 'source_authority': provenance,
              'target': args.target, 'runner': {'system': platform.system(), 'machine': platform.machine()},
              'run_url': f'https://github.com/qiyuanhuakai/dezirclip/actions/runs/{os.environ["GITHUB_RUN_ID"]}',
              'job_id': args.job_id, 'method': 'executed_native', 'standalone': [], 'containers': []}
    try:
        from release_identity_packages import collect
        collect(args, report)
        report['status'] = 'passed'
    except BaseException as error:
        report.update(status='failed',error=type(error).__name__ + ': ' + str(error))
        raise
    finally:
        (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        summary = {k: report[k] for k in ('schema_version', 'status', 'source_revision', 'workflow_source_revision', 'target', 'run_url', 'job_id')}
        summary.update(report_sha256=sha(args.out / 'report.json'), artifacts_path=str(args.out))
        print('DEZIRCLIP_RELEASE_IDENTITIES ' + json.dumps(summary), flush=True)
        bounded=json.dumps(report,separators=(',',':'))
        require(len(bounded.encode())<=256*1024,'Identity report exceeds diagnostic bound')
        print('DEZIRCLIP_RELEASE_IDENTITIES_REPORT '+bounded,flush=True)

if __name__ == '__main__':
    main()
