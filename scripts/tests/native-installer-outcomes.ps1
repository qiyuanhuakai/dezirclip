$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../test-native-windows-upgrade.ps1'
$tokens = $null; $parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
$functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Run-Installer' }, $true))
if ($functions.Count -ne 1) { throw 'Expected exactly one installer outcome function.' }
# Import only the controller declaration; all process operations below are mocks.
. ([scriptblock]::Create($functions[0].Extent.Text))
function Resolve-RunnerFixturePath([string]$Path, [switch]$RequireFile) {
    if ($Path -cne $script:syntheticPath -or -not $RequireFile) { throw 'Unexpected pure fixture input.' }
    return $Path
}
function Save-RunEvidence { $script:saveCalls++ }
function Start-Process([string]$FilePath, [string[]]$ArgumentList, [string]$WindowStyle, [switch]$PassThru) {
    if ($FilePath -cne $script:syntheticPath -or $WindowStyle -cne 'Hidden' -or -not $PassThru -or @($ArgumentList).Count -ne 1 -or $ArgumentList[0] -cne '/S') { throw 'Unexpected process mock input.' }
    $script:launches++
    if ($script:launchFailure) { throw 'Synthetic launch failure.' }
    return $script:processMock
}
function Assert-Equal($Actual, $Expected) { if ($Actual -cne $Expected) { throw "Expected '$Expected', received '$Actual'." } }
$directory = Join-Path ([System.IO.Path]::GetTempPath()) ('dzc-installer-outcomes-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($directory) | Out-Null
$script:syntheticPath = Join-Path $directory 'input.bin'
[System.IO.File]::WriteAllText($script:syntheticPath, 'synthetic non-executable fixture')
$digest = (Get-FileHash -LiteralPath $script:syntheticPath -Algorithm SHA256).Hash
try {
    $cases = @(
        @{ name = 'ordinary success'; exit = 0; expectedFailure = $false; accepted = $true },
        @{ name = 'ordinary failure remains failed'; exit = 2; expectedFailure = $false; accepted = $false },
        @{ name = 'required-file failure is retained as expected'; exit = 2; expectedFailure = $true; accepted = $true },
        @{ name = 'ordinary NSIS cancellation is retained as expected'; exit = 1; expectedFailure = $true; accepted = $true },
        @{ name = 'silent success cannot satisfy negative gate'; exit = 0; expectedFailure = $true; accepted = $false },
        @{ name = 'signed heap corruption cannot satisfy negative gate'; exit = -1073740940; expectedFailure = $true; accepted = $false },
        @{ name = 'unsigned heap corruption cannot satisfy negative gate'; exit = 3221226356; expectedFailure = $true; accepted = $false },
        @{ name = 'signed access violation cannot satisfy negative gate'; exit = -1073741819; expectedFailure = $true; accepted = $false },
        @{ name = 'unsigned access violation cannot satisfy negative gate'; exit = 3221225477; expectedFailure = $true; accepted = $false },
        @{ name = 'unknown exit cannot satisfy negative gate'; exit = 3; expectedFailure = $true; accepted = $false },
        @{ name = 'reboot status cannot satisfy negative gate'; exit = 3010; expectedFailure = $true; accepted = $false },
        @{ name = 'timeout never satisfies negative gate'; exit = 2; expectedFailure = $true; accepted = $false; timeout = $true },
        @{ name = 'launch error never satisfies negative gate'; exit = 2; expectedFailure = $true; accepted = $false; launchFailure = $true },
        @{ name = 'wrong installer hash never launches'; exit = 2; expectedFailure = $true; accepted = $false; wrongHash = $true }
    )
    foreach ($case in $cases) {
        $script:runEvidence = [ordered]@{ installers = @() }
        $script:saveCalls = 0; $script:launches = 0; $script:launchFailure = [bool]$case.launchFailure
        $script:processMock = [pscustomobject]@{ Id = 42; ExitCode = $case.exit; HasExited = -not [bool]$case.timeout; Timeout = [bool]$case.timeout; Killed = $false }
        $script:processMock | Add-Member ScriptMethod WaitForExit { param($Milliseconds) return -not $this.Timeout }
        $script:processMock | Add-Member ScriptMethod Kill { param($EntireTree) $this.Killed = $true; $this.HasExited = $true }
        $accepted = $true
        $expectedDigest = if ($case.wrongHash) { '0' * 64 } else { $digest }
        try { Run-Installer -Path $script:syntheticPath -Arguments @('/S') -ExpectedSha256 $expectedDigest -ExpectFailure:$case.expectedFailure }
        catch { $accepted = $false }
        Assert-Equal $accepted $case.accepted
        if ($case.wrongHash) {
            Assert-Equal $script:launches 0
            Assert-Equal $runEvidence.installers.Count 0
        } else {
            Assert-Equal $script:launches 1
            Assert-Equal $runEvidence.installers.Count 1
            $record = $runEvidence.installers[0]
            Assert-Equal $record.status $(if ($case.accepted) { 'passed' } else { 'failed' })
            Assert-Equal $record.expectedFailure $case.expectedFailure
            if (-not $record.startedAt -or -not $record.finishedAt -or $script:saveCalls -lt 2) { throw 'Outcome receipt lifecycle was lost.' }
            if (-not $case.timeout -and -not $case.launchFailure) { Assert-Equal $record.exitCode $case.exit }
            if (-not $case.accepted -and -not $record.error) { throw 'Original failure was not retained.' }
            Assert-Equal $script:processMock.Killed ([bool]$case.timeout)
        }
        Write-Output ('Passed: ' + $case.name)
    }
} finally {
    [System.IO.File]::Delete($script:syntheticPath)
    [System.IO.Directory]::Delete($directory, $false)
}
Write-Output "$($cases.Count) installer controller checks passed using mocked processes and synthetic bytes; no installer, ACL, registry, GUI or Windows process APIs were invoked."
