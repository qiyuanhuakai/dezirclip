"""Resolve actual native receipt inputs and retain bounded preflight diagnostics."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

from release_identities import ROOT, TARGETS, VERSION, expected_assets, native_target, require, sha

REPOSITORY = 'qiyuanhuakai/dezirclip'
MAX_RESPONSE = 2 * 1024 * 1024
HEADER_NAMES = {'content-type', 'x-github-request-id', 'x-accepted-github-permissions',
                'x-ratelimit-limit', 'x-ratelimit-remaining', 'x-ratelimit-reset',
                'x-ratelimit-used', 'x-ratelimit-resource', 'retry-after',
                'x-github-api-version-selected'}


def safe_message(value, token=''):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9 .,;:'()/_-]{1,256}", value):
        return None
    if '://' in value or (token and token in value) or re.search(r'github_pat_|gh[pousr]_|authorization|bearer|cookie', value, re.I):
        return None
    return value


def safe_headers(headers, token=''):
    result = {}
    if not hasattr(headers, 'items'):
        return result
    for key, value in headers.items():
        key = key.lower()
        if key in HEADER_NAMES and isinstance(value, str) and 0 < len(value) <= 256:
            if re.fullmatch(r'[A-Za-z0-9 .,:;=_/-]+', value) and '://' not in value \
                    and not (token and token in value) \
                    and not re.search(r'github_pat_|gh[pousr]_|authorization|bearer|cookie', value, re.I):
                result[key] = value
    return result


def safe_error_body(raw, status, token=''):
    if len(raw) > MAX_RESPONSE:
        return {}
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeError):
        return {}
    if not isinstance(value, dict):
        return {}
    result = {}
    message = safe_message(value.get('message'), token)
    if message:
        result['message'] = message
    if str(value.get('status')) == str(status):
        result['status'] = str(status)
    documentation = value.get('documentation_url')
    if isinstance(documentation, str) and len(documentation) <= 512:
        try:
            parsed = urlsplit(documentation)
        except ValueError:
            return result
        if parsed.scheme == 'https' and parsed.netloc == 'docs.github.com' and not parsed.query and not parsed.fragment \
                and not (token and token in documentation) \
                and not re.search(r'github_pat_|gh[pousr]_', documentation):
            result['documentation_url'] = documentation
    return result


class InputFailure(RuntimeError):
    def __init__(self, message, status=None, headers=None, body=None):
        super().__init__(message)
        self.details = {'category': message, 'http_status': status,
                        'headers': headers or {}, 'body': body or {}}


def metadata_response(status, headers, raw, token=''):
    retained_headers = safe_headers(headers, token)
    if status != 200:
        raise InputFailure('MetadataRequestFailed', status, retained_headers, safe_error_body(raw, status, token))
    require(len(raw) <= MAX_RESPONSE, 'Metadata response exceeds bound')
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeError) as error:
        raise InputFailure('MetadataResponseInvalid', status, retained_headers) from error
    require(isinstance(value, dict), 'Metadata object required')
    return value, {'http_status': status, 'headers': retained_headers}


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def metadata(route, token):
    pattern = r'repos/' + REPOSITORY + r'/(releases/[0-9]+|actions/runs/[0-9]+/attempts/[0-9]+/jobs\?per_page=100)'
    require(re.fullmatch(pattern, route) is not None, 'Fixed repository metadata route required')
    request = Request('https://api.github.com/' + route, headers={
        'Accept': 'application/vnd.github+json', 'Authorization': 'Bearer ' + token,
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'DezirClip-release-receipts'})
    try:
        with build_opener(NoRedirect()).open(request, timeout=60) as response:
            return metadata_response(response.status, response.headers, response.read(MAX_RESPONSE + 1), token)
    except HTTPError as error:
        return metadata_response(error.code, error.headers, error.read(MAX_RESPONSE + 1), token)
    except (URLError, TimeoutError) as error:
        raise InputFailure('MetadataTransportFailed') from error


def select_job(jobs, target, runner_name, workflow_source):
    require(isinstance(jobs, list), 'Actual job metadata required')
    matches = [job for job in jobs if isinstance(job, dict)
               and isinstance(job.get('name'), str)
               and job['name'].endswith('Release identities (' + target + ')')
               and job.get('runner_name') == runner_name
               and job.get('head_sha') == workflow_source]
    require(len(matches) == 1 and type(matches[0].get('id')) is int and matches[0]['id'] > 0,
            'Unique actual native job identity required')
    return matches[0]['id']


def requested_assets(release, target, release_id, tag):
    require(release.get('id') == release_id and type(release.get('id')) is int
            and release.get('draft') is True and release.get('tag_name') == tag,
            'Expected original draft release')
    archive, packages = expected_assets(target)
    result = []
    for name in [archive, archive + '.sha256', *packages]:
        matches = [asset for asset in release.get('assets', []) if isinstance(asset, dict) and asset.get('name') == name]
        require(len(matches) == 1, 'Missing or duplicate final asset: ' + name)
        asset = matches[0]
        require(type(asset.get('id')) is int and asset['id'] > 0
                and type(asset.get('size')) is int and asset['size'] > 0, 'Actual asset identity required')
        result.append(asset)
    return result


def new_owned_path(value, boundary):
    value = value.absolute()
    require('\n' not in str(value) and '\r' not in str(value), 'Single-line owned path required')
    require(value.resolve().is_relative_to(boundary) and value.resolve() != boundary and not value.exists(),
            'New owned path beneath RUNNER_TEMP required')
    current = value
    while current.resolve() != boundary:
        require(not current.is_symlink(), 'Owned path must not traverse links')
        require(current.parent != current, 'Owned path must have RUNNER_TEMP as an ancestor')
        current = current.parent
    return value.resolve()


def environment_values(values):
    require(all('\n' not in str(value) and '\r' not in str(value) for value in values.values()),
            'Single-line workflow environment values required')
    with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8', newline='\n') as output:
        for key, value in values.items():
            output.write(key + '=' + str(value) + '\n')
        output.flush()
        os.fsync(output.fileno())


def owned_roots(input_path, output_path, diagnostics_path, boundary):
    inputs = new_owned_path(input_path, boundary)
    output = new_owned_path(output_path, boundary)
    diagnostics = new_owned_path(diagnostics_path, boundary)
    require(inputs != output and not inputs.is_relative_to(output) and not output.is_relative_to(inputs),
            'Separate input and output roots required')
    require(output.name.startswith('dezirclip-release-identities-'), 'Original native output name required')
    require(not diagnostics.is_relative_to(inputs) and not diagnostics.is_relative_to(output)
            and not inputs.is_relative_to(diagnostics) and not output.is_relative_to(diagnostics),
            'Separate diagnostic root required')
    return inputs, output, diagnostics


def diagnostic(directory, record):
    descriptor, temporary = tempfile.mkstemp(prefix='.preflight-', dir=directory)
    with os.fdopen(descriptor, 'wb') as output:
        output.write((json.dumps(record, indent=2) + '\n').encode('utf-8'))
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, directory / 'input-preflight.json')


def download(asset, directory, token):
    command = ['gh', 'release', 'download', 'v' + VERSION, '--repo', REPOSITORY,
               '--pattern', asset['name'], '--dir', str(directory)]
    try:
        result = subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, timeout=300,
                                env=dict(os.environ, GH_TOKEN=token))
    except (OSError, subprocess.TimeoutExpired) as error:
        raise InputFailure('AssetDownloadUnavailable') from error
    if result.returncode:
        text = (result.stdout + result.stderr)[:MAX_RESPONSE].decode('utf-8', errors='replace')
        matches = re.findall(r'HTTP\s+([1-5][0-9]{2})\b', text)
        status = int(matches[-1]) if matches else None
        raise InputFailure('AssetDownloadFailed', status)
    path = directory / asset['name']
    require(path.is_file() and not path.is_symlink() and path.stat().st_size == asset['size'],
            'Downloaded asset size or ownership differs')
    digest = sha(path)
    published = asset.get('digest')
    if isinstance(published, str) and published.startswith('sha256:'):
        require(published == 'sha256:' + digest, 'Downloaded asset digest differs')
    return {'name': asset['name'], 'id': asset['id'], 'size': asset['size'], 'sha256': digest}


def resolve(args):
    require(re.fullmatch('[0-9a-f]{40}', args.source) is not None and args.release_id > 0,
            'Exact source and numeric release ID required')
    require(os.environ.get('GITHUB_ACTIONS') == 'true'
            and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'Disposable Actions runner required')
    boundary = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
    inputs, output, diagnostics = owned_roots(args.input, args.output, args.diagnostics, boundary)
    diagnostics.mkdir(mode=0o700, parents=True)
    token = os.environ.get('GH_TOKEN', '')
    record = {'schema_version': 1, 'kind': 'release_identity_input_preflight', 'status': 'running',
              'stage': 'environment', 'source_revision': args.source, 'target': args.platform,
              'release_id': args.release_id, 'started_at': datetime.now(timezone.utc).isoformat(),
              'requests': [], 'downloads': []}
    diagnostic(diagnostics, record)
    try:
        environment_values({'DZC_IN': inputs, 'DZC_RECEIPT_ASSETS': inputs,
                            'DZC_RECEIPT_OUT': output, 'DZC_RECEIPT_JOB_ID': '',
                            'DZC_RECEIPT_DIAGNOSTICS': diagnostics})
        record['stage'] = 'authority'
        diagnostic(diagnostics, record)
        from release_recovery import authority
        provenance = authority(args.source, ROOT)
        require(provenance['source_revision'] == args.source and provenance['tag'] == 'v' + VERSION
                and provenance['release_id'] in (None, args.release_id), 'Release authority differs')
        require(provenance['run_id'] == os.environ.get('GITHUB_RUN_ID')
                and provenance['run_attempt'] == os.environ.get('GITHUB_RUN_ATTEMPT')
                and re.fullmatch('[1-9][0-9]*', provenance['run_id'] or '')
                and re.fullmatch('[1-9][0-9]*', provenance['run_attempt'] or ''), 'Actual run identity required')
        require(os.environ.get('GITHUB_REPOSITORY') == REPOSITORY and token, 'Repository token required')
        native_target(args.platform)
        record['authority'] = provenance
        for stage, route in [('release_metadata', f'repos/{REPOSITORY}/releases/{args.release_id}'),
                             ('job_metadata', f'repos/{REPOSITORY}/actions/runs/{provenance["run_id"]}/attempts/{provenance["run_attempt"]}/jobs?per_page=100')]:
            record['stage'] = stage
            record['route'] = route
            diagnostic(diagnostics, record)
            value, observation = metadata(route, token)
            record['requests'].append(dict(observation, stage=stage, route=route))
            if stage == 'release_metadata':
                assets = requested_assets(value, args.platform, args.release_id, provenance['tag'])
            else:
                job_id = select_job(value.get('jobs'), args.platform, os.environ['RUNNER_NAME'],
                                    provenance['workflow_source_revision'])
        environment_values({'DZC_RECEIPT_JOB_ID': job_id})
        record['job_id'] = job_id
        inputs.mkdir(mode=0o700, parents=True)
        for asset in assets:
            record['stage'] = 'asset_download'
            record['asset'] = asset['name']
            diagnostic(diagnostics, record)
            record['downloads'].append(download(asset, inputs, token))
        record.update(status='passed', stage='complete')
    except BaseException as error:
        record.update(status='failed', error_type=type(error).__name__)
        if isinstance(error, InputFailure):
            record['error'] = error.details
        else:
            record['error'] = {'category': 'InputPreflightFailed', 'http_status': None,
                               'headers': {}, 'body': {}}
            message = safe_message(str(error), token)
            if message:
                record['error']['body']['message'] = message
        raise
    finally:
        record['completed_at'] = datetime.now(timezone.utc).isoformat()
        diagnostic(diagnostics, record)
        print('DEZIRCLIP_RELEASE_INPUT_PREFLIGHT ' + json.dumps({
            'status': record['status'], 'stage': record['stage'], 'source_revision': args.source,
            'target': args.platform, 'diagnostic': str(diagnostics / 'input-preflight.json')}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True)
    parser.add_argument('--release-id', required=True, type=int)
    parser.add_argument('--platform', required=True, choices=TARGETS)
    parser.add_argument('--input', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--diagnostics', required=True, type=Path)
    resolve(parser.parse_args())


if __name__ == '__main__':
    main()
