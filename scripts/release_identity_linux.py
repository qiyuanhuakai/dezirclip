"""Execute only receipt metadata inside private Linux namespaces and Secret Service."""
import json
import os
from pathlib import Path
import shlex
import shutil
import sqlite3
import sys
from release_identities import ROOT, json_output, require, run, sha, validate_info

def flatpak_bindings(extracted,logs):
    require(extracted.resolve().is_relative_to(logs.resolve()),'Owned Flatpak extraction required')
    mountpoint=extracted/'var'
    require(not mountpoint.is_symlink(),'Flatpak var mountpoint must not be a link')
    mountpoint.mkdir(mode=0o700,exist_ok=True)
    variable=logs/'flatpak-var';variable.mkdir(mode=0o700)
    for name in ('tmp','lib'):(variable/name).mkdir(mode=0o700)
    bindings=['--bind',str(variable),'/package/var','--dir','/sys']
    for name in ('block','bus','class','dev','devices'):bindings+=['--dir','/sys/'+name]
    return bindings

def sandbox_info(args,members,extracted,logs,flatpak):
    import release_identity_guard
    guard=release_identity_guard.install()
    require(args.preseed_helper and args.preseed_helper.is_file(),'Explicit native QA preseed helper required')
    source=logs/'sources';source.mkdir()
    for name in ('release_identities.py','release_identity_linux.py','release_identity_guard.py','release_identity_secret_service.py'):
        shutil.copy2(ROOT/'scripts'/name,source/name)
    shutil.copy2(ROOT/'scripts/performance/native_capture.py',source/'native_capture.py')
    shutil.copy2(args.preseed_helper,source/'ss-preseed');(source/'ss-preseed').chmod(0o700)
    plan={'source_revision':args.source,'target':args.target,'flatpak':flatpak,'members':{role:'/package/'+path.relative_to(extracted).as_posix() for role,path in members.items()}}
    if (extracted/'AppRun').is_file():plan['desktop_launcher']='/package/AppRun'
    (source/'plan.json').write_text(json.dumps(plan))
    cmd=['bwrap','--unshare-user','--unshare-pid','--unshare-net','--unshare-ipc','--unshare-uts','--die-with-parent','--new-session','--clearenv','--ro-bind','/usr','/usr','--symlink','usr/bin','/bin','--symlink','usr/sbin','/sbin','--ro-bind','/lib','/lib','--ro-bind','/etc','/etc','--dev','/dev','--proc','/proc','--tmpfs','/tmp','--tmpfs','/run','--dir','/run/fixture','--chmod','0700','/run/fixture','--dir','/q','--dir','/home/fixture','--bind',str(logs),'/evidence','--ro-bind',str(source),'/fixture-bin','--ro-bind',str(extracted),'/package','--ro-bind','/usr/bin/WebKitWebDriver','/fixture-bin/WebKitWebDriver']
    if Path('/lib64').exists():cmd+=['--ro-bind','/lib64','/lib64']
    if flatpak:cmd+=['--ro-bind','/var/lib/flatpak','/var/lib/flatpak',*flatpak_bindings(extracted,logs)]
    env={'HOME':'/home/fixture','XDG_RUNTIME_DIR':'/run/fixture','PATH':'/usr/bin:/bin','LANG':'C.UTF-8','TZ':'UTC','GDK_BACKEND':'x11','NO_AT_BRIDGE':'1','TAURI_WEBVIEW_AUTOMATION':'true','WEBKIT_DISABLE_DMABUF_RENDERER':'1','WEBKIT_DISABLE_COMPOSITING_MODE':'1','DEZIRCLIP_DISABLE_LINUX_SERVICES':'window_tracker,edge_docking,mouse_hotkey','TIEZ_DISABLE_LINUX_SERVICES':'window_tracker,edge_docking,mouse_hotkey'}
    for key,value in env.items():cmd+=['--setenv',key,value]
    cmd+=['--chdir','/evidence','/usr/bin/xvfb-run','-a','-s','-screen 0 1280x900x24 -noreset','/usr/bin/python3','-s','/fixture-bin/release_identity_secret_service.py','--helper','/fixture-bin/ss-preseed','--output','/evidence/private-ss','--','/usr/bin/python3','-s','/fixture-bin/release_identity_linux.py','--inside']
    run(cmd,logs,'sandbox',timeout=300)
    secret=json.loads((logs/'private-ss/proof.json').read_text());require(secret['status']=='passed','Private Secret Service proof failed')
    result=json.loads((logs/'actual-identities.json').read_text())
    require(result['credential_probe']['status']=='passed','Inner credential syscall boundary failed')
    (logs/'boundary.json').write_text(json.dumps({'outer':guard,'inner':result['credential_probe'],'helper_sha256':sha(args.preseed_helper),'command':cmd,'secret_service_proof_sha256':sha(logs/'private-ss/proof.json')},indent=2))
    return {role:validate_info(info,role,args.target,args.source) for role,info in result['build_info'].items()}

def inside():
    import release_identity_guard
    from native_capture import Driver
    root=Path('/evidence');plan=json.loads(Path('/fixture-bin/plan.json').read_text())
    probe=release_identity_guard.probe()
    private=Path(os.environ['DZC_QA_FIXTURE_ROOT'])
    require(private.parent==Path('/q') and private.name.startswith('dezirclip-private-ss-') and (private/'.dzc-private-ss-qa-v1').read_text()=='synthetic-fixture-only\n','Private preseeded fixture missing')
    database=Path(os.environ['XDG_DATA_HOME'])/'io.github.qiyuanhuakai.dezirclip/clipboard.db'
    database.parent.mkdir(parents=True,exist_ok=True);os.environ['DEZIRCLIP_DB_PATH']=str(database)
    def command(role,args):
        if plan['flatpak']:
            return ['flatpak','build','--runtime','--readonly','--unshare=network','--share=ipc','--socket=x11','--socket=session-bus','--filesystem=/q','--filesystem=/evidence','--env=TAURI_WEBVIEW_AUTOMATION=true','--env=DEZIRCLIP_DB_PATH='+str(database),'--env=DISPLAY='+os.environ['DISPLAY'],'--env=DBUS_SESSION_BUS_ADDRESS='+os.environ['DBUS_SESSION_BUS_ADDRESS'],'/package','/app/bin/'+{'desktop':'dezirclip','cli':'dzc','service':'dzc-service'}[role],*args]
        return [plan['members'][role],*args]
    result={}
    for role,args in [('cli',['build-info','--json']),('service',['--build-info'])]:
        if role in plan['members']:
            result[role]=validate_info(json_output(run(command(role,args),root,role)),role,plan['target'],plan['source_revision'])
    if 'desktop' in plan['members']:
        run(command('cli',['count','--json']),root,'empty-database')
        require(database.is_file(),'Synthetic metadata profile database missing')
        with sqlite3.connect(database) as connection:
            require(connection.execute('SELECT COUNT(*) FROM clipboard_history').fetchone()==(0,),'Receipt profile must be empty')
            connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',{'app.silent_start':'false','app.idle_destroy_enabled':'false','app.update_auto_check':'false','app.ocr_enabled':'false','app.classification_enabled':'false','app.sequential_mode':'false'}.items())
        executable=plan.get('desktop_launcher',plan['members']['desktop'])
        if plan['flatpak']:
            launcher=root/'desktop-launcher';launcher.write_text('#!/bin/sh\nexec '+shlex.join(command('desktop',[]))+'\n');launcher.chmod(0o700);executable=str(launcher)
        with Driver(root,executable) as driver:
            result['desktop']=validate_info(driver.invoke('get_build_info'),'desktop',plan['target'],plan['source_revision'])
    (root/'actual-identities.json').write_text(json.dumps({'status':'passed','credential_probe':probe,'build_info':result},indent=2)+'\n')

if __name__=='__main__':
    require(sys.argv[1:]==['--inside'],'Only explicit private collector entry is supported');inside()
