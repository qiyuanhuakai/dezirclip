"""Extract final packages and execute their metadata on matching disposable runners."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import struct
from release_identities import ROOT, expected_assets, find_members, inspect_machine, json_output, require, run, safe_member, sha, validate_info

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
                safe_member(fields['Path'].replace('\\','/'))
                require('Symbolic Link' not in fields and 'Hard Link' not in fields,'NSIS links unsupported')
        run(['7z','x','-y','-sccUTF-8','-o'+str(root),archive],logs,'extract')
    elif archive.name.endswith('.AppImage'):
        run(['unsquashfs','-no-progress','-no-xattrs','-offset',str(squashfs_offset(archive)),'-dest',root,archive],logs,'extract')
    elif archive.name.endswith('.deb'):
        run(['dpkg-deb','--extract',archive,root],logs,'extract')
    elif archive.name.endswith('.rpm'):
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

def windows_environment(logs):
    allowed={'SYSTEMROOT','WINDIR','COMSPEC','PATH','PATHEXT','PROGRAMFILES','PROGRAMFILES(X86)','PROGRAMW6432','PROCESSOR_ARCHITECTURE','NUMBER_OF_PROCESSORS'}
    env={key:value for key,value in os.environ.items() if key.upper() in allowed};profile=logs/'profile'
    for name,relative in [('USERPROFILE','home'),('HOME','home'),('APPDATA','roaming'),('LOCALAPPDATA','local'),('TEMP','temp'),('TMP','temp')]:
        path=profile/relative;path.mkdir(parents=True,exist_ok=True);env[name]=str(path)
    database=profile/'roaming/io.github.qiyuanhuakai.dezirclip/clipboard.db';database.parent.mkdir(parents=True,exist_ok=True)
    env['DEZIRCLIP_DB_PATH']=str(database);env['WEBVIEW2_USER_DATA_FOLDER']=str(profile/'webview')
    env.update(GITHUB_ACTIONS='true',RUNNER_ENVIRONMENT='github-hosted',RUNNER_TEMP=os.environ['RUNNER_TEMP'])
    return env

def windows_info(members,logs,target,revision):
    env=windows_environment(logs)
    result={}
    for role,arguments in [('cli',['build-info','--json']),('service',['--build-info'])]:
        result[role]=validate_info(json_output(run([members[role],*arguments],logs,role,env)),role,target,revision)
    run([members['cli'],'count','--json'],logs,'empty-database',env)
    database=Path(env['DEZIRCLIP_DB_PATH']);require(database.is_file(),'Synthetic receipt profile missing')
    with sqlite3.connect(database) as connection:
        require(connection.execute('SELECT COUNT(*) FROM clipboard_history').fetchone()==(0,),'Receipt profile must be empty')
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',{'app.silent_start':'false','app.idle_destroy_enabled':'false','app.update_auto_check':'false','app.ocr_enabled':'false','app.classification_enabled':'false','app.sequential_mode':'false'}.items())
    raw=run(['node',ROOT/'scripts/release-identity-desktop.mjs',members['desktop'],logs/'desktop-profile'],logs,'desktop',env,timeout=120)
    result['desktop']=validate_info(json_output(raw),'desktop',target,revision);return result

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
                info=validate_info(json_output(run([path,'build-info','--json'],logs,'cli',windows_environment(logs))),'cli',args.target,args.source)
            else:
                from release_identity_linux import sandbox_info
                info=sandbox_info(args,{'cli':path},root,logs,False)['cli']
            require(sha(path)==binary_digest,'Standalone executable changed during metadata execution')
            report['standalone'].append({'platform':args.target,'asset':name,'asset_sha256':digest,'binary_sha256':binary_digest,'build_info':info})
        else:
            unpack(archive,root,logs,args.target);members=find_members(root,args.target);infos=native_info(args,members,root,logs,name.endswith('.flatpak'))
            report['containers'].append({'platform':args.target,'asset':name,'asset_sha256':digest,'contents':[{'component':role,'path':path.relative_to(root).as_posix(),'bytes':path.stat().st_size,'binary_sha256':sha(path),'build_info':infos[role]} for role,path in members.items()]})
        require(sha(archive)==digest,'Final release asset changed during collection')
        remove_owned_tree(root,args.out)
        if (logs/'ostree').exists():remove_owned_tree(logs/'ostree',args.out)
        (args.out/'partial-report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
