param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ('dzc-process-identity-' + [guid]::NewGuid().ToString('N')) }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Identity tests require a new output directory.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$scriptPath = Join-Path $repository 'scripts/test-native-windows-upgrade.ps1'
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
$functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-ActualProcessImage' }, $true))
if ($functions.Count -ne 1) { throw 'Expected exactly one identity decision function.' }
# Evaluate only the pure injectable decision function, never the guarded
# installer script body or the native process-query adapter.
. ([scriptblock]::Create($functions[0].Extent.Text))
$evidence = [ordered]@{ schemaVersion = 1; verificationOnly = $true; processLaunched = $false; nativeProcessQueryUsed = $false; scriptSha256 = (Get-FileHash -LiteralPath $scriptPath -Algorithm SHA256).Hash; status = 'running'; tests = @() }
function Save-Evidence { $evidence | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'evidence.json') -Encoding utf8 }
function Assert-Equal($Actual, $Expected) { if ($Actual -cne $Expected) { throw "Expected '$Expected', received '$Actual'." } }
function Check([string]$Name, [scriptblock]$Body) {
    try { $record = & $Body; $evidence.tests += @{ name = $Name; status = 'passed'; observations = $record } }
    catch { $evidence.tests += @{ name = $Name; status = 'failed'; error = $_.Exception.Message }; throw }
    finally { Save-Evidence }
}
function Snapshot($Path, $Exited = $false, [string]$ErrorText = '') {
    return [pscustomobject]@{ processId = 1234; observedPath = $Path; hasExited = $Exited; error = $ErrorText; pathSource = 'injected fixture' }
}
function Invoke-IdentitySequence([object[]]$Values, [int]$Attempts = 3) {
    $state = [pscustomobject]@{ queue = [System.Collections.Generic.Queue[object]]::new(); reads = 0; delays = 0 }
    foreach ($value in $Values) { $state.queue.Enqueue($value) }
    $read = {
        $state.reads++
        $value = $state.queue.Dequeue()
        if ($value -is [System.Exception]) { throw $value }
        return $value
    }.GetNewClosure()
    $delay = { param($Milliseconds) $state.delays++ }.GetNewClosure()
    $result = Wait-ActualProcessImage -ExpectedPath 'C:\fixture\installed\dezirclip.exe' -ReadProcess $read -Delay $delay -MaximumAttempts $Attempts
    return [pscustomobject]@{ result = $result; reads = $state.reads; delays = $state.delays }
}
$expected = 'C:\fixture\installed\dezirclip.exe'
Save-Evidence
try {
    Check 'null path then actual expected image succeeds with retained first observation' {
        $run = Invoke-IdentitySequence @((Snapshot $null $false 'temporarily unavailable'), (Snapshot $expected))
        Assert-Equal $run.result.verified $true
        Assert-Equal $run.result.reason 'identity_verified'
        Assert-Equal $run.reads 2
        Assert-Equal $run.delays 1
        Assert-Equal $run.result.attempts[0].observedPath $null
        Assert-Equal $run.result.attempts[0].error 'temporarily unavailable'
        Assert-Equal $run.result.attempts[1].observedPath $expected
        $run
    }
    Check 'persistently empty image path is bounded and rejected' {
        $run = Invoke-IdentitySequence @((Snapshot ''), (Snapshot ''), (Snapshot ''))
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'image_path_unavailable'
        Assert-Equal $run.reads 3
        Assert-Equal $run.delays 2
        $run
    }
    Check 'a nonempty wrong image fails immediately even if a later value would match' {
        $run = Invoke-IdentitySequence @((Snapshot 'C:\other\dezirclip.exe'), (Snapshot $expected))
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'unexpected_image_path'
        Assert-Equal $run.reads 1
        Assert-Equal $run.delays 0
        $run
    }
    Check 'process exit rejects an otherwise matching path immediately' {
        $run = Invoke-IdentitySequence @((Snapshot $expected $true), (Snapshot $expected))
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'process_exited'
        Assert-Equal $run.reads 1
        Assert-Equal $run.delays 0
        $run
    }
    Check 'temporarily unreadable query preserves the error before later actual verification' {
        $run = Invoke-IdentitySequence @([System.InvalidOperationException]::new('injected query unavailable'), (Snapshot $expected))
        Assert-Equal $run.result.verified $true
        Assert-Equal $run.reads 2
        if ($run.result.attempts[0].error -notmatch 'injected query unavailable') { throw 'Query error was lost.' }
        $run
    }
    Check 'matching image without a reliable alive state is rejected' {
        $run = Invoke-IdentitySequence @((Snapshot $expected $null), (Snapshot $expected $null), (Snapshot $expected $null))
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'process_state_unavailable'
        Assert-Equal $run.reads 3
        $run
    }
    Check 'case differences use Windows ordinal path comparison' {
        $run = Invoke-IdentitySequence @((Snapshot $expected.ToUpperInvariant()))
        Assert-Equal $run.result.verified $true
        Assert-Equal $run.reads 1
        Assert-Equal $run.delays 0
        $run
    }
    Check 'an error cannot turn a nonempty wrong image into a retry' {
        $run = Invoke-IdentitySequence @((Snapshot 'C:\other\dezirclip.exe' $false 'access error'), (Snapshot $expected))
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'unexpected_image_path'
        Assert-Equal $run.reads 1
        $run
    }
    Check 'a supplied expected launch filename is never identity evidence' {
        $snapshot = [pscustomobject]@{ processId = 1234; observedPath = $null; hasExited = $false; startInfoFileName = $expected; error = '' }
        $run = Invoke-IdentitySequence @($snapshot, $snapshot, $snapshot)
        Assert-Equal $run.result.verified $false
        Assert-Equal $run.result.reason 'image_path_unavailable'
        Assert-Equal $run.reads 3
        $run
    }
    $evidence.status = 'passed'
} catch { $evidence.status = 'failed'; $evidence.error = $_.Exception.Message; throw }
finally { Save-Evidence; Write-Output "Pure injected evidence: $OutputDirectory" }
