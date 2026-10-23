# Final Windows release upgrade gate

This gate is prepared until the exact clean `v0.2.0-alpha.2` tag runs the reusable
`release-windows-upgrade.yml` workflow. Static tests and the five existing debug
installer scenarios do not establish acceptance of the final signed NSIS bytes.
The release workflow must wait for this gate before making the draft public.

Preparation verifies all eight signed updater assets and retains a hashed input
policy. Two separate disposable native x64 Windows runners then install the
pinned published `0.1.20` or `0.2.0-alpha.1` installer, create a real historical
profile through its own CLI, and upgrade in the same directory with the original
final installer. The alpha.1 path requires its actual `90ba996` source, schema21
and three-component inventory. Neither path rebuilds a package or changes a
Tauri bundle token.

The gate compares the three installed programs with members extracted from that
same final NSIS archive. CLI and Service metadata come from their real programs;
Desktop metadata, profile identity and rendered retained history come from its
actual WebView. A synthetic pinned/tagged record and all five frozen preferences
must survive schema24 migration and the real final uninstaller. Typed external
clipboard settings and Explorer identities must remain unchanged. Acceptance
requires at least one active original Explorer before the final
upgrade; a headless runner without one fails instead of accepting empty snapshots.
Native clipboard writes and settings ownership acquisition are disabled in the GUI
bridge for these two upgrade scenarios.

All product execution is restricted to the disposable hosted Windows runner.
Each attempt retains its original inputs, raw observations and failure logs;
an incomplete observation cannot be sealed as a passing report. The aggregate
`windows-upgrade.json` binds both scenario reports to the same source, release,
policy and eight asset identities. It does not promote an update channel.

Local preparation parses source only:

```powershell
python -I -B scripts/tests/release-windows-upgrade-ps.py
$tokens = $null; $errors = $null
[System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path scripts/test-release-windows-upgrade.ps1), [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count) { throw ($errors -join "`n") }
```
