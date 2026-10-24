import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_identity_inputs as inputs
from release_identity_merge import workflow_identity


SOURCE = '0' * 40
TOOLING = '1' * 40


class InputDiagnosticsTests(unittest.TestCase):
    def test_permission_failure_retains_only_safe_response_fields(self):
        body = json.dumps({'message': 'Resource not accessible by integration', 'status': '403',
                           'documentation_url': 'https://docs.github.com/rest/releases/releases#get-a-release',
                           'token': 'fixture-secret', 'extra': 'not archived'}).encode()
        headers = {'X-GitHub-Request-Id': 'ABCD:12:345', 'X-Accepted-GitHub-Permissions': 'contents=read',
                   'Authorization': 'Bearer fixture-secret', 'Set-Cookie': 'fixture-secret',
                   'Location': 'https://example.invalid/object?signature=fixture-secret'}
        with self.assertRaises(inputs.InputFailure) as caught:
            inputs.metadata_response(403, headers, body, 'fixture-secret')
        details = caught.exception.details
        self.assertEqual(details['http_status'], 403)
        self.assertEqual(details['headers'], {'x-github-request-id': 'ABCD:12:345',
                                             'x-accepted-github-permissions': 'contents=read'})
        self.assertEqual(details['body'], {'message': 'Resource not accessible by integration', 'status': '403'})
        self.assertNotIn('fixture-secret', json.dumps(details))

    def test_error_body_drops_urls_credentials_and_arbitrary_fields(self):
        for message in ['Bearer hidden', 'ghp_fixturesecret', 'github_pat_fixturesecret',
                        'https://example.invalid/signed/object', 'fixture-secret']:
            raw = json.dumps({'message': message, 'status': '403',
                              'documentation_url': 'https://docs.github.com/path?sig=fixture-secret',
                              'errors': ['fixture-secret']}).encode()
            self.assertEqual(inputs.safe_error_body(raw, 403, 'fixture-secret'), {'status': '403'})

    def test_safe_documentation_url_is_bounded_and_official(self):
        valid = 'https://docs.github.com/rest/releases/releases'
        self.assertEqual(inputs.safe_error_body(json.dumps({'documentation_url': valid}).encode(), 403),
                         {'documentation_url': valid})
        for url in ['https://docs.github.com.evil.invalid/path', 'https://docs.github.com/path#fragment',
                    'https://docs.github.com/path?sig=hidden', 'https://docs.github.com/ghp_hidden']:
            self.assertEqual(inputs.safe_error_body(json.dumps({'documentation_url': url}).encode(), 403), {})

    def test_whitelisted_header_cannot_smuggle_secret_or_url(self):
        self.assertEqual(inputs.safe_headers({'X-GitHub-Request-Id': 'fixture-secret',
                                             'Retry-After': 'https://example.invalid/signed',
                                             'Content-Type': 'Bearer hidden'}, 'fixture-secret'), {})

    def test_html_and_oversize_errors_do_not_become_raw_diagnostics(self):
        self.assertEqual(inputs.safe_error_body(b'<html>fixture-secret</html>', 500), {})
        self.assertEqual(inputs.safe_error_body(b'x' * (inputs.MAX_RESPONSE + 1), 502), {})

    def test_success_response_returns_object_without_archiving_body(self):
        value, observation = inputs.metadata_response(200, {'Content-Type': 'application/json'}, b'{"id":422}')
        self.assertEqual(value, {'id': 422})
        self.assertEqual(observation, {'http_status': 200, 'headers': {'content-type': 'application/json'}})
        with self.assertRaises(ValueError):
            inputs.metadata_response(200, {}, b'[]')


class ActualIdentitySelectionTests(unittest.TestCase):
    def test_diagnostics_cannot_create_the_fresh_native_output(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-input-path-test-') as temporary:
            boundary = Path(temporary).resolve()
            output = boundary / 'dezirclip-release-identities-linux-x86_64'
            with self.assertRaises(ValueError):
                inputs.owned_roots(boundary / 'inputs', output, output / 'diagnostics', boundary)
            self.assertFalse(output.exists())

    def test_recovery_selects_real_workflow_head(self):
        target = 'linux-x86_64'
        job = {'id': 12, 'name': 'Collect actual final package identities / Release identities (' + target + ')',
               'runner_name': 'runner-fixture', 'head_sha': TOOLING}
        self.assertEqual(inputs.select_job([job], target, 'runner-fixture', TOOLING), 12)
        with self.assertRaises(ValueError):
            inputs.select_job([job], target, 'runner-fixture', SOURCE)
        with self.assertRaises(ValueError):
            inputs.select_job([job, job], target, 'runner-fixture', TOOLING)

    def test_catalog_requires_complete_original_draft_assets(self):
        target = 'linux-x86_64'
        archive, packages = inputs.expected_assets(target)
        names = [archive, archive + '.sha256', *packages]
        assets = [{'name': name, 'id': index + 1, 'size': 100} for index, name in enumerate(names)]
        release = {'id': 422, 'draft': True, 'tag_name': 'v' + inputs.VERSION, 'assets': assets}
        self.assertEqual([value['name'] for value in inputs.requested_assets(release, target, 422, release['tag_name'])], names)
        with self.assertRaises(ValueError):
            inputs.requested_assets(dict(release, draft=False), target, 422, release['tag_name'])
        with self.assertRaises(ValueError):
            inputs.requested_assets(dict(release, assets=assets[:-1]), target, 422, release['tag_name'])
        with self.assertRaises(ValueError):
            inputs.requested_assets(dict(release, assets=assets + [assets[0]]), target, 422, release['tag_name'])

    def test_recovery_provenance_requires_original_matching_authority(self):
        authority = {'schema_version': 1, 'mode': 'recovery', 'source_revision': SOURCE,
                     'workflow_source_revision': TOOLING, 'tag': 'v' + inputs.VERSION,
                     'release_id': 422, 'run_id': '123', 'run_attempt': '2'}
        record = {'workflow_source_revision': TOOLING, 'source_authority': authority,
                  'run_url': 'https://github.com/qiyuanhuakai/dezirclip/actions/runs/123'}
        self.assertEqual(workflow_identity(record, SOURCE, TOOLING, authority), TOOLING)
        for changed in [dict(record, workflow_source_revision=SOURCE),
                        dict(record, source_authority=dict(authority, run_attempt='1')),
                        dict(record, run_url='https://github.com/qiyuanhuakai/dezirclip/actions/runs/999')]:
            with self.assertRaises(ValueError):
                workflow_identity(changed, SOURCE, TOOLING, authority)
        with self.assertRaises(ValueError):
            workflow_identity(record, SOURCE, TOOLING)

    def test_legacy_tag_metadata_cannot_stand_in_for_recovery(self):
        self.assertEqual(workflow_identity({}, SOURCE, SOURCE), SOURCE)
        with self.assertRaises(ValueError):
            workflow_identity({}, SOURCE, TOOLING)


if __name__ == '__main__':
    unittest.main()
