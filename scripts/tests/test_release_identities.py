"""Synthetic release receipt validation only; no product, installer or clipboard runs."""
import copy
import ast
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import textwrap
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import release_identities as identity
import release_identity_guard as guard
from release_identity_merge import merge
from release_identity_packages import squashfs_offset
from release_identity_packages import windows_environment
from release_identity_packages import remove_owned_tree
from release_identity_linux import flatpak_bindings
from release_identity_linux import sandbox_info
from release_identity_linux import (inspector_address,network_namespace_value,flatpak_command,
    flatpak_launcher_source,flatpak_inner_source,validate_flatpak_transport,
    desktop_build_info,retain_webview_failure,WEBVIEW_DIAGNOSTIC_SCRIPT)

REVISION='a'*40
DIGEST='b'*64

def info(role,target):
    names=['dzc-core','dzc-platform','dzc-protocol','dzc-host']+(['dzc-service'] if role=='service' else [])
    return {'metadata_version':1,'component':role,'component_version':identity.VERSION,'build_id':identity.VERSION+'@'+REVISION,'source_revision':REVISION,'source_state':'verified_checkout','revision_origin':'explicit_checkout','target':identity.TARGETS[target],'protocol_version':{'major':1,'minor':0},'database_schema_version':24,'linked_components':[{'component':name,'version':identity.VERSION} for name in names]}

def report(target):
    standalone,containers=identity.expected_assets(target)
    return {'schema_version':1,'status':'passed','method':'executed_native','tag':'v'+identity.VERSION,'source_revision':REVISION,'target':target,'runner':{'system':'Windows' if target.startswith('windows-') else 'Linux','machine':'ARM64' if target.endswith('aarch64') else 'x86_64'},'job_id':123,'run_url':'https://github.com/qiyuanhuakai/dezirclip/actions/runs/123','standalone':[{'platform':target,'asset':standalone,'asset_sha256':DIGEST,'path':'dzc.exe' if target.startswith('windows-') else 'dzc','bytes':123,'binary_sha256':DIGEST,'build_info':info('cli',target)}],'containers':[{'platform':target,'asset':name,'asset_sha256':DIGEST,'contents':[{'component':role,'path':'usr/bin/'+binary,'bytes':123,'binary_sha256':DIGEST,'build_info':info(role,target)} for role,binary in identity.ROLES.items()]} for name in containers]}

def evaluate(machine,audit,number):
    accumulator=0;pc=0;program=guard.instructions(machine)
    for _ in range(100):
        code,jt,jf,value=program[pc]
        if code==0x20:accumulator=audit if value==4 else number
        elif code==0x54:accumulator&=value
        elif code==0x15:pc+=(jt if accumulator==value else jf)
        elif code==0x06:return value
        else:raise AssertionError('Unknown BPF instruction')
        pc+=1
    raise AssertionError('Filter did not return')

class ReleaseIdentities(unittest.TestCase):
    def test_final_mount_step_requires_the_original_run_receipt_and_only_reads_sysctl(self):
        source=(identity.ROOT/'.github/workflows/ci.yml').read_text(encoding='utf-8')
        def block(name):
            step=source.split('      - name: '+name+'\n',1)[1].split('\n      - name:',1)[0].split('\n  standalone-cli:',1)[0]
            return textwrap.dedent(step.split('        run: |\n',1)[1]).rstrip()
        early=block('Permit private user namespaces on disposable Linux runner')
        late=block('Verify actual private Linux release receipt mounts')
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            root=Path(temp);concrete=type(root)
            environment={'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'github-hosted','RUNNER_TEMP':temp,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'2','GITHUB_JOB':'tauri','GITHUB_SHA':REVISION,'RUNNER_NAME':'synthetic-runner','RUNNER_ARCH':'X64'}
            current=['1','0'];key=SimpleNamespace(exists=lambda:True,read_text=lambda:current.pop(0) if len(current)>1 else current[0])
            label=SimpleNamespace(read_text=lambda:'synthetic-unconfined')
            def path(name):
                if str(name)=='/proc/sys/kernel/apparmor_restrict_unprivileged_userns':return key
                if str(name)=='/proc/self/attr/current':return label
                return concrete(name)
            with patch.dict(os.environ,environment,clear=True),patch('sys.platform','linux'),patch('pathlib.Path',side_effect=path),patch('shutil.which',return_value='/usr/bin/bwrap'),patch('subprocess.run') as execute,patch('builtins.print'):
                exec(early,{})
                execute.assert_called_once_with(['sudo','-n','sysctl','-w','kernel.apparmor_restrict_unprivileged_userns=0'],check=True)
                receipt=root/'dezirclip-userns-123-2-tauri.json';original=json.loads(receipt.read_text())
                execute.reset_mock();exec(late,{})
                execute.assert_called_once()
                self.assertNotIn('sudo',execute.call_args.args[0])
                for field,value in (('GITHUB_RUN_ID','124'),('GITHUB_RUN_ATTEMPT','3'),('GITHUB_JOB','other'),('GITHUB_SHA','c'*40),('RUNNER_NAME','other'),('RUNNER_ARCH','ARM64')):
                    altered=copy.deepcopy(original);altered['identity'][field]=value;receipt.write_text(json.dumps(altered));execute.reset_mock()
                    with self.subTest(field=field),self.assertRaises(AssertionError):exec(late,{})
                    execute.assert_not_called()
                for field,value in (('schema_version',True),('status','failed'),('key_exists',1),('before','unknown'),('after','1'),('padding','x'*8192)):
                    altered=copy.deepcopy(original);altered[field]=value;receipt.write_text(json.dumps(altered));execute.reset_mock()
                    with self.subTest(field=field),self.assertRaises(AssertionError):exec(late,{})
                    execute.assert_not_called()
                receipt.write_text(json.dumps(original));current[0]='1';execute.reset_mock()
                with self.assertRaises(AssertionError):exec(late,{})
                execute.assert_not_called();receipt.unlink()
                with self.assertRaises(AssertionError):exec(late,{})
                execute.assert_not_called()

    def test_disposable_userns_setup_refuses_local_execution_and_checks_readback(self):
        blocks=[]
        for name in ('ci.yml','release-identities.yml'):
            source=(identity.ROOT/'.github/workflows'/name).read_text(encoding='utf-8')
            step=source.split('      - name: Permit private user namespaces on disposable Linux runner\n',1)[1].split('\n      - name:',1)[0]
            blocks.append(textwrap.dedent(step.split('        run: |\n',1)[1].split('          # Retain this run',1)[0]).rstrip())
        self.assertEqual(blocks[0],blocks[1])
        for environment in ({},{'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'self-hosted'}):
            with patch.dict(os.environ,environment,clear=True),patch('sys.platform','linux'),patch('subprocess.run') as mutate,patch('pathlib.Path') as path:
                with self.assertRaises(AssertionError):exec(blocks[0],{})
                mutate.assert_not_called();path.assert_not_called()
        for exists,before,after,passes in ((False,None,None,True),(True,'1','0',True),(True,'0','0',True),(True,'1','1',False),(True,'unexpected','0',False)):
            key=SimpleNamespace(exists=lambda:exists,read_text=lambda:next(values))
            values=iter((before,after))
            label=SimpleNamespace(read_text=lambda:'synthetic-unconfined')
            with patch.dict(os.environ,{'GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'github-hosted'},clear=True),patch('sys.platform','linux'),patch('pathlib.Path',side_effect=lambda name:key if name.endswith('apparmor_restrict_unprivileged_userns') else label),patch('subprocess.run') as mutate,patch('builtins.print'):
                if passes:exec(blocks[0],{})
                else:
                    with self.assertRaises(AssertionError):exec(blocks[0],{})
                if exists and before in ('0','1'):mutate.assert_called_once_with(['sudo','-n','sysctl','-w','kernel.apparmor_restrict_unprivileged_userns=0'],check=True)
                else:mutate.assert_not_called()

    @unittest.skipUnless(sys.platform=='linux' and shutil.which('bwrap'),'Real Linux bubblewrap mount control; no product execution')
    def test_real_readonly_directory_requires_the_nested_file_mountpoint(self):
        source=str(Path(identity.__file__).parent)
        inner='''
import errno,json,os,pathlib,sys
sys.path.insert(0,'/fixture-bin')
import guard
proof={'guard':guard.probe(),'namespaces':{name:os.readlink('/proc/self/ns/'+name) for name in ('user','pid','net','ipc','uts','mnt')}}
try:pathlib.Path('/fixture-bin/refuse-write').write_bytes(b'synthetic')
except OSError as error:
    assert error.errno==errno.EROFS
    proof['readonly_tree']=True
else:raise AssertionError('Fixture tree became writable')
print(json.dumps(proof))
'''
        script='''
import json,os,pathlib,subprocess,sys,tempfile
sys.path.insert(0,sys.argv[1])
import release_identity_guard as guard
proof={'guard':guard.install(),'mounts':[],'parent_namespaces':{name:os.readlink('/proc/self/ns/'+name) for name in ('user','pid','net','ipc','uts','mnt')}}
with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-mount-') as temp:
    root=pathlib.Path(temp)
    (root/'guard.py').write_bytes((pathlib.Path(sys.argv[1])/'release_identity_guard.py').read_bytes())
    for prepared in (False,True):
        if prepared:(root/'WebKitWebDriver').open('xb').close()
        command=['bwrap','--unshare-all','--die-with-parent','--new-session','--clearenv','--ro-bind','/usr','/usr','--ro-bind','/lib','/lib','--proc','/proc','--dev','/dev']
        if pathlib.Path('/lib64').exists():command+=['--ro-bind','/lib64','/lib64']
        command+=['--ro-bind',str(root),'/fixture-bin','--ro-bind','/usr/bin/true','/fixture-bin/WebKitWebDriver','/usr/bin/python3','-I','-B','-c',sys.argv[2]]
        result=subprocess.run(command,capture_output=True,text=True,timeout=10)
        proof['mounts'].append({'prepared':prepared,'returncode':result.returncode,'stderr':result.stderr,'stdout':result.stdout})
print(json.dumps(proof))
'''
        result=subprocess.run([sys.executable,'-I','-B','-c',script,source,inner],capture_output=True,text=True,timeout=25)
        self.assertEqual(result.returncode,0,result.stderr)
        proof=json.loads(result.stdout);self.assertEqual(proof['guard']['status'],'passed')
        missing,prepared=proof['mounts']
        self.assertNotEqual(missing['returncode'],0);self.assertIn('Read-only file system',missing['stderr'])
        self.assertEqual(prepared['returncode'],0,prepared['stderr'])
        inside=json.loads(prepared['stdout']);self.assertTrue(inside['readonly_tree'])
        self.assertEqual(inside['guard']['status'],'passed')
        self.assertTrue(all(proof['parent_namespaces'][name]!=value for name,value in inside['namespaces'].items()))
        print('DEZIRCLIP_RELEASE_MOUNT_BOUNDARY '+json.dumps(proof),flush=True)

    def test_every_linux_receipt_prepares_driver_before_readonly_source_bind(self):
        for package in ('standalone','AppImage','deb','rpm','flatpak'):
            with self.subTest(package=package),tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
                logs=Path(temp);extracted=logs/'extracted';extracted.mkdir()
                roles=('cli',) if package=='standalone' else tuple(identity.ROLES)
                members={role:extracted/binary for role,binary in identity.ROLES.items() if role in roles}
                for path in members.values():path.write_bytes(b'synthetic member; never executed')
                before={role:identity.sha(path) for role,path in members.items()}
                helper=logs/'synthetic-helper';helper.write_bytes(b'synthetic helper; never executed')
                args=SimpleNamespace(preseed_helper=helper,source=REVISION,target='linux-x86_64')
                def assert_mount(command,directory,label,**kwargs):
                    source=logs/'sources';destination=source/'WebKitWebDriver'
                    self.assertTrue(destination.is_file());self.assertFalse(destination.is_symlink())
                    self.assertEqual(destination.read_bytes(),b'')
                    source_bind=command.index(str(source))
                    self.assertEqual(command[source_bind-1:source_bind+2],['--ro-bind',str(source),'/fixture-bin'])
                    driver_bind=command.index('/usr/bin/WebKitWebDriver')
                    self.assertGreater(driver_bind,source_bind)
                    self.assertEqual(command[driver_bind-1:driver_bind+2],['--ro-bind','/usr/bin/WebKitWebDriver','/fixture-bin/WebKitWebDriver'])
                    for flag in ('--unshare-user','--unshare-pid','--unshare-net','--unshare-ipc','--unshare-uts'):
                        self.assertIn(flag,command)
                    private=logs/'private-ss';private.mkdir();(private/'proof.json').write_text(json.dumps({'status':'passed'}))
                    observed={'credential_probe':{'status':'passed'},'build_info':{role:info(role,args.target) for role in roles}}
                    if package=='flatpak':
                        observed['flatpak_transport']={'schema_version':1,'kind':'flatpak_automation_transport',
                            'status':'observed_same_private_network','outside_network_namespace':'net:[1]',
                            'network_namespace':'net:[2]','inspector':'127.0.0.1:12345',
                            'host_network_shared':False,'product_acceptance_claimed':False}
                    (logs/'actual-identities.json').write_text(json.dumps(observed))
                with patch('release_identity_guard.install',return_value={'synthetic':True}),patch('release_identity_linux.current_network_namespace',return_value='net:[1]'),patch('release_identity_linux.run',side_effect=assert_mount) as execute:
                    result=sandbox_info(args,members,extracted,logs,package=='flatpak')
                self.assertEqual(set(result),set(roles));execute.assert_called_once()
                self.assertEqual(before,{role:identity.sha(path) for role,path in members.items()})

    def test_flatpak_var_is_an_owned_separate_mount_without_member_changes(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            logs=Path(temp);extracted=logs/'extracted';extracted.mkdir()
            (extracted/'files').mkdir();(extracted/'files/dzc').write_bytes(b'synthetic original executable')
            (extracted/'metadata').write_bytes(b'synthetic original metadata')
            before={name:identity.sha(extracted/name) for name in ('files/dzc','metadata')}
            bindings=flatpak_bindings(extracted,logs)
            self.assertEqual(bindings[:3],['--bind',str(logs/'flatpak-var'),'/package/var'])
            self.assertEqual(bindings[3:],sum((['--dir','/sys'+name] for name in ('','/block','/bus','/class','/dev','/devices')),[]))
            self.assertTrue((extracted/'var').is_dir());self.assertEqual(list((extracted/'var').iterdir()),[])
            self.assertTrue((logs/'flatpak-var/tmp').is_dir());self.assertTrue((logs/'flatpak-var/lib').is_dir())
            self.assertEqual(before,{name:identity.sha(extracted/name) for name in before})
            outside=logs.parent/'synthetic-outside-package'
            with self.assertRaises(ValueError):flatpak_bindings(outside,logs)

    def test_cleanup_checks_the_final_owned_target(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            root=Path(temp);owned=root/'owned';owned.mkdir();outside=root/'outside';outside.mkdir()
            (outside/'preserved').write_text('synthetic preserved file')
            with self.assertRaises(ValueError):remove_owned_tree(outside,owned)
            self.assertTrue((outside/'preserved').is_file())
            child=owned/'extracted';child.mkdir();remove_owned_tree(child,owned)
            self.assertFalse(child.exists())
    @unittest.skipUnless(sys.platform=='linux','Native Linux credential guard only; no product execution')
    def test_native_guard_is_inherited_by_a_child_with_only_safe_invalid_probes(self):
        source=str(Path(identity.__file__).parent)
        nested='import sys,json;sys.path.insert(0,'+repr(source)+');import release_identity_guard as g;print(json.dumps(g.probe()))'
        script='import sys,json,subprocess;sys.path.insert(0,'+repr(source)+');import release_identity_guard as g;outer=g.install();inner=json.loads(subprocess.check_output([sys.executable,"-I","-B","-c",'+repr(nested)+'],text=True));print(json.dumps({"outer":outer,"inner":inner}))'
        result=subprocess.run([sys.executable,'-I','-B','-c',script],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        proof=json.loads(result.stdout)
        for name in ('outer','inner'):
            self.assertEqual(proof[name]['status'],'passed')
            self.assertTrue(all(call['errno']==1 and call['result']==-1 for call in proof[name]['calls']))

    def test_child_logs_are_bounded_and_deadlines_reject_without_product_execution(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            root=Path(temp)
            with self.assertRaisesRegex(ValueError,'output exceeds bound'):
                identity.run([sys.executable,'-I','-c','import sys;sys.stderr.write("x"*(9*1024*1024))'],root,'overflow')
            self.assertLessEqual((root/'overflow.stderr.log').stat().st_size,8*1024*1024)
            with self.assertRaisesRegex(ValueError,'execution deadline'):
                identity.run([sys.executable,'-I','-c','import time;time.sleep(5)'],root,'deadline',timeout=.1)

    def test_windows_product_environment_drops_actions_and_publishing_credentials(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp,patch.dict(os.environ,{'RUNNER_TEMP':temp,'GH_TOKEN':'synthetic-do-not-forward','ACTIONS_RUNTIME_TOKEN':'synthetic-do-not-forward'},clear=True):
            env=windows_environment(Path(temp))
            self.assertNotIn('GH_TOKEN',env);self.assertNotIn('ACTIONS_RUNTIME_TOKEN',env)
            self.assertTrue(Path(env['APPDATA']).resolve().is_relative_to(Path(temp).resolve()))
    def test_exact_four_standalone_and_ten_distinct_containers(self):
        assets=[asset for target in identity.TARGETS for group in identity.expected_assets(target) for asset in ([group] if isinstance(group,str) else group)]
        self.assertEqual(len(assets),14);self.assertEqual(len(set(assets)),14)

    def test_actual_schema_inventory_source_and_types_are_required(self):
        for target in identity.TARGETS:
            for role in identity.ROLES:
                identity.validate_info(info(role,target),role,target,REVISION)
                changes=[('source_revision','c'*40),('build_id','unrelated'),('database_schema_version',True),('database_schema_version',23),('metadata_version',True),('source_state','modified_checkout'),('target','other'),('protocol_version',{'major':True,'minor':0}),('linked_components',[])]
                for name,value in changes:
                    malformed=info(role,target);malformed[name]=value
                    with self.subTest(target=target,role=role,field=name),self.assertRaises(ValueError):identity.validate_info(malformed,role,target,REVISION)
                malformed=info(role,target);malformed['linked_components'][-1]=copy.deepcopy(malformed['linked_components'][0])
                with self.assertRaises(ValueError):identity.validate_info(malformed,role,target,REVISION)

    def test_merge_binds_original_report_hashes_and_never_changes_target(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            paths=[]
            for target in identity.TARGETS:
                path=Path(temp)/(target+'.json');path.write_text(json.dumps(report(target)));paths.append(path)
            merged=merge(paths,REVISION)
            self.assertEqual(len(merged['standalone']),4);self.assertEqual(len(merged['containers']),10)
            self.assertEqual(merged['standalone'][0]['proof']['evidence_sha256'],identity.sha(paths[0]))
            for index,mutation in enumerate([lambda r:r.update(status='failed'),lambda r:r.update(method='ci_bound_static'),lambda r:r.update(source_revision='d'*40),lambda r:r['runner'].update(machine='x86_64'),lambda r:r['containers'].pop(),lambda r:r['containers'][0]['contents'][0].update(binary_sha256='missing'),lambda r:r['standalone'][0].pop('path'),lambda r:r['standalone'][0].update(path='../dzc.exe'),lambda r:r['standalone'][0].pop('bytes'),lambda r:r['standalone'][0].update(bytes=True)]):
                original=report('windows-aarch64');mutation(original);paths[3].write_text(json.dumps(original))
                with self.subTest(mutation=index),self.assertRaises(ValueError):merge(paths,REVISION)

    def test_local_process_cannot_start_final_receipt_execution(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp,patch.dict(os.environ,{},clear=True):
            out=Path(temp)/'new'
            with self.assertRaises(ValueError):identity.source_boundary('windows-x86_64',REVISION,out)
            self.assertFalse(out.exists())

    def test_native_target_rejects_cross_runner_and_wrong_pe_machine(self):
        with patch.dict(os.environ,{},clear=True):
            identity.native_target('windows-aarch64','ARM64','Windows')
            with self.assertRaises(ValueError):identity.native_target('windows-aarch64','AMD64','Windows')
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            data=bytearray(128);data[:2]=b'MZ';struct.pack_into('<I',data,60,64);data[64:68]=b'PE\0\0';struct.pack_into('<H',data,68,0xaa64);struct.pack_into('<H',data,88,0x20b)
            path=Path(temp)/'synthetic.exe';path.write_bytes(data)
            identity.inspect_machine(path,'windows-aarch64')
            with self.assertRaises(ValueError):identity.inspect_machine(path,'windows-x86_64')

    def test_paths_and_squashfs_offsets_fail_closed(self):
        for name in ('../escape','/absolute','C:/absolute','a\\b','a/../b','a//b'):
            with self.assertRaises(ValueError):identity.safe_member(name)
        with tempfile.TemporaryDirectory(prefix='dezirclip-receipt-test-') as temp:
            path=Path(temp)/'synthetic.AppImage';data=bytearray(512);data[128:132]=b'hsqs';struct.pack_into('<HH',data,156,4,0);struct.pack_into('<Q',data,168,96);path.write_bytes(data)
            self.assertEqual(squashfs_offset(path),128)
            data[256:352]=data[128:224];path.write_bytes(data)
            with self.assertRaises(ValueError):squashfs_offset(path)

    def test_bpf_denies_all_credential_calls_and_foreign_abis(self):
        for machine,(audit,numbers,getpid) in guard.ARCH.items():
            for number in numbers:self.assertEqual(evaluate(machine,audit,number),0x50001)
            self.assertEqual(evaluate(machine,audit,getpid),0x7fff0000)
            self.assertEqual(evaluate(machine,audit^1,getpid),0x80000000)
            if machine=='x86_64':
                for number in numbers:self.assertEqual(evaluate(machine,audit,number|0x40000000),0x50001)

class FlatpakAutomationTransport(unittest.TestCase):
    def environment(self):
        private='/q/dezirclip-private-ss-test'
        return {'DISPLAY':':99','DBUS_SESSION_BUS_ADDRESS':'unix:path='+private+'/runtime/bus',
                'DZC_QA_FIXTURE_ROOT':private,'HOME':private+'/home','XDG_CONFIG_HOME':private+'/config',
                'XDG_DATA_HOME':private+'/data','XDG_CACHE_HOME':private+'/cache',
                'GH_TOKEN':'fixture-not-forwarded','ARBITRARY':'fixture-not-forwarded'}

    def observations(self):
        outer={'schema_version':1,'kind':'flatpak_automation_outer','status':'launching',
               'outside_network_namespace':'net:[1]','network_namespace':'net:[2]',
               'inspector':'127.0.0.1:12345','product_acceptance_claimed':False}
        inner={'schema_version':1,'kind':'flatpak_automation_inner','status':'observed',
               'network_namespace':'net:[2]','inspector':'127.0.0.1:12345'}
        return outer,inner

    def test_only_desktop_shares_the_outer_network_and_forwards_one_inspector(self):
        arguments=['--automation','space argument','semi;argument']
        command=flatpak_command('desktop',arguments,'/q/private/data/clipboard.db',self.environment(),'127.0.0.1:12345')
        self.assertIn('--share=network',command);self.assertNotIn('--unshare=network',command)
        self.assertIn('--readonly',command);self.assertIn('--share=ipc',command)
        self.assertEqual([value for value in command if value.startswith('--env=WEBKIT_INSPECTOR_SERVER=')],
                         ['--env=WEBKIT_INSPECTOR_SERVER=127.0.0.1:12345'])
        self.assertEqual(command[command.index('/package')+1:],['/bin/sh','/evidence/flatpak-desktop-inner.sh',*arguments])
        self.assertNotIn('fixture-not-forwarded',' '.join(command))
        with self.assertRaises(ValueError):flatpak_command('desktop',arguments,'/q/db',self.environment())

    def test_desktop_forwards_the_four_exact_original_private_profile_directories(self):
        environment=self.environment();private=environment['DZC_QA_FIXTURE_ROOT']
        arguments=['--automation','space argument','semi;argument']
        database=private+'/data/io.github.qiyuanhuakai.dezirclip/clipboard.db'
        command=flatpak_command('desktop',arguments,database,environment,'127.0.0.1:12345')
        for name,relative in (('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache')):
            self.assertEqual([value for value in command if value.startswith('--env='+name+'=')],
                             ['--env='+name+'='+private+'/'+relative])
        self.assertIn('--env=DEZIRCLIP_DB_PATH='+database,command)
        self.assertEqual(command[command.index('/package')+1:],['/bin/sh','/evidence/flatpak-desktop-inner.sh',*arguments])
        self.assertNotIn('--env=DZC_QA_FIXTURE_ROOT='+private,command)
        self.assertFalse(any(value.startswith('--env=FONTCONFIG_') or value.endswith('/fonts:ro') for value in command))

    def test_desktop_refuses_missing_host_and_noncanonical_private_profile_values(self):
        environment=self.environment()
        invalid_roots=(None,'','/home/actual-user','/q/private','/q/dezirclip-private-ss-',
                       '/q/dezirclip-private-ss-test/','/q//dezirclip-private-ss-test',
                       '/q/dezirclip-private-ss-test/../outside','relative','/q/dezirclip-private-ss-test\n')
        for private in invalid_roots:
            with self.subTest(private=private),self.assertRaises(ValueError):
                flatpak_command('desktop',[],'/q/private/db',{**environment,'DZC_QA_FIXTURE_ROOT':private},'127.0.0.1:12345')
        for name in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME'):
            for value in (None,'','/home/actual-user',environment[name]+'/',environment[name]+'/../outside'):
                with self.subTest(name=name,value=value),self.assertRaises(ValueError):
                    flatpak_command('desktop',[],'/q/private/db',{**environment,name:value},'127.0.0.1:12345')
            missing=dict(environment);missing.pop(name)
            with self.subTest(missing=name),self.assertRaises(ValueError):
                flatpak_command('desktop',[],'/q/private/db',missing,'127.0.0.1:12345')

    def test_only_desktop_receives_profile_overrides_without_extending_filesystem_or_network(self):
        environment=self.environment()
        for role in ('cli','service'):
            command=flatpak_command(role,[],'/q/private/db',environment)
            self.assertFalse(any(value.startswith('--env='+name+'=') for value in command
                                 for name in ('HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME')))
            self.assertIn('--unshare=network',command)
        desktop=flatpak_command('desktop',[],'/q/private/db',environment,'127.0.0.1:12345')
        self.assertEqual([value for value in desktop if value.startswith('--filesystem=')],
                         ['--filesystem=/q','--filesystem=/evidence'])
        self.assertEqual([value for value in desktop if value in ('--share=network','--unshare=network')],['--share=network'])
        self.assertNotIn('fixture-not-forwarded',' '.join(desktop))
        self.assertIn('exec /app/bin/dezirclip "$@"',flatpak_inner_source('net:[2]'))

    def test_cli_service_and_count_keep_their_nested_network(self):
        for role,arguments,binary in [('cli',['build-info','--json'],'dzc'),('cli',['count','--json'],'dzc'),('service',['--build-info'],'dzc-service')]:
            command=flatpak_command(role,arguments,'/q/private/db',self.environment())
            self.assertIn('--unshare=network',command);self.assertNotIn('--share=network',command)
            self.assertEqual(command[command.index('/package')+1:],['/app/bin/'+binary,*arguments])
            self.assertFalse(any('WEBKIT_INSPECTOR_SERVER' in value for value in command))
            with self.assertRaises(ValueError):flatpak_command(role,arguments,'/q/db',self.environment(),'127.0.0.1:12')

    def test_launcher_generates_no_frozen_inspector_and_preserves_driver_arguments(self):
        source=flatpak_launcher_source();tree=ast.parse(source)
        self.assertEqual([node.module for node in ast.walk(tree) if isinstance(node,ast.ImportFrom)],['release_identity_linux'])
        self.assertIn("sys.path.insert(0,'/fixture-bin')",source)
        self.assertIn('launch_flatpak_desktop(sys.argv[1:])',source)
        self.assertNotIn('WEBKIT_INSPECTOR_SERVER',source)
        inner=flatpak_inner_source('net:[2]')
        self.assertIn('readlink /proc/self/ns/net',inner)
        self.assertIn('${WEBKIT_INSPECTOR_SERVER-}',inner)
        self.assertIn("[ \"$network_namespace\" = 'net:[2]' ]",inner)
        self.assertIn('exec /app/bin/dezirclip "$@"',inner)
        self.assertNotIn('env |',inner)

    def test_invalid_inspector_and_namespace_values_are_rejected(self):
        for value in (None,'','127.0.0.1:0','127.0.0.1:65536','127.0.0.1:0001','127.0.0.1:12\n',
                      '0.0.0.0:12','localhost:12','[::1]:12','https://127.0.0.1:12','127.0.0.1:12;echo secret'):
            with self.subTest(value=value),self.assertRaises(ValueError):inspector_address(value)
        for value in (None,'','net:[]','net:[0]','mnt:[1]','net:[-1]','net:[2]\n'):
            with self.subTest(value=value),self.assertRaises(ValueError):network_namespace_value(value)
            with self.subTest(generated_namespace=value),self.assertRaises(ValueError):flatpak_inner_source(value)
        self.assertEqual(inspector_address('127.0.0.1:65535'),'127.0.0.1:65535')

    def test_inner_and_outer_must_share_the_private_namespace_and_never_the_host(self):
        outer,inner=self.observations()
        observed=validate_flatpak_transport(outer,inner,'net:[2]','net:[1]')
        self.assertEqual(observed['status'],'observed_same_private_network')
        self.assertFalse(observed['host_network_shared']);self.assertFalse(observed['product_acceptance_claimed'])
        for changed in [dict(inner,network_namespace='net:[1]'),dict(inner,network_namespace='net:[3]'),
                        dict(inner,schema_version=True)]:
            with self.assertRaises(ValueError):validate_flatpak_transport(outer,changed,'net:[2]','net:[1]')
        with self.assertRaises(ValueError):validate_flatpak_transport(outer,inner,'net:[1]','net:[1]')
        with self.assertRaises(ValueError):validate_flatpak_transport(dict(outer,outside_network_namespace='net:[3]'),inner,'net:[2]','net:[1]')

    def test_inspector_and_typed_observations_must_match(self):
        outer,inner=self.observations()
        for changed in [dict(inner,inspector='127.0.0.1:54321'),dict(inner,kind='other')]:
            with self.assertRaises(ValueError):validate_flatpak_transport(outer,changed,'net:[2]','net:[1]')
        with self.assertRaises(ValueError):validate_flatpak_transport(dict(outer,product_acceptance_claimed=True),inner,'net:[2]','net:[1]')


class WebViewFailureDiagnostics(unittest.TestCase):
    def setUp(self):
        temporary=tempfile.TemporaryDirectory(prefix='dezirclip-webview-pure-')
        self.addCleanup(temporary.cleanup);self.root=Path(temporary.name)
        self.plan={'source_revision':REVISION,'target':'linux-x86_64'}
        self.driver=MagicMock();self.driver.route='session/synthetic-session';self.driver.port=12345
        self.driver.__enter__.return_value=self.driver
        self.driver.__exit__.return_value=False
        self.request=MagicMock(side_effect=[['synthetic-window'],'synthetic-window'])

    def fail_enter(self):
        original=RuntimeError('Actual Desktop WebView did not initialize')
        self.driver.__enter__.side_effect=original
        return original

    def retained(self):
        return json.loads((self.root/'desktop-webview-failure.log').read_text(encoding='utf-8'))

    def test_success_keeps_original_build_info_and_context_cleanup_without_diagnostics(self):
        self.driver.invoke.return_value=info('desktop',self.plan['target'])
        result=desktop_build_info(self.driver,self.root,self.plan,self.request)
        self.assertEqual(result,self.driver.invoke.return_value)
        self.driver.invoke.assert_called_once_with('get_build_info')
        self.driver.__exit__.assert_called_once();self.driver.close.assert_not_called()
        self.driver.execute.assert_not_called();self.request.assert_not_called()
        self.assertFalse((self.root/'desktop-webview-failure.log').exists())

    def test_failed_enter_retains_only_metadata_and_rethrows_the_original_exception(self):
        original=self.fail_enter()
        self.driver.execute.return_value={'url':'tauri://localhost/index.html','readyState':'complete',
            'invokeType':'undefined','bodyTextLength':0,'rootChildElementCount':0,
            'body':'synthetic content omitted','environment':{'token':'synthetic content omitted'}}
        with self.assertRaises(RuntimeError) as caught:
            desktop_build_info(self.driver,self.root,self.plan,self.request)
        self.assertIs(caught.exception,original)
        record=self.retained()
        self.assertEqual(record['page'],{key:self.driver.execute.return_value[key] for key in
            ('url','readyState','invokeType','bodyTextLength','rootChildElementCount')})
        self.assertEqual(record['window_handles'],['synthetic-window'])
        self.assertEqual(record['current_window_handle'],'synthetic-window')
        self.assertEqual(record['diagnostic_errors'],{})
        self.assertFalse(record['product_acceptance_claimed'])
        self.assertEqual(record['status'],'diagnostic_only')
        self.assertNotIn('synthetic content omitted',json.dumps(record))
        self.driver.execute.assert_called_once_with(WEBVIEW_DIAGNOSTIC_SCRIPT)
        self.assertEqual(self.request.call_args_list[0].args,(12345,'session/synthetic-session/window/handles'))
        self.assertEqual(self.request.call_args_list[1].args,(12345,'session/synthetic-session/window'))
        self.driver.close.assert_called_once();self.driver.__exit__.assert_not_called()

    def test_collection_and_cleanup_errors_never_replace_the_failed_enter_exception(self):
        original=self.fail_enter();self.driver.execute.side_effect=ValueError('synthetic page body omitted')
        self.request.side_effect=[OSError('synthetic response omitted'),RuntimeError('synthetic response omitted')]
        self.driver.close.side_effect=OSError('synthetic cleanup failed')
        with self.assertRaises(RuntimeError) as caught:
            desktop_build_info(self.driver,self.root,self.plan,self.request)
        self.assertIs(caught.exception,original)
        record=self.retained()
        self.assertEqual(record['diagnostic_errors'],{'page':'ValueError','window_handles':'OSError',
                                                     'current_window_handle':'RuntimeError'})
        self.assertNotIn('omitted',json.dumps(record))
        self.driver.close.assert_called_once()

    def test_retention_failure_does_not_replace_original_exception_or_skip_cleanup(self):
        original=self.fail_enter()
        with patch('release_identity_linux.retain_webview_failure',side_effect=OSError('synthetic disk failure')):
            with self.assertRaises(RuntimeError) as caught:
                desktop_build_info(self.driver,self.root,self.plan,self.request)
        self.assertIs(caught.exception,original);self.driver.close.assert_called_once()

    def test_metadata_bounds_exclude_document_content_and_limit_handles(self):
        self.driver.execute.return_value={'url':'data:text/html,synthetic document omitted','readyState':'x'*200,
            'invokeType':'x'*200,'bodyTextLength':True,'rootChildElementCount':-1,'html':'synthetic document omitted'}
        self.request.side_effect=[['x'*1000]*30,'x'*1000]
        retain_webview_failure(self.driver,self.root,self.plan,RuntimeError(),self.request)
        record=self.retained();page=record['page']
        self.assertEqual(page['url'],'data:[omitted]');self.assertEqual(len(page['readyState']),32)
        self.assertEqual(len(page['invokeType']),16);self.assertIsNone(page['bodyTextLength'])
        self.assertIsNone(page['rootChildElementCount']);self.assertEqual(len(record['window_handles']),16)
        self.assertTrue(record['window_handles_truncated'])
        self.assertTrue(all(len(handle)==256 for handle in record['window_handles']))
        self.assertEqual(len(record['current_window_handle']),256)
        self.assertLessEqual((self.root/'desktop-webview-failure.log').stat().st_size,16*1024)
        self.assertNotIn('synthetic document omitted',json.dumps(record))
        (self.root/'desktop-webview-failure.log').unlink()
        self.driver.execute.return_value['url']='tauri://localhost/'+('界\n"\\'*3000)
        self.request.side_effect=[['界\n"\\'*1000]*30,'界\n"\\'*1000]
        retain_webview_failure(self.driver,self.root,self.plan,RuntimeError(),self.request)
        record=self.retained()
        self.assertNotIn('\n',record['page']['url'])
        self.assertLessEqual(len(record['page']['url'].encode('utf-8')),2048)
        self.assertTrue(all(len(handle.encode('utf-8'))<=256 for handle in record['window_handles']))
        self.assertLessEqual((self.root/'desktop-webview-failure.log').stat().st_size,16*1024)

    def test_missing_session_does_not_query_and_post_enter_failure_uses_context_exit(self):
        self.driver.route=None
        retain_webview_failure(self.driver,self.root,self.plan,RuntimeError(),self.request)
        self.driver.execute.assert_not_called();self.request.assert_not_called()
        self.assertFalse(self.retained()['session_established'])
        (self.root/'desktop-webview-failure.log').unlink()
        original=ValueError('synthetic build metadata refusal');self.driver.invoke.side_effect=original
        with self.assertRaises(ValueError) as caught:
            desktop_build_info(self.driver,self.root,self.plan,self.request)
        self.assertIs(caught.exception,original);self.driver.__exit__.assert_called_once()
        self.driver.close.assert_not_called();self.assertFalse((self.root/'desktop-webview-failure.log').exists())


if __name__=='__main__':unittest.main()
