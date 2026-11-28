"""Inspect final desktop packages and run historical checks on matching disposable runners."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import struct
import sys
import tempfile
from release_identities import ROOT, ROLES, TARGETS, VERSION, expected_assets, find_members, inspect_machine, json_output, require, run, safe_member, sha, validate_info
from release_version import rpm_parts

def standalone(archive, root):
    spec=importlib.util.spec_from_file_location('release_archive',ROOT/'scripts/verify-dzc-package.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    module.verify_checksum(archive);module.extract_archive(archive,root)

def check_tree(root):
    for path in root.rglob('*'):
        require(path.resolve().is_relative_to(root.resolve()),'Extracted package link escapes root')

def remove_owned_tree(path,parent):
    require(not path.is_symlink() and path.resolve(strict=True).is_relative_to(parent.resolve(strict=True))
            and path.resolve()!=parent.resolve(),'Refuse cleanup outside the owned receipt directory')
    shutil.rmtree(path)

def squashfs_offset(path):
    with path.open('rb') as source:data=source.read(16*1024*1024)
    found=[];start=0
    while True:
        start=data.find(b'hsqs',start)
        if start<0:break
        if start+96<=len(data):
            major,minor=struct.unpack_from('<HH',data,start+28);used=struct.unpack_from('<Q',data,start+40)[0]
            if (major,minor)==(4,0) and 96<=used<=path.stat().st_size-start:found.append(start)
        start+=4
    require(len(found)==1,'Expected one bounded AppImage SquashFS superblock');return found[0]

def unpack(archive,root,logs,target):
    root.mkdir()
    if archive.name.endswith('.exe'):
        listing=run(['7z','l','-slt','-sccUTF-8',archive],logs,'list').replace('\r\n','\n').split('\n----------\n')
        require(len(listing)==2,'Unrecognized NSIS listing')
        for block in listing[1].split('\n\n'):
            fields=dict(line.split(' = ',1) for line in block.splitlines() if ' = ' in line)
            if 'Path' in fields:
                safe_member(fields['Path'].replace('\\','/').removeprefix('./'))
                require('Symbolic Link' not in fields and 'Hard Link' not in fields,'NSIS links unsupported')
        run(['7z','x','-y','-sccUTF-8','-o'+str(root),archive],logs,'extract')
    elif archive.name.endswith('.AppImage'):
        run(['unsquashfs','-no-progress','-no-xattrs','-offset',str(squashfs_offset(archive)),'-dest',root,archive],logs,'extract')
    elif archive.name.endswith('.deb'):
        run(['dpkg-deb','--extract',archive,root],logs,'extract')
    elif archive.name.endswith(('.rpm', '.pkg.tar.zst', '.src.tar.gz')):
        listing=run(['bsdtar','-tf',archive],logs,'list')
        for name in listing.splitlines():safe_member(name.removeprefix('./').rstrip('/'))
        run(['bsdtar','--no-same-owner','--no-same-permissions','-xf',archive,'-C',root],logs,'extract')
    elif archive.name.endswith('.flatpak'):
        repository=logs/'ostree'
        run(['ostree','--repo='+str(repository),'init','--mode=archive-z2'],logs,'ostree-init')
        run(['flatpak','build-import-bundle',repository,archive],logs,'flatpak-import')
        refs=run(['ostree','--repo='+str(repository),'refs'],logs,'flatpak-refs').splitlines()
        arch='aarch64' if target.endswith('aarch64') else 'x86_64'
        refs=[ref for ref in refs if ref.startswith('app/io.github.qiyuanhuakai.dezirclip/'+arch+'/')]
        require(len(refs)==1,'Expected one matching exported Flatpak ref')
        root.rmdir()
        run(['ostree','--repo='+str(repository),'checkout','--user-mode',refs[0],root],logs,'flatpak-checkout')
    else:raise ValueError('Unsupported final package')
    check_tree(root)

def package_fields(text, separator):
    fields = {}
    for line in text.splitlines():
        key, delimiter, value = line.partition(separator)
        if delimiter:
            fields[key.strip()] = value.strip()
    return fields


def desktop_package_files(root, target, mygo):
    roles = {**ROLES, 'desktop': 'dzc-desktop'} if mygo else ROLES
    members = find_members(root, target, roles)
    if target.startswith('windows-'):
        directory, extension = root, '.exe'
    else:
        directory = root / ('usr/lib/DezirClip-MyGo' if mygo else 'usr/bin')
        extension = ''
        launcher = root / ('usr/bin/dezirclip-mygo' if mygo else 'usr/bin/dezirclip')
        desktop_file = root / 'usr/share/applications' / ('DezirClip-MyGo.desktop' if mygo else 'DezirClip.desktop')
        require(launcher.is_file() and os.access(launcher, os.X_OK) and desktop_file.is_file(), 'Missing Linux desktop launcher')
    result = {}
    for role, path in members.items():
        require(path == directory / (roles[role] + extension), 'Unexpected installed path for ' + role)
        require(extension or os.access(path, os.X_OK), 'Missing executable permission for ' + role)
        result[role] = {'path': path.relative_to(root).as_posix(), 'sha256': sha(path)}
    return result


def verify_arch_recipe(root, name, version, deb, digest):
    paths = [path.relative_to(root).as_posix() for path in root.rglob('*') if not path.is_dir()]
    require(sorted(paths) == [name+'/.SRCINFO', name+'/PKGBUILD'], 'Unexpected AUR recipe contents')
    recipe, srcinfo = (root / name / file for file in ('PKGBUILD', '.SRCINFO'))
    require(all(path.is_file() and not path.is_symlink() and path.stat().st_size < 16*1024
                for path in (recipe, srcinfo)), 'Invalid AUR recipe files')
    source = f'https://github.com/qiyuanhuakai/dezirclip/releases/download/v{version}/{deb}'
    fields = package_fields(srcinfo.read_text(encoding='utf-8'), '=')
    require(fields.get('source') == source and fields.get('sha256sums') == digest
            and fields.get('pkgname') == name and fields.get('arch') == 'x86_64', 'AUR metadata differs from the staged Debian package')
    fields = package_fields(recipe.read_text(encoding='utf-8'), '=')
    require(fields.get('source') == f"('{source}')" and fields.get('sha256sums') == f"('{digest}')",
            'AUR build recipe refers to different source bytes')


def verify_candidate_packages(version, assets):
    rpm_version, rpm_release = rpm_parts(version)
    debs = {}
    verified = []
    # Verify Debian first, since the Arch package and recipe derive from those files.
    for asset in sorted(assets, key=lambda entry: not entry['name'].endswith('.deb')):
        name, target, archive = asset['name'], asset['target'], Path(asset['path'])
        require(target in TARGETS and name == archive.name and archive.is_file() and not archive.is_symlink()
                and sha(archive) == asset['sha256'], 'Candidate archive differs: ' + name)
        mygo = name.startswith(('DezirClip-MyGo', 'dezirclip-mygo-bin-')) or name.endswith('-MyGo-setup.exe')
        deb_name = 'dezir-clip-my-go' if mygo else 'dezir-clip'
        arch_name = 'dezirclip-mygo-bin' if mygo else 'dezirclip-bin'
        with tempfile.TemporaryDirectory(prefix='dezirclip-package-check-') as directory:
            logs = Path(directory)
            root = logs / 'root'
            try:
                unpack(archive, root, logs, target)
                members = {}
                if name.endswith('.src.tar.gz'):
                    deb = debs[(target, mygo)]
                    verify_arch_recipe(root, arch_name, version, deb['name'], deb['sha256'])
                else:
                    members = desktop_package_files(root, target, mygo)
                if name.endswith('.AppImage'):
                    inspect_machine(archive, target)
                elif name.endswith('.deb'):
                    fields = package_fields(run(['dpkg-deb', '-f', archive, 'Package', 'Version', 'Architecture'],
                                                logs, 'package-metadata'), ':')
                    require(fields == {'Package': deb_name, 'Version': version,
                                       'Architecture': 'arm64' if target.endswith('aarch64') else 'amd64'},
                            'Debian package metadata differs')
                    debs[(target, mygo)] = {**asset, 'members': members}
                elif name.endswith('.rpm'):
                    metadata = run(['rpm', '-qp', '--qf', '%{NAME} %{VERSION} %{RELEASE} %{ARCH}', archive],
                                   logs, 'package-metadata').split()
                    require(metadata == [deb_name, rpm_version, rpm_release, target.split('-', 1)[1]], 'RPM package metadata differs')
                elif name.endswith('.pkg.tar.zst'):
                    fields = package_fields((root / '.PKGINFO').read_text(encoding='utf-8'), '=')
                    require(fields.get('pkgname') == arch_name and fields.get('arch') == 'x86_64'
                            and name == f"{arch_name}-{fields.get('pkgver')}-x86_64.pkg.tar.zst", 'Arch package metadata differs')
                    require(members == debs[(target, mygo)]['members'], 'Arch executable payload differs from Debian')
            except (OSError, ValueError, KeyError) as error:
                raise ValueError(f'{name}: {error}') from error
        verified.append({'name': name, 'target': target, 'sha256': asset['sha256'], 'members': members})
    return verified


def windows_environment(logs):
    allowed={'SYSTEMROOT','WINDIR','COMSPEC','PATH','PATHEXT','PROGRAMFILES','PROGRAMFILES(X86)','PROGRAMW6432','PROCESSOR_ARCHITECTURE','NUMBER_OF_PROCESSORS'}
    env={key:value for key,value in os.environ.items() if key.upper() in allowed};profile=logs/'profile'
    from release_identity_windows_paths import profile_layout
    for name,path in profile_layout(profile).items():
        path.mkdir(mode=0o700,parents=True,exist_ok=True)
        if name!='LOCALLOW':env[name]=str(path)
    for name in ('GITHUB_ACTIONS','RUNNER_ENVIRONMENT','RUNNER_TEMP','GITHUB_REPOSITORY','GITHUB_REF','GITHUB_REF_NAME',
                 'GITHUB_SHA','GITHUB_EVENT_NAME','GITHUB_WORKFLOW_REF','GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT',
                 'DZC_RELEASE_RECOVERY','DZC_RELEASE_TOOLING_SOURCE'):
        if name in os.environ:env[name]=os.environ[name]
    database=Path(env['APPDATA'])/'io.github.qiyuanhuakai.dezirclip/clipboard.db';database.parent.mkdir(parents=True,exist_ok=True)
    env['DEZIRCLIP_DB_PATH']=str(database);env['WEBVIEW2_USER_DATA_FOLDER']=str(profile/'webview')
    return env

def windows_preflight(logs,revision):
    env=windows_environment(logs)
    run([sys.executable,ROOT/'scripts/release_identity_windows_paths.py','--source',revision,'--profile',logs/'profile',
         '--output',logs.parent/('windows-known-folders-'+logs.name+'.json')],logs,'known-folders',env,timeout=60)
    return env

def windows_info(members,logs,target,revision):
    env=windows_preflight(logs,revision)
    result={}
    for role,arguments in [('cli',['build-info','--json']),('service',['--build-info'])]:
        result[role]=validate_info(json_output(run([members[role],*arguments],logs,role,env)),role,target,revision,version=VERSION)
    run([members['cli'],'count','--json'],logs,'empty-database',env)
    database=Path(env['DEZIRCLIP_DB_PATH']);require(database.is_file(),'Synthetic receipt profile missing')
    with sqlite3.connect(database) as connection:
        require(connection.execute('SELECT COUNT(*) FROM clipboard_history').fetchone()==(0,),'Receipt profile must be empty')
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',{'app.silent_start':'false','app.idle_destroy_enabled':'false','app.update_auto_check':'false','app.ocr_enabled':'false','app.classification_enabled':'false','app.sequential_mode':'false'}.items())
    raw=run(['node',ROOT/'scripts/release-identity-desktop.mjs',members['desktop'],logs/'desktop-profile',sys.executable,revision],logs,'desktop',env,timeout=180)
    result['desktop']=validate_info(json_output(raw),'desktop',target,revision,version=VERSION);return result

def native_info(args,members,extracted,logs,flatpak=False):
    before={role:sha(path) for role,path in members.items()}
    if args.target.startswith('windows-'):result=windows_info(members,logs,args.target,args.source)
    else:
        from release_identity_linux import sandbox_info
        result=sandbox_info(args,members,extracted,logs,flatpak)
    require(all(sha(members[role])==digest for role,digest in before.items()),'Packaged executable changed during collection');return result

def collect(args,report):
    archive_name,package_names=expected_assets(args.target)
    require(args.assets.is_dir() and not args.assets.is_symlink(),'Downloaded final assets required')
    for index,name in enumerate([archive_name,*package_names]):
        archive=args.assets/name;require(archive.is_file() and not archive.is_symlink(),'Missing final asset: '+name);digest=sha(archive)
        logs=args.out/f'asset-{index}';logs.mkdir();root=logs/'extracted'
        if index==0:
            standalone(archive,root);path=root/('dzc.exe' if args.target.startswith('windows-') else 'dzc');inspect_machine(path,args.target)
            binary_digest=sha(path)
            if args.target.startswith('windows-'):
                info=validate_info(json_output(run([path,'build-info','--json'],logs,'cli',windows_preflight(logs,args.source))),'cli',args.target,args.source,version=VERSION)
            else:
                from release_identity_linux import sandbox_info
                info=sandbox_info(args,{'cli':path},root,logs,False)['cli']
            require(sha(path)==binary_digest,'Standalone executable changed during metadata execution')
            report['standalone'].append({'platform':args.target,'asset':name,'asset_sha256':digest,'path':path.relative_to(root).as_posix(),'bytes':path.stat().st_size,'binary_sha256':binary_digest,'build_info':info})
        else:
            unpack(archive,root,logs,args.target);members=find_members(root,args.target);infos=native_info(args,members,root,logs,name.endswith('.flatpak'))
            report['containers'].append({'platform':args.target,'asset':name,'asset_sha256':digest,'contents':[{'component':role,'path':path.relative_to(root).as_posix(),'bytes':path.stat().st_size,'binary_sha256':sha(path),'build_info':infos[role]} for role,path in members.items()]})
        require(sha(archive)==digest,'Final release asset changed during collection')
        remove_owned_tree(root,args.out)
        if (logs/'ostree').exists():remove_owned_tree(logs/'ostree',args.out)
        (args.out/'partial-report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--candidate-version', required=True)
    args = parser.parse_args()
    print(json.dumps(verify_candidate_packages(args.candidate_version, json.load(sys.stdin))))
