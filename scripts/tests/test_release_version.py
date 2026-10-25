"""Pure version/catalog checks; no release, product or native API execution."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from release_version import release_version, rpm_parts


class ReleaseVersionTests(unittest.TestCase):
    def test_rpm_naming_matches_bundle_rpm_for_supported_releases(self):
        for version, expected in [
            ('0.2.0-alpha.3', ('0.2.0', '0.alpha.3')),
            ('0.2.0-alpha.2', ('0.2.0', '0.alpha.2')),
            ('1.2.3-beta.10+build.4', ('1.2.3', '0.beta.10')),
            ('1.2.3-rc.0', ('1.2.3', '0.rc.0')),
            ('1.2.3', ('1.2.3', '1')),
            ('1.2.3+build.4', ('1.2.3', '1')),
        ]:
            with self.subTest(version=version):
                self.assertEqual(rpm_parts(version), expected)

    def test_unsupported_or_noncanonical_versions_are_rejected(self):
        for version in (None, True, 123, '', 'v0.2.0-alpha.3', '01.2.3', '1.02.3', '1.2.03',
                        '1.2.3-alpha.03', '1.2.3-dev.1', '1.2.3-alpha', '1.2.3-alpha.1\n',
                        '1.2.3+', '1.2.3+bad..metadata', '1.2.3-rc.1+bad_name'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                rpm_parts(version)

    def test_version_only_comes_from_the_requested_checkout(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-version-test-') as temp:
            root = Path(temp)
            (root / 'package.json').write_text(json.dumps({'version': '0.2.0-alpha.3'}), encoding='utf-8')
            with patch.dict(os.environ, {'DZC_RELEASE_VERSION': '0.2.0-alpha.2',
                                         'GITHUB_REF_NAME': 'v0.2.0-alpha.2'}):
                self.assertEqual(release_version(root), '0.2.0-alpha.3')

    def test_missing_or_invalid_manifest_version_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix='dezirclip-version-test-') as temp:
            root = Path(temp)
            with self.assertRaises(FileNotFoundError):
                release_version(root)
            for package in ({}, {'version': True}, {'version': '0.2.0-alpha.03'}, []):
                (root / 'package.json').write_text(json.dumps(package), encoding='utf-8')
                with self.subTest(package=package), self.assertRaises(ValueError):
                    release_version(root)


if __name__ == '__main__':
    unittest.main()
