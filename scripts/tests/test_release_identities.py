"""Synthetic release receipt validation only; no product, installer or clipboard runs."""
import copy
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
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import release_identities as identity
import release_identity_guard as guard
from release_identity_merge import merge
from release_identity_packages import squashfs_offset
from release_identity_packages import windows_environment
from release_identity_packages import remove_owned_tree
from release_identity_linux import flatpak_bindings
from release_identity_linux import sandbox_info

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
    def test_disposable_userns_setup_refuses_local_execution_and_checks_readback(self):
        blocks=[]
        for name in ('ci.yml','release-identities.yml'):
            source=(identity.ROOT/'.github/workflows'/name).read_text(encoding='utf-8')
            step=source.split('      - name: Permit private user namespaces on disposable Linux runner\n',1)[1].split('\n      - name:',1)[0]
            blocks.append(textwrap.dedent(step.split('        run: |\n',1)[1]))
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
                    private=logs/'private-ss';private.mkdir();(private/'proof.json').write_text(json.dumps({'status':'passed'}))
                    (logs/'actual-identities.json').write_text(json.dumps({'credential_probe':{'status':'passed'},'build_info':{role:info(role,args.target) for role in roles}}))
                with patch('release_identity_guard.install',return_value={'synthetic':True}),patch('release_identity_linux.run',side_effect=assert_mount) as execute:
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
            self.assertTrue(Path(env['APPDATA']).is_relative_to(Path(temp)))
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

if __name__=='__main__':unittest.main()
