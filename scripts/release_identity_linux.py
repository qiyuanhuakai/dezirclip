"""Execute only receipt metadata inside private Linux namespaces and Secret Service."""
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import sys
from release_identities import ROOT, json_output, require, run, sha, validate_info

def inspector_address(value):
    require(isinstance(value,str),'Explicit loopback inspector address required')
    match=re.fullmatch(r'127\.0\.0\.1:([1-9][0-9]{0,4})',value)
    require(match is not None and int(match.group(1))<=65535,'Explicit loopback inspector address required')
    return value


def network_namespace_value(value):
    require(isinstance(value,str) and re.fullmatch(r'net:\[[1-9][0-9]*\]',value),
            'Actual network namespace identity required')
    return value


def current_network_namespace():
    return network_namespace_value(os.readlink('/proc/self/ns/net'))


def flatpak_command(role,arguments,database,environment,inspector=None):
    binaries={'desktop':'dezirclip','cli':'dzc','service':'dzc-service'}
    require(role in binaries and isinstance(arguments,list) and all(isinstance(value,str) for value in arguments),
            'Explicit packaged role and argument vector required')
    if role=='desktop':
        inspector=inspector_address(inspector)
        network='--share=network'
        extra=['--env=WEBKIT_INSPECTOR_SERVER='+inspector]
        private=environment.get('DZC_QA_FIXTURE_ROOT')
        require(isinstance(private,str) and re.fullmatch(r'/q/dezirclip-private-ss-[a-z0-9_-]+',private),
                'Exact original private Secret Service root required for Desktop XDG')
        forwarded={name:private+'/'+relative for name,relative in
                   (('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache'))}
        require(all(environment.get(name)==value for name,value in forwarded.items()),
                'Desktop HOME/XDG must match the original private Secret Service directories')
        extra+=['--env='+name+'='+value for name,value in forwarded.items()]
        target=['/bin/sh','/evidence/flatpak-desktop-inner.sh',*arguments]
    else:
        require(inspector is None,'Only the Desktop automation transport receives an inspector address')
        network='--unshare=network';extra=[]
        target=['/app/bin/'+binaries[role],*arguments]
    return ['flatpak','build','--runtime','--readonly',network,'--share=ipc','--socket=x11','--socket=session-bus',
            '--filesystem=/q','--filesystem=/evidence','--env=TAURI_WEBVIEW_AUTOMATION=true',
            '--env=DEZIRCLIP_DB_PATH='+str(database),'--env=DISPLAY='+environment['DISPLAY'],
            '--env=DBUS_SESSION_BUS_ADDRESS='+environment['DBUS_SESSION_BUS_ADDRESS'],*extra,'/package',*target]


def flatpak_launcher_source():
    return """#!/usr/bin/python3 -s
import sys
sys.dont_write_bytecode=True
sys.path.insert(0,'/fixture-bin')
from release_identity_linux import launch_flatpak_desktop
launch_flatpak_desktop(sys.argv[1:])
"""


def flatpak_inner_source(private):
    private=network_namespace_value(private)
    return """#!/bin/sh
set -eu
network_namespace=$(readlink /proc/self/ns/net)
: "${WEBKIT_INSPECTOR_SERVER:?Driver inspector address missing}"
printf '{"schema_version":1,"kind":"flatpak_automation_inner","status":"observed","network_namespace":"%s","inspector":"%s"}\\n' "$network_namespace" "${WEBKIT_INSPECTOR_SERVER-}" > /evidence/flatpak-transport-inner.log
[ "$network_namespace" = '__PRIVATE_NAMESPACE__' ] || { printf 'Flatpak private network namespace differs\\n' >&2; exit 1; }
exec /app/bin/dezirclip "$@"
""".replace('__PRIVATE_NAMESPACE__',private)


def launch_flatpak_desktop(arguments):
    root=Path('/evidence');plan=json.loads(Path('/fixture-bin/plan.json').read_text())
    require(plan['flatpak'] is True,'Only the private Flatpak Desktop launcher is supported')
    outside=network_namespace_value(plan['outside_network_namespace'])
    private=current_network_namespace()
    require(private!=outside,'Outer automation namespace must remain separate from the host')
    inspector=inspector_address(os.environ.get('WEBKIT_INSPECTOR_SERVER'))
    command=flatpak_command('desktop',arguments,os.environ['DEZIRCLIP_DB_PATH'],os.environ,inspector)
    record={'schema_version':1,'kind':'flatpak_automation_outer','status':'launching',
            'source_revision':plan['source_revision'],'target':plan['target'],
            'outside_network_namespace':outside,'network_namespace':private,'inspector':inspector,
            'browser_arguments':arguments,'product_acceptance_claimed':False}
    with (root/'flatpak-transport-outer.log').open('x',encoding='utf-8') as output:
        json.dump(record,output,indent=2);output.write('\n');output.flush();os.fsync(output.fileno())
    os.execvp(command[0],command)


def validate_flatpak_transport(outer,inner,private,outside):
    private=network_namespace_value(private);outside=network_namespace_value(outside)
    require(private!=outside,'Outer automation namespace must remain separate from the host')
    require(isinstance(outer,dict) and type(outer.get('schema_version')) is int and outer['schema_version']==1
            and outer.get('kind')=='flatpak_automation_outer' and outer.get('status')=='launching'
            and outer.get('product_acceptance_claimed') is False,'Original outer automation transport observation required')
    require(isinstance(inner,dict) and type(inner.get('schema_version')) is int and inner['schema_version']==1
            and inner.get('kind')=='flatpak_automation_inner' and inner.get('status')=='observed',
            'Original inner automation transport observation required')
    require(network_namespace_value(outer.get('outside_network_namespace'))==outside
            and network_namespace_value(outer.get('network_namespace'))==private
            and network_namespace_value(inner.get('network_namespace'))==private,
            'Flatpak automation must share only the original outer private network namespace')
    inspector=inspector_address(outer.get('inspector'))
    require(inspector_address(inner.get('inspector'))==inspector,'Flatpak inspector address changed at the namespace boundary')
    return {'schema_version':1,'kind':'flatpak_automation_transport','status':'observed_same_private_network',
            'outside_network_namespace':outside,'network_namespace':private,'inspector':inspector,
            'host_network_shared':False,'product_acceptance_claimed':False}


WEBVIEW_DIAGNOSTIC_SCRIPT = '''
return {
  url: /^(data|javascript):/i.test(window.location.href) ? window.location.protocol + "[omitted]" : window.location.href.slice(0, 2048),
  readyState: document.readyState,
  invokeType: typeof window.__TAURI_INTERNALS__?.invoke,
  bodyTextLength: document.body ? document.body.innerText.length : null,
  rootChildElementCount: document.getElementById("root")?.childElementCount ?? null
};
'''


def diagnostic_text(value,limit):
    if not isinstance(value,str):return None
    value=re.sub(r'[\x00-\x1f\x7f]','',value)
    return value.encode('utf-8',errors='replace')[:limit].decode('utf-8',errors='ignore')


def retain_webview_failure(driver,root,plan,error,request):
    record={'schema_version':1,'kind':'linux_webview_failure_diagnostics','status':'diagnostic_only',
            'source_revision':plan['source_revision'],'target':plan['target'],
            'original_error_type':diagnostic_text(type(error).__name__,80),'product_acceptance_claimed':False,
            'session_established':bool(driver.route),'page':None,'window_handles':None,
            'current_window_handle':None,'diagnostic_errors':{}}
    if driver.route:
        try:
            page=driver.execute(WEBVIEW_DIAGNOSTIC_SCRIPT)
            require(isinstance(page,dict),'WebView metadata object required')
            record['page']={key:diagnostic_text(page.get(key),limit)
                            for key,limit in (('url',2048),('readyState',32),('invokeType',16))}
            if record['page']['url'] and re.match(r'(?i)^(data|javascript):',record['page']['url']):
                record['page']['url']=record['page']['url'].split(':',1)[0]+':[omitted]'
            for key in ('bodyTextLength','rootChildElementCount'):
                value=page.get(key)
                record['page'][key]=value if type(value) is int and 0<=value<=2**53-1 else None
        except Exception as failure:record['diagnostic_errors']['page']=diagnostic_text(type(failure).__name__,80)
        try:
            handles=request(driver.port,driver.route+'/window/handles')
            require(isinstance(handles,list) and all(isinstance(value,str) for value in handles),
                    'WebDriver window handle list required')
            record['window_handles']=[diagnostic_text(value,256) for value in handles[:16]]
            record['window_handles_truncated']=len(handles)>16
        except Exception as failure:record['diagnostic_errors']['window_handles']=diagnostic_text(type(failure).__name__,80)
        try:
            handle=request(driver.port,driver.route+'/window')
            require(isinstance(handle,str),'Current WebDriver window handle required')
            record['current_window_handle']=diagnostic_text(handle,256)
        except Exception as failure:record['diagnostic_errors']['current_window_handle']=diagnostic_text(type(failure).__name__,80)
    encoded=json.dumps(record,indent=2,ensure_ascii=False)+'\n'
    require(len(encoded.encode('utf-8'))<=16*1024,'WebView diagnostic bound exceeded')
    with (root/'desktop-webview-failure.log').open('x',encoding='utf-8') as output:
        output.write(encoded);output.flush();os.fsync(output.fileno())


def desktop_build_info(driver,root,plan,request):
    entered=False
    try:
        with driver:
            entered=True
            return validate_info(driver.invoke('get_build_info'),'desktop',plan['target'],plan['source_revision'])
    except Exception as error:
        if not entered:
            try:retain_webview_failure(driver,root,plan,error,request)
            except Exception:pass
        raise
    finally:
        if not entered:
            try:driver.close()
            except Exception:pass


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
    # The later file bind needs an existing destination in the read-only tree.
    (source/'WebKitWebDriver').open('xb').close()
    plan={'source_revision':args.source,'target':args.target,'flatpak':flatpak,'members':{role:'/package/'+path.relative_to(extracted).as_posix() for role,path in members.items()}}
    if flatpak and 'desktop' in members:
        plan['outside_network_namespace']=current_network_namespace()
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
    boundary={'outer':guard,'inner':result['credential_probe'],'helper_sha256':sha(args.preseed_helper),'command':cmd,'secret_service_proof_sha256':sha(logs/'private-ss/proof.json')}
    if flatpak and 'desktop' in members:
        transport=result['flatpak_transport']
        require(type(transport.get('schema_version')) is int and transport['schema_version']==1
                and transport['kind']=='flatpak_automation_transport' and transport['status']=='observed_same_private_network'
                and transport['outside_network_namespace']==plan['outside_network_namespace']
                and transport['network_namespace']!=plan['outside_network_namespace']
                and transport['host_network_shared'] is False and transport['product_acceptance_claimed'] is False,
                'Original private Flatpak transport proof differs')
        boundary['flatpak_transport']=transport
    (logs/'boundary.json').write_text(json.dumps(boundary,indent=2))
    return {role:validate_info(info,role,args.target,args.source) for role,info in result['build_info'].items()}

def inside():
    import release_identity_guard
    from native_capture import Driver, request
    root=Path('/evidence');plan=json.loads(Path('/fixture-bin/plan.json').read_text())
    probe=release_identity_guard.probe()
    private=Path(os.environ['DZC_QA_FIXTURE_ROOT'])
    require(private.parent==Path('/q') and private.name.startswith('dezirclip-private-ss-') and (private/'.dzc-private-ss-qa-v1').read_text()=='synthetic-fixture-only\n','Private preseeded fixture missing')
    database=Path(os.environ['XDG_DATA_HOME'])/'io.github.qiyuanhuakai.dezirclip/clipboard.db'
    database.parent.mkdir(parents=True,exist_ok=True);os.environ['DEZIRCLIP_DB_PATH']=str(database)
    def command(role,args):
        if plan['flatpak']:
            return flatpak_command(role,args,database,os.environ)
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
            launcher=root/'desktop-launcher';launcher.write_text(flatpak_launcher_source());launcher.chmod(0o700);executable=str(launcher)
            inner=root/'flatpak-desktop-inner.sh';inner.write_text(flatpak_inner_source(current_network_namespace()));inner.chmod(0o700)
        driver=Driver(root,executable)
        result['desktop']=desktop_build_info(driver,root,plan,request)
    observed={'status':'passed','credential_probe':probe,'build_info':result}
    if plan['flatpak'] and 'desktop' in plan['members']:
        outer=json.loads((root/'flatpak-transport-outer.log').read_text())
        require(outer['source_revision']==plan['source_revision'] and outer['target']==plan['target'],
                'Flatpak transport source or target changed')
        observed['flatpak_transport']=validate_flatpak_transport(outer,json.loads((root/'flatpak-transport-inner.log').read_text()),
            current_network_namespace(),plan['outside_network_namespace'])
    (root/'actual-identities.json').write_text(json.dumps(observed,indent=2)+'\n')

if __name__=='__main__':
    require(sys.argv[1:]==['--inside'],'Only explicit private collector entry is supported');inside()
