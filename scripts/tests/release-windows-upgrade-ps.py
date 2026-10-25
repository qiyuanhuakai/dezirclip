"""Static contract checks only; never imports or runs the native acceptance program."""
import ast
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/test-release-windows-upgrade.ps1'
SOURCE = SCRIPT.read_text(encoding='utf-8')
WORKFLOW = (ROOT / '.github/workflows/release-windows-upgrade.yml').read_text(encoding='utf-8')
PROFILE = SOURCE.split("$profileProgram = @'\n", 1)[1].split("\n'@", 1)[0]
TREE = ast.parse(PROFILE)
CHECKS = (
    'original_installer_hash', 'candidate_signature_and_hash', 'actual_old_install', 'actual_old_profile_created',
    'actual_upgrade_same_directory', 'installed_three_role_bytes', 'actual_three_role_build_info', 'history_and_schema24',
    'frozen_preferences_preserved', 'windows_settings_preserved', 'explorer_not_restarted', 'controlled_processes_exited',
    'actual_release_uninstall', 'uninstall_preserves_profile_and_settings',
)
FROZEN = {'app.quick_paste_hotkey', 'app.classification_enabled', 'app.hidden_filter_chips', 'app.screenshot_enabled', 'app.ocr_enabled'}


class UpgradeSourceContract(unittest.TestCase):
    def test_parameters_and_disposable_native_source_guards_precede_execution(self):
        parameters = re.findall(r'\[string\]\$(\w+)', SOURCE.split('$ErrorActionPreference', 1)[0])
        self.assertEqual(parameters, ['FixtureDirectory', 'ExpectedSource', 'ExpectedReleaseId', 'ExpectedPolicySha256'])
        guard = SOURCE.split('function Resolve-OwnedPath', 1)[0]
        for item in ('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'GITHUB_REPOSITORY', 'GITHUB_REF', 'GITHUB_SHA',
                     'ProcessArchitecture', 'OSArchitecture', 'PROCESSOR_ARCHITEW6432', 'rev-parse HEAD', '--untracked-files=no'):
            self.assertIn(item, guard)
        self.assertIn("^dezirclip-upgrade-[a-f0-9]{32}$", SOURCE)
        self.assertIn('ReparsePoint', SOURCE)

    def test_exact_fourteen_checks_are_recorded_once(self):
        names = SOURCE.split('$checkNames = @(', 1)[1].split('\n$raw =', 1)[0]
        self.assertEqual(tuple(re.findall("'([a-z0-9_]+)'", names)), CHECKS)
        invocations = re.findall(r"Pass-Check '([a-z0-9_]+)'", SOURCE)
        self.assertEqual(set(invocations), set(CHECKS))
        self.assertEqual(len(invocations), len(CHECKS))
        self.assertIn("kind = 'release_windows_upgrade_native'", SOURCE)
        self.assertIn("$raw.status = 'failed'", SOURCE)
        self.assertIn("$_.status -cne 'passed'", SOURCE)

    def test_controller_contract_keeps_policy_verification_and_frozen_sealing(self):
        verify = SOURCE.index("Invoke-Controller 'verify-staged'")
        extract = SOURCE.index("Invoke-Controller 'extract'")
        install = SOURCE.index("'install-original-published'")
        seal = SOURCE.index("Invoke-Controller 'seal'")
        self.assertLess(verify, extract)
        self.assertLess(extract, install)
        self.assertLess(SOURCE.index('if ($failure) { throw $failure }'), seal)
        self.assertIn("($Command -cne 'seal')", SOURCE)
        self.assertIn('if ($Entry.RecordObservation) { Save-Raw }', SOURCE)
        self.assertIn("if ($Policy) { $arguments += $ExpectedPolicySha256 }", SOURCE)

    def test_final_member_bytes_are_compared_without_rebuilding_or_token_patches(self):
        self.assertIn("'cli,desktop,service'", SOURCE)
        self.assertIn('$record.bytes -ne $expected.bytes', SOURCE)
        self.assertIn('$record.sha256 -cne $expected.sha256', SOURCE)
        self.assertIn('$raw.roles.desktop = $observed.gui.buildInfo', SOURCE)
        self.assertIn('acquireOwnership = $false; verifyCapture = $false', SOURCE)
        for prohibited in ('__TAURI_BUNDLE_TYPE_VAR_', 'build-native-windows', 'cargo build', 'test-native-windows-upgrade.ps1'):
            self.assertNotIn(prohibited, SOURCE)

    def test_original_process_objects_are_retained_and_children_drop_credentials(self):
        for required in ('$start.Environment.Clear()', 'ProcessWindowStyle]::Hidden', '$start.CreateNoWindow = $true',
                         '$entry.Handle = $process.SafeHandle', '$Entry.Process.Kill($true)', '$Entry.Process.WaitForExit(20000)',
                         '$entry.Process.HasExited', '$raw.gui_exit', 'cdp_port_released = $portFree'):
            self.assertIn(required, SOURCE)
        self.assertNotIn('Stop-Process', SOURCE)
        environment = SOURCE.split('function New-ChildEnvironment', 1)[1].split('function Start-OwnedProcess', 1)[0]
        self.assertNotIn('GH_TOKEN', environment)
        self.assertNotIn('ACTIONS_RUNTIME_TOKEN', environment)

    def test_historical_package_and_actual_database_identity_are_required(self):
        for required in ('d20033124c74bea451301716afb46623607602bae37647748ae7d25ab7dd4b8e',
                         '31d8b88064e1185c9f74b5fdd327902b9bc860e523b9a5ac0d71620ec7adbb87',
                         '90ba9960744b6cd57caaaf597f68533a7e41be45',
                         '9e96fcd49e4e36c903edd19a1424856383e229ea131f455fbabdcf816622db05',
                         '037f40cf014cd4bb3dce4a0a0c5cbcaee23affae', '$oldInfo.database_schema_version -ne $oldSchema',
                         '@($oldInfo.linked_components).Count -ne $oldLinked.Count', 'original_installer_hash', 'old-cli-create-profile'):
            self.assertIn(required, SOURCE)
        self.assertIn("assert database.is_file() and not database.is_symlink()", PROFILE)
        self.assertIn("assert version==21", PROFILE)
        self.assertIn("assert version==24", PROFILE)
        self.assertIn("assert entry_id==before['marker']['id']", PROFILE)
        self.assertIn('connection.backup(backup)', PROFILE)
        self.assertIn("$oldSchema = if ($inputs.scenario -ceq 'PublishedAlpha2ToRelease') { 24 } else { 21 }", SOURCE)
        self.assertIn("if ($inputs.scenario -ceq 'PublishedAlpha2ToRelease') { $oldLinked += 'dzc-host' }", SOURCE)
        self.assertIn("if inputs['scenario']=='PublishedAlpha2ToRelease': assert version==24", PROFILE)
        self.assertIn('assert 0<version<=21', PROFILE)

    def test_embedded_python_parses_and_limits_mutation_to_seed(self):
        parents = {child: parent for parent in ast.walk(TREE) for child in ast.iter_child_nodes(parent)}
        for node in ast.walk(TREE):
            if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute) or node.func.attr not in ('execute', 'executemany'):
                continue
            if not node.args or not isinstance(node.args[0], ast.Constant) or not isinstance(node.args[0].value, str):
                continue
            sql = node.args[0].value.lstrip().upper()
            if not sql.startswith(('INSERT', 'UPDATE', 'DELETE', 'CREATE', 'DROP', 'ALTER', 'REPLACE')):
                continue
            current = parents.get(node)
            while current and not (isinstance(current, ast.If) and ast.unparse(current.test) == "operation == 'seed'"):
                current = parents.get(current)
            self.assertIsNotNone(current, sql)
        self.assertIn("'rw' if operation=='seed' else 'ro'", PROFILE)

    def test_five_preferences_pin_and_tag_survive_upgrade_and_uninstall(self):
        frozen = next(node.value for node in TREE.body if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == 'frozen' for target in node.targets))
        self.assertEqual(set(ast.literal_eval(frozen)), FROZEN)
        self.assertIn("row[4]==1 and json.loads(row[3])==['releaseupgradetag']", PROFILE)
        self.assertIn("all(settings.get(key)==value for key,value in frozen.items())", PROFILE)
        self.assertIn("Invoke-ProfileCheck 'check' 'upgraded-profile-check'", SOURCE)
        self.assertIn("Invoke-ProfileCheck 'check' 'uninstalled-profile-check'", SOURCE)
        self.assertIn('registered_installation_absent = $true', SOURCE)
        self.assertLess(SOURCE.index("if ($explorerBefore.Count -eq 0) { throw"), SOURCE.index("'install-final-release-upgrade'"))

    def test_workflow_is_reusable_only_and_keeps_each_scenario_disposable(self):
        self.assertIn('on:\n  workflow_call:', WORKFLOW)
        self.assertNotIn('pull_request:', WORKFLOW)
        self.assertNotIn('\n  push:', WORKFLOW)
        self.assertIn('contents: read', WORKFLOW)
        self.assertIn('scenario: [LegacyToAlpha2, PublishedAlphaToAlpha2, PublishedAlpha2ToRelease]', WORKFLOW)
        self.assertIn("python-version: '3.13.15'", WORKFLOW)
        self.assertIn('sqlite3.sqlite_version_info < (3, 50, 4)', WORKFLOW)
        self.assertEqual(WORKFLOW.count('GH_TOKEN:'), 2)
        native = WORKFLOW.split('- name: Execute the actual final NSIS upgrade and uninstall', 1)[1].split('- name: Preserve original native observations', 1)[0]
        self.assertNotIn('GH_TOKEN', native)
        self.assertIn('pattern: release-windows-upgrade-*To*-attempt-', WORKFLOW)
        self.assertIn('if: always()', WORKFLOW)

    def test_candidate_version_is_from_package_and_recovery_stays_fixed(self):
        self.assertIn("(Join-Path $repository 'package.json')", SOURCE)
        self.assertNotIn("$version = '0.2.0-alpha.2'", SOURCE)
        self.assertIn("if ($version -cne '0.2.0-alpha.2'", SOURCE)
        self.assertIn("$ExpectedReleaseId -cne '422045565'", SOURCE)


if __name__ == '__main__':
    unittest.main(verbosity=2)
