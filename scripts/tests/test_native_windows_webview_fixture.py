"""Pure source/fake-clock checks for the disposable NSIS WebView fixture."""
import json
from pathlib import Path
import re
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
BUILD = ROOT / "scripts/build-native-windows-fixture.ps1"
UPGRADE = ROOT / "scripts/test-native-windows-upgrade.ps1"
OWNERSHIP = ROOT / "scripts/tests/native-windows-ownership.mjs"


def restore_source():
    source = UPGRADE.read_text(encoding="utf-8")
    return source[source.index("function Restore-FixtureWebViewEnvironmentEntry("):source.index("function Get-OriginalGuiHint(")]


class SourceRestorePolicy:
    """A source-constrained Env-provider model; it does not execute PowerShell."""
    def __init__(self, source):
        self.paths = dict(re.findall(r"'(WEBVIEW2_[A-Z_]+)' \{ '(Env:WEBVIEW2_[A-Z_]+)' \}", source))
        self.set_command = re.search(r"(?m)^\s*(Set-Item) -LiteralPath \$environmentPath -Value \$Prior.value -ErrorAction Stop$", source)
        self.remove_command = re.search(r"(?m)^\s*(Remove-Item) -LiteralPath \$environmentPath -ErrorAction Stop$", source)
        if not self.set_command or not self.remove_command or "SetEnvironmentVariable" in source:
            raise ValueError("The source must use the explicit Env-provider policy.")
        if "if ($Prior.existed)" not in source or "elseif (Test-Path -LiteralPath $environmentPath)" not in source:
            raise ValueError("Original presence and current presence must select the operation.")

    def restore(self, entries, name, prior, fail_operation=None):
        if name not in self.paths:
            raise ValueError("Unknown environment entry")
        path = self.paths[name]
        if type(prior["existed"]) is not bool or (prior["existed"] and type(prior["value"]) is not str):
            raise ValueError("Invalid original entry")
        operation = self.set_command[1] if prior["existed"] else self.remove_command[1] if path in entries else None
        if operation == fail_operation and operation is not None:
            raise OSError("Synthetic provider failure")
        if operation == "Set-Item":
            entries[path] = prior["value"]
        elif operation == "Remove-Item":
            del entries[path]
        if (path in entries) != prior["existed"] or (prior["existed"] and entries[path] != prior["value"]):
            raise ValueError("Original environment was not restored")
        return operation


class WebViewFixtureSourceTests(unittest.TestCase):
    def test_build_keeps_production_window_browser_arguments(self):
        source = BUILD.read_text(encoding="utf-8")
        self.assertNotIn("additionalBrowserArgs", source)
        self.assertIn("windows=$configuration.app.windows", source)

    def test_same_scenario_user_data_is_outside_uploaded_upgrade_root(self):
        source = UPGRADE.read_text(encoding="utf-8")
        self.assertIn("'dezirclip-upgrade-' + $scenarioId", source)
        self.assertIn("'dezirclip-nsis-webview-' + $scenarioId", source)
        self.assertEqual(source.count("$webviewDirectory ="), 1)
        self.assertIn("$profileDirectory = Join-Path $fixtureRoot 'profile'", source)
        launch = source[source.index("function Invoke-ActualGui("):source.index("function Assert-Uninstalled {")]
        self.assertEqual(launch.count("Start-Process -FilePath"), 1)
        self.assertIn("if (-not $script:webviewReuseReady)", launch)
        self.assertIn("$waitConfirmed -and $final.hasExited -eq $true -and $portFree", launch)
        deletions = re.findall(r"(?m)^\s*Remove-Item[^\n]*", restore_source() + launch)
        self.assertEqual([line.strip() for line in deletions], ["Remove-Item -LiteralPath $environmentPath -ErrorAction Stop"])
        self.assertNotRegex("\n".join(deletions), r"-Recurse|-Force|profile|\.db|\*|\?")
        self.assertNotIn("reset", launch.lower())

    def test_only_known_runtime_environment_is_scoped_to_the_one_launch(self):
        source = UPGRADE.read_text(encoding="utf-8")
        helper = source[source.index("function New-FixtureWebViewEnvironment("):source.index("function Get-OriginalGuiHint(")]
        self.assertEqual(set(re.findall(r"WEBVIEW2_[A-Z_]+", helper)), {
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "WEBVIEW2_USER_DATA_FOLDER"})
        self.assertIn('(Get-Item -LiteralPath ("Env:" + $key)).Value', source)
        self.assertIn('$existed = Test-Path -LiteralPath ("Env:" + $key)', source)
        self.assertIn("existed = $existed; value =", source)
        self.assertIn("Restore-FixtureWebViewEnvironmentEntry $key $previousEnvironment[$key]", source)
        self.assertIn("The original environment-entry presence was not restored.", source)
        self.assertIn("The original environment-entry value was not restored.", source)
        launch = source[source.index("function Invoke-ActualGui("):source.index("function Assert-Uninstalled {")]
        self.assertLess(launch.index("$application = Start-Process"), launch.index("$identity = Wait-ActualProcessImage"))
        self.assertIn("if ($application) {", launch)

    def test_env_provider_restores_absent_empty_and_text_in_the_fake_model(self):
        policy = SourceRestorePolicy(restore_source())
        self.assertEqual(policy.paths, {
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": "Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_USER_DATA_FOLDER": "Env:WEBVIEW2_USER_DATA_FOLDER"})
        for name, path in policy.paths.items():
            for value in (None, "", "synthetic-original"):
                for initial in ({}, {path: "synthetic-override"}):
                    with self.subTest(name=name, value=value, initial=initial):
                        entries = {**initial, "Env:UNRELATED_SYNTHETIC": "unchanged"}
                        policy.restore(entries, name, {"existed": value is not None, "value": value})
                        self.assertEqual(path in entries, value is not None)
                        if value is not None:
                            self.assertEqual(entries[path], value)
                        self.assertEqual(entries["Env:UNRELATED_SYNTHETIC"], "unchanged")
        with self.assertRaisesRegex(ValueError, "Unknown environment"):
            policy.restore({}, "UNRELATED_SYNTHETIC", {"existed": False, "value": None})

    def test_env_provider_failures_and_legacy_null_regression(self):
        policy = SourceRestorePolicy(restore_source())
        for name, path in policy.paths.items():
            for prior, operation in (({"existed": False, "value": None}, "Remove-Item"),
                                     ({"existed": True, "value": ""}, "Set-Item"),
                                     ({"existed": True, "value": "synthetic-original"}, "Set-Item")):
                entries = {path: "synthetic-override"}
                with self.assertRaisesRegex(OSError, "Synthetic provider failure"):
                    policy.restore(entries, name, prior, fail_operation=operation)
                self.assertEqual(entries[path], "synthetic-override")
            self.assertIsNone(policy.restore({}, name, {"existed": False, "value": None}))
            legacy_after_null_binding = {path: ""}
            self.assertNotEqual(path in legacy_after_null_binding, False)
            policy.restore(legacy_after_null_binding, name, {"existed": False, "value": None})
            self.assertNotIn(path, legacy_after_null_binding)
        legacy_source = restore_source().replace(
            "Remove-Item -LiteralPath $environmentPath -ErrorAction Stop",
            "[Environment]::SetEnvironmentVariable($Name, $null, 'Process')")
        with self.assertRaisesRegex(ValueError, "explicit Env-provider"):
            SourceRestorePolicy(legacy_source)

    def test_cloud_env_regression_is_guarded_bounded_and_has_no_launch(self):
        source = UPGRADE.read_text(encoding="utf-8")
        start = source.index("function Assert-FixtureWebViewEnvironmentRestoration {")
        end = source.index("function Convert-GuiConfigurationJson(")
        cloud = source[start:end]
        guard = cloud.index("requires a disposable GitHub-hosted Windows runner")
        self.assertLess(guard, cloud.index("$original = @{}"))
        self.assertIn("$env:GITHUB_ACTIONS -ne 'true'", cloud)
        self.assertIn("$env:RUNNER_ENVIRONMENT -ne 'github-hosted'", cloud)
        self.assertIn("$env:OS -ne 'Windows_NT'", cloud)
        self.assertIn("$PSVersionTable.PSVersion -lt [version]'7.5'", cloud)
        self.assertIn("powershellVersion = $PSVersionTable.PSVersion.ToString()", cloud)
        self.assertEqual(set(re.findall(r"label = '(absent|empty|text)'", cloud)), {"absent", "empty", "text"})
        self.assertIn("foreach ($mode in @('normal', 'synthetic-exception'))", cloud)
        self.assertIn("$receipt.cases.Count -ne 12", cloud)
        self.assertIn("finally {", cloud)
        self.assertEqual(cloud.count("Restore-FixtureWebViewEnvironmentEntry $restoreName $original[$restoreName]"), 1)
        self.assertEqual(cloud.count("Restore-FixtureWebViewEnvironmentEntry $name $original[$name]"), 1)
        self.assertNotRegex(cloud, r"(?m)^\s*(?:Start-Process|Start-Sleep|Remove-Item|&\s+node)\b")
        self.assertIn("The synthetic exception is not an actual Start-Process failure.", cloud)
        self.assertIn("if (-not $caseError) { $caseError = $_ }", cloud)
        self.assertIn("if (-not $firstError) { $firstError = $_ }", cloud)
        main = source[source.index("\ntry {\n    Assert-FixtureWebViewEnvironmentRestoration"):]
        self.assertLess(main.index("Assert-FixtureWebViewEnvironmentRestoration"), main.index("Initialize-SharedProfile"))
        self.assertLess(main.index("Assert-FixtureWebViewEnvironmentRestoration"), main.index("Run-Installer"))

    def test_hints_use_only_original_process_and_first_error_survives_cleanup(self):
        source = UPGRADE.read_text(encoding="utf-8")
        helper = source[source.index("function Get-OriginalGuiHint("):source.index("function Convert-GuiConfigurationJson(")]
        self.assertNotIn("Get-Process", helper)
        self.assertNotIn("Get-CimInstance", helper)
        self.assertIn("$Application.StartTime", helper)
        self.assertIn("$Application.MainWindowHandle", helper)
        self.assertIn("zero main-window handle does not establish", helper)
        launch = source[source.index("function Invoke-ActualGui("):source.index("function Assert-Uninstalled {")]
        self.assertIn("'before-cdp-discovery'", launch)
        self.assertIn("'after-discovery-before-cleanup'", launch)
        self.assertIn("'after-original-cleanup'", launch)
        self.assertIn("catch { if (-not $primaryError) { $primaryError = $_ } }", launch)
        self.assertLess(launch.rindex("if ($primaryError) { throw $primaryError }"), launch.rindex("if ($cleanupErrors.Count)"))
        self.assertIn("$application.WaitForExit(20000)", launch)
        self.assertIn("$attempt -lt 50", launch)

    @unittest.skipUnless(shutil.which("node"), "Node is needed for pure fake-clock checks")
    def test_discovery_budget_and_phases_with_fake_clock(self):
        source = OWNERSHIP.read_text(encoding="utf-8")
        start = source.index("export const ownershipDiscoveryPolicy")
        end = source.index("// This bridge observes")
        pure = source[start:end].replace("export ", "")
        program = r"""
const assert = require('node:assert/strict');
const vm = require('node:vm');
const scope = { assert }; vm.createContext(scope);
vm.runInContext(PURE + ';this.policy=ownershipDiscoveryPolicy;', scope);
assert.equal(scope.policy.maximumAttempts,180);
assert.equal(scope.policy.requestTimeoutMs,1500);
assert.equal(scope.policy.pauseMs,250);
assert.equal(scope.policy.totalBudgetMs,315000);
assert.equal(scope.ownershipDiscoveryBudget(10,10,0).active,true);
assert.equal(scope.ownershipDiscoveryBudget(10,315010,0).active,false);
assert.equal(scope.ownershipDiscoveryBudget(10,11,180).active,false);
let budget=scope.ownershipDiscoveryBudget(10,314910,1);
assert.equal(budget.requestTimeoutMs,100); assert.equal(budget.pauseMs,100);
assert.throws(()=>scope.ownershipDiscoveryBudget(10,9,1));
assert.throws(()=>scope.ownershipDiscoveryBudget(10,10,-1));
assert.equal(scope.ownershipDiscoveryStage({cause:{code:'ECONNREFUSED'}},undefined,false),'tcp-refused');
assert.equal(scope.ownershipDiscoveryStage({name:'TimeoutError'},undefined,false),'request-aborted');
assert.equal(scope.ownershipDiscoveryStage(new Error('body'),200,false),'request-or-body-failed');
assert.equal(scope.ownershipDiscoveryStage(null,200,false),'http-without-main-page');
assert.equal(scope.ownershipDiscoveryStage(null,200,true),'http-main-page');
"""
        program = program.replace("PURE", json.dumps(pure))
        result = subprocess.run([shutil.which("node"), "-e", program], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ownershipDiscoveryBudget(discoveryStarted, completed, attempt).active", source)
        self.assertIn('observation.stage = "main-page-after-deadline"', source)
        self.assertEqual(source.count('fetch(`http://127.0.0.1:${port}/json/list`'), 1)

    @unittest.skipUnless(shutil.which("node"), "Node is needed for pure fake discovery checks")
    def test_actual_discovery_loop_has_no_late_acceptance_or_extra_attempts(self):
        source = OWNERSHIP.read_text(encoding="utf-8")
        pure = source[source.index("export const ownershipDiscoveryPolicy"):source.index("// This bridge observes")]
        pure = pure.replace("export ", "")
        loop = source[source.index("  const discoveryStarted = performance.now();"):source.index("  evidence.page = page;")]
        program = r"""
const assert = require('node:assert/strict');
const vm = require('node:vm');
const target = {type:'page',url:'http://tauri.localhost/',webSocketDebuggerUrl:'ws://127.0.0.1:9222/only-fake'};
async function scenario(mode) {
  let clock=0, calls=0; const pauses=[], timeouts=[], evidence={observations:[]};
  const scope={assert,evidence,port:9222,page:undefined,
    performance:{now:()=>clock},
    AbortSignal:{timeout:ms=>{timeouts.push(ms); return 'fake-signal';}},
    delay:async ms=>{pauses.push(ms);clock+=ms;},
    fetch:async()=>{calls++;
      if(mode==='refused'){throw {cause:{code:'ECONNREFUSED'},toString:()=> 'synthetic-refused'};}
      clock += mode==='late' ? 315000 : mode==='slow' ? 1500 : 1;
      return {status:200,json:async()=> mode==='early'||mode==='late' ? [target] : []};
    }};
  vm.createContext(scope);
  try {await vm.runInContext(PURE+';(async()=>{'+LOOP+';return page;})()',scope);}
  catch(error){assert.match(String(error),/did not expose its main WebView/);}
  return {calls,clock,pauses,timeouts,evidence,page:scope.page};
}
(async()=>{
  let value=await scenario('early');
  assert.equal(value.calls,1);assert.equal(value.page,target);assert.equal(value.pauses.length,0);
  assert.equal(value.evidence.observations[0].stage,'http-main-page');
  value=await scenario('late');
  assert.equal(value.calls,1);assert.equal(value.page,undefined);assert.equal(value.pauses.length,0);
  assert.equal(value.evidence.observations[0].stage,'main-page-after-deadline');
  value=await scenario('refused');
  assert.equal(value.calls,180);assert.equal(value.pauses.length,179);assert.equal(value.clock,44750);
  assert(value.evidence.observations.every(row=>row.stage==='tcp-refused'));
  value=await scenario('slow');
  assert.equal(value.calls,180);assert.equal(value.pauses.length,179);assert.equal(value.clock,314750);
  assert(value.timeouts.every(ms=>ms>0&&ms<=1500));assert(value.pauses.every(ms=>ms>=0&&ms<=250));
})().catch(error=>{console.error(error);process.exitCode=1;});
"""
        program = program.replace("PURE", json.dumps(pure)).replace("LOOP", json.dumps(loop))
        result = subprocess.run([shutil.which("node"), "-e", program], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

