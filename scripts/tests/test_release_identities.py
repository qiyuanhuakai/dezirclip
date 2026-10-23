"""Synthetic release receipt validation only; no product, installer or clipboard runs."""
import copy
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import release_identities as identity
import release_identity_guard as guard
from release_identity_merge import merge
from release_identity_packages import squashfs_offset
from release_identity_packages import windows_environment
from release_identity_packages import remove_owned_tree

REVISION='a'*40
DIGEST='b'*64

def info(role,target):
    names=['dzc-core','dzc-platform','dzc-protocol','dzc-host']+(['dzc-service'] if role=='service' else [])
    return {'metadata_version':1,'component':role,'component_version':identity.VERSION,'build_id':identity.VERSION+'@'+REVISION,'source_revision':REVISION,'source_state':'verified_checkout','revision_origin':'explicit_checkout','target':identity.TARGETS[target],'protocol_version':{'major':1,'minor':0},'database_schema_version':24,'linked_components':[{'component':name,'version':identity.VERSION} for name in names]}

def report(target):
    standalone,containers=identity.expected_assets(target)
    return {'schema_version':1,'status':'passed','method':'executed_native','tag':'v'+identity.VERSION,'source_revision':REVISION,'target':target,'runner':{'system':'Windows' if target.startswith('windows-') else 'Linux','machine':'ARM64' if target.endswith('aarch64') else 'x86_64'},'job_id':123,'run_url':'https://github.com/qiyuanhuakai/dezirclip/actions/runs/123','standalone':[{'platform':target,'asset':standalone,'asset_sha256':DIGEST,'binary_sha256':DIGEST,'build_info':info('cli',target)}],'containers':[{'platform':target,'asset':name,'asset_sha256':DIGEST,'contents':[{'component':role,'path':'usr/bin/'+binary,'bytes':123,'binary_sha256':DIGEST,'build_info':info(role,target)} for role,binary in identity.ROLES.items()]} for name in containers]}

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
            for index,mutation in enumerate([lambda r:r.update(status='failed'),lambda r:r.update(method='ci_bound_static'),lambda r:r.update(source_revision='d'*40),lambda r:r['runner'].update(machine='x86_64'),lambda r:r['containers'].pop(),lambda r:r['containers'][0]['contents'][0].update(binary_sha256='missing')]):
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
