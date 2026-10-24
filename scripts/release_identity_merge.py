"""Bind four original native reports to the public acceptance provenance format."""
import argparse
import json
from pathlib import Path
import re
from release_identities import TARGETS, VERSION, expected_assets, require, safe_member, sha, validate_info

def workflow_identity(record, revision, expected_workflow, expected_authority=None):
    workflow = record.get('workflow_source_revision', revision)
    require(isinstance(workflow, str) and re.fullmatch('[0-9a-f]{40}', workflow)
            and workflow == expected_workflow, 'Original workflow source differs')
    original = record.get('source_authority')
    if workflow != revision:
        require(isinstance(original, dict) and isinstance(expected_authority, dict)
                and original.get('mode') == expected_authority.get('mode') == 'recovery',
                'Original recovery workflow authority required')
        require(type(expected_authority.get('schema_version')) is int and expected_authority['schema_version'] == 1
                and expected_authority.get('source_revision') == revision
                and expected_authority.get('workflow_source_revision') == workflow
                and expected_authority.get('tag') == 'v' + VERSION
                and type(expected_authority.get('release_id')) is int and expected_authority['release_id'] > 0
                and isinstance(expected_authority.get('run_id'), str) and re.fullmatch('[1-9][0-9]*', expected_authority['run_id'])
                and isinstance(expected_authority.get('run_attempt'), str) and re.fullmatch('[1-9][0-9]*', expected_authority['run_attempt']),
                'Verified recovery workflow authority required')
        for key in ('schema_version', 'source_revision', 'workflow_source_revision', 'tag', 'release_id', 'run_id', 'run_attempt'):
            require(original.get(key) == expected_authority.get(key), 'Original recovery authority differs: ' + key)
        require(record.get('run_url') == 'https://github.com/qiyuanhuakai/dezirclip/actions/runs/' + original['run_id'],
                'Original recovery run identity differs')
    elif original is not None:
        require(isinstance(original, dict) and original.get('schema_version') == 1
                and original.get('mode') == 'tag' and original.get('source_revision') == revision
                and original.get('workflow_source_revision') == revision and original.get('tag') == 'v' + VERSION,
                'Original tag workflow authority differs')
    return workflow


def merge(paths,revision,workflow_source=None,source_authority=None):
    require(re.fullmatch('[0-9a-f]{40}',revision),'Exact final source required')
    require(len(paths)==4,'Require four original native report files')
    workflow_source = workflow_source or revision
    result={'schema_version':1,'tag':'v'+VERSION,'source_revision':revision,
            'workflow_source_revision':workflow_source,'standalone':[],'containers':[]}
    targets=set()
    for path in paths:
        require(path.is_file() and not path.is_symlink() and path.stat().st_size<=256*1024,'Bounded original report required')
        record=json.loads(path.read_text(encoding='utf-8'))
        target=record.get('target');require(target in TARGETS and target not in targets,'Duplicate or unknown report target');targets.add(target)
        require(type(record.get('schema_version')) is int and record['schema_version']==1 and record.get('status')=='passed' and record.get('method')=='executed_native' and record.get('source_revision')==revision and record.get('tag')=='v'+VERSION,'Original report is not passing exact-source native evidence')
        workflow = workflow_identity(record, revision, workflow_source, source_authority)
        runner=record.get('runner',{});system=runner.get('system','').lower();machine=runner.get('machine','').lower()
        arch={'amd64':'x86_64','x86_64':'x86_64','arm64':'aarch64','aarch64':'aarch64'}.get(machine)
        require(target==f'{system}-{arch}','Original runner does not match target')
        require(type(record.get('job_id')) is int and record['job_id']>0 and re.fullmatch(r'https://github.com/qiyuanhuakai/dezirclip/actions/runs/[0-9]+',record.get('run_url','')),'Original CI identity missing')
        proof={'source_revision':revision,'workflow_source_revision':workflow,
               'workflow_source_revision_origin':'original_native_report' if 'workflow_source_revision' in record else 'legacy_exact_tag_report',
               'method':'executed_native','run_url':record['run_url'],'job_id':record['job_id'],'target':target,'evidence_sha256':sha(path)}
        if target.endswith('aarch64'):proof['arm_evidence']=f'Actual native {runner["system"]} {runner["machine"]} runner; packaged machine checked before execution; actual CLI/Service JSON and Desktop bridge output retained in the original report. No x64 execution is relabelled.'
        standalone,containers=expected_assets(target)
        require(isinstance(record.get('standalone'),list) and len(record['standalone'])==1 and isinstance(record.get('containers'),list) and sorted(x.get('asset','') for x in record['containers'])==sorted(containers),'Original asset inventory differs')
        cli=record['standalone'][0]
        require(cli.get('platform')==target and cli.get('asset')==standalone,'Standalone target/name differs')
        require(cli.get('path')==('dzc.exe' if target.startswith('windows-') else 'dzc')
                and type(cli.get('bytes')) is int and cli['bytes']>0,'Standalone member path/bytes missing')
        validate_info(cli.get('build_info'),'cli',target,revision)
        for value in (cli.get('asset_sha256'),cli.get('binary_sha256')):require(isinstance(value,str) and re.fullmatch('[0-9a-f]{64}',value),'Standalone digest missing')
        result['standalone'].append(dict(cli,proof=proof))
        for container in record['containers']:
            require(container.get('platform')==target and re.fullmatch('[0-9a-f]{64}',container.get('asset_sha256','')),'Container target/digest differs')
            contents=container.get('contents');require(isinstance(contents,list) and len(contents)==3 and sorted(x.get('component','') for x in contents)==['cli','desktop','service'],'Container role inventory differs')
            for member in contents:
                safe_member(member.get('path'));require(type(member.get('bytes')) is int and member['bytes']>0 and re.fullmatch('[0-9a-f]{64}',member.get('binary_sha256','')),'Container member bytes differ')
                validate_info(member.get('build_info'),member['component'],target,revision)
            result['containers'].append(dict(container,proof=proof))
    require(targets==set(TARGETS) and len(result['containers'])==10,'Incomplete final provenance')
    return result

def main():
    from release_recovery import authority
    from release_identities import ROOT
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reports',type=Path,required=True);parser.add_argument('--source',required=True);parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    require(not args.out.exists(),'New aggregate output required')
    provenance=authority(args.source,ROOT)
    value=merge(sorted(args.reports.glob('*/report.json')),args.source,provenance['workflow_source_revision'],provenance);encoded=json.dumps(value,indent=2)+'\n'
    require(len(encoded.encode())<=256*1024,'Aggregate exceeds diagnostic bound');args.out.write_text(encoded,encoding='utf-8')
    print('DEZIRCLIP_RELEASE_PROVENANCE '+json.dumps(value,separators=(',',':')),flush=True)
    print(json.dumps({'status':'passed','provenance_sha256':sha(args.out),'standalone':4,'containers':10,'source_revision':args.source}),flush=True)

if __name__=='__main__':main()
