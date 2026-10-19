param(
    [ValidateSet('OldToNew', 'LegacyUninstall', 'OwnedUpgrade', 'OwnedExternalEdit')][string]$Scenario = 'OldToNew',
    [string]$OldInstaller = (Join-Path $PSScriptRoot 'DezirClip_0.1.20_x64-setup.exe'),
    [string]$NewInstaller,
    [string]$PreviousCandidate
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or -not $env:RUNNER_TEMP) {
    throw 'This acceptance script is restricted to a fresh GitHub-hosted disposable Windows runner.'
}
if ($env:OS -ne 'Windows_NT' -or (Get-Process dezirclip -ErrorAction SilentlyContinue)) {
    throw 'A fresh Windows runner without a running DezirClip is required.'
}
function Get-ReceiptCount {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\io.github.qiyuanhuakai.dezirclip\SystemSettingsOwnership\v1')
    try { if ($key) { return $key.GetValueNames().Count }; return 0 } finally { if ($key) { $key.Dispose() } }
}
if ((Get-ReceiptCount) -ne 0) {
    throw 'Existing DezirClip ownership is present; use a fresh runner.'
}
$owned = $Scenario -in @('OwnedUpgrade', 'OwnedExternalEdit')
if (-not $owned -and (Get-FileHash -LiteralPath $OldInstaller -Algorithm SHA256).Hash -ne 'D20033124C74BEA451301716AFB46623607602BAE37647748AE7D25AB7DD4B8E') {
    throw 'The old installer is not the verified published v0.1.20 x64 artifact.'
}
if ($Scenario -ne 'LegacyUninstall' -and ([string]::IsNullOrWhiteSpace($NewInstaller) -or -not (Test-Path -LiteralPath $NewInstaller -PathType Leaf))) {
    throw 'Supply the actual new NSIS candidate installer.'
}
$fixtureRoot = Join-Path $env:RUNNER_TEMP ('dezirclip-upgrade-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$installDirectory = Join-Path $fixtureRoot 'installed'
$targets = @(
    @('Software/Microsoft/Clipboard', 'EnableClipboardHistory'),
    @('Software/Microsoft/Clipboard', 'EnableCloudClipboard'),
    @('Software/Microsoft/Windows/CurrentVersion/Explorer/Advanced', 'DisabledHotkeys'),
    @('Software/Microsoft/Windows/CurrentVersion/Policies/Explorer', 'DisallowClipboardHistory'),
    @('Software/Policies/Microsoft/Windows/System', 'AllowClipboardHistory'),
    @('Software/Policies/Microsoft/Windows/System', 'AllowCrossDeviceClipboard')
)
function Snapshot-Settings {
    foreach ($target in $targets) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($target[0].Replace('/', '\'))
        try {
            $exists = $null -ne $key -and $key.GetValueNames().Contains($target[1])
            [pscustomobject]@{
                Key = $target[0]; Name = $target[1]; Exists = $exists
                Kind = $(if ($exists) { $key.GetValueKind($target[1]).ToString() } else { $null })
                Value = $(if ($exists) { $key.GetValue($target[1], $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } else { $null })
            }
        } finally { if ($key) { $key.Dispose() } }
    }
}
function Run-Installer([string]$Path, [string[]]$Arguments) {
    $process = Start-Process -FilePath (Resolve-Path -LiteralPath $Path).Path -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(120000)) { throw 'Installer timed out; preserve the runner for diagnostics.' }
    if ($process.ExitCode -ne 0) { throw "Installer failed with $($process.ExitCode)." }
}
function Assert-CurrentHistory {
    $version = & (Join-Path $installDirectory 'dzc.exe') --version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch '0\.2\.0-alpha\.1') { throw 'Current CLI sidecar has the wrong version.' }
    $version | Set-Content (Join-Path $fixtureRoot 'new-cli-version.txt')
    $history = & (Join-Path $installDirectory 'dzc.exe') search nativeupgradefixturemarker --metadata --json
    if ($LASTEXITCODE -ne 0 -or $history -notmatch 'nativeupgradefixturemarker') { throw 'Current CLI did not preserve the existing history.' }
    $history | Set-Content (Join-Path $fixtureRoot 'new-history.json')
}
$initial = if ($Scenario -eq 'OwnedUpgrade') { $PreviousCandidate } elseif ($owned) { $NewInstaller } else { $OldInstaller }
if ([string]::IsNullOrWhiteSpace($initial) -or -not (Test-Path -LiteralPath $initial -PathType Leaf)) { throw 'Initial installer fixture is missing.' }
Run-Installer $initial @('/S', "/D=$installDirectory")
if (-not (Test-Path -LiteralPath (Join-Path $installDirectory 'dezirclip.exe'))) { throw 'Old desktop was not installed.' }
if (-not (Test-Path -LiteralPath (Join-Path $installDirectory 'dzc.exe'))) { throw 'Old CLI sidecar was not installed.' }
$env:DEZIRCLIP_DB_PATH = Join-Path $fixtureRoot 'history.db'
& (Join-Path $installDirectory 'dzc.exe') add 'nativeupgradefixturemarker' | Set-Content (Join-Path $fixtureRoot 'old-add.txt')
if ($LASTEXITCODE -ne 0) { throw 'Old CLI fixture initialization failed.' }
for ($index = 0; $index -lt $targets.Count; $index++) {
    $target = $targets[$index]
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($target[0].Replace('/', '\'))
    try {
        if ($index -eq 2) { $key.SetValue($target[1], $(if ($owned) { 'X' } else { 'XV' }), [Microsoft.Win32.RegistryValueKind]::String) }
        else { $key.SetValue($target[1], 7, [Microsoft.Win32.RegistryValueKind]::DWord) }
    } finally { $key.Dispose() }
}
$before = @(Snapshot-Settings)
$beforeJson = $before | ConvertTo-Json -Depth 6
$beforeJson | Set-Content (Join-Path $fixtureRoot 'settings-before.json')
$explorerBefore = @(Get-Process explorer -ErrorAction SilentlyContinue | Select-Object Id,StartTime)
if ($owned) {
    $port = 9222
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $listener.Stop()
    $application = Start-Process -FilePath (Join-Path $installDirectory 'dezirclip.exe') -WorkingDirectory $installDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $fixtureRoot 'application-stdout.txt') -RedirectStandardError (Join-Path $fixtureRoot 'application-stderr.txt')
    try {
        & node (Join-Path $PSScriptRoot 'tests/native-windows-ownership.mjs') $port (Join-Path $fixtureRoot 'cdp-observations.json')
        if ($LASTEXITCODE -ne 0) { throw 'Actual application ownership commands failed.' }
    } finally {
        $application.Refresh()
        @{Id=$application.Id; HasExited=$application.HasExited; ExitCode=$(if ($application.HasExited) { $application.ExitCode } else { $null }); Port=$port} |
            ConvertTo-Json | Set-Content (Join-Path $fixtureRoot 'application-process.json')
        Get-Content (Join-Path $fixtureRoot 'application-stderr.txt') -ErrorAction SilentlyContinue | Write-Output
        Get-Content (Join-Path $fixtureRoot 'application-stdout.txt') -Tail 60 -ErrorAction SilentlyContinue | Write-Output
        Get-Content (Join-Path $fixtureRoot 'application-process.json') | Write-Output
        Get-Content (Join-Path $fixtureRoot 'cdp-observations.json') -Tail 40 -ErrorAction SilentlyContinue | Write-Output
        if (-not $application.HasExited) { Stop-Process -Id $application.Id -Force }
    }
    $acquired = @(Snapshot-Settings)
    if ($acquired[0].Value -ne 0 -or $acquired[1].Value -ne 0 -or $acquired[2].Value -ne 'XV') { throw 'Application did not persist expected owned values.' }
    if ((Get-ReceiptCount) -ne 3) { throw 'Application did not persist all three ownership receipts.' }
    if ($Scenario -eq 'OwnedUpgrade') {
        Run-Installer $NewInstaller @('/S', '/UPDATE', "/D=$installDirectory")
        if (($acquired | ConvertTo-Json -Depth 6) -ne (@(Snapshot-Settings) | ConvertTo-Json -Depth 6)) { throw 'Upgrade changed owned values.' }
        if ((Get-ReceiptCount) -ne 3) { throw 'Upgrade removed ownership receipts.' }
    } else {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Clipboard')
        try { $key.SetValue('EnableClipboardHistory', 9, [Microsoft.Win32.RegistryValueKind]::DWord) } finally { $key.Dispose() }
    }
    Assert-CurrentHistory
    Run-Installer (Join-Path $installDirectory 'uninstall.exe') @('/S', "_?=$installDirectory")
    $restored = @(Snapshot-Settings)
    if ($Scenario -eq 'OwnedExternalEdit') { $before[0].Value = 9; $beforeJson = $before | ConvertTo-Json -Depth 6 }
    if ($beforeJson -ne ($restored | ConvertTo-Json -Depth 6)) { throw 'Owned uninstall did not restore the exact typed original values or preserve the newer external edit.' }
    if ((Get-ReceiptCount) -ne 0) { throw 'Uninstall left ownership receipts.' }
    $restored | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'settings-restored.json')
    Write-Output "Evidence: $fixtureRoot"
    exit 0
}
if ($Scenario -eq 'LegacyUninstall') {
    Run-Installer (Join-Path $installDirectory 'uninstall.exe') @('/S', "_?=$installDirectory")
} else {
    Run-Installer $NewInstaller @('/S', '/UPDATE', "/D=$installDirectory")
}
$after = @(Snapshot-Settings)
$afterJson = $after | ConvertTo-Json -Depth 6
$afterJson | Set-Content (Join-Path $fixtureRoot 'settings-after.json')
@{Scenario=$Scenario; ExplorerBefore=$explorerBefore; ExplorerAfter=@(Get-Process explorer -ErrorAction SilentlyContinue | Select-Object Id,StartTime)} |
    ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'process-evidence.json')
if ($Scenario -eq 'LegacyUninstall') {
    if ($beforeJson -eq $afterJson) { throw 'Expected the published old hook to demonstrate its destructive reset behavior.' }
    if ($after[0].Value -ne 1 -or $after[1].Value -ne 1 -or $after[2].Value -ne 'X' -or $after[3].Exists -or $after[4].Exists -or $after[5].Exists) {
        throw 'Published old hook behavior differed; retain evidence and investigate.'
    }
} else {
    if ($beforeJson -ne $afterJson) { throw 'Upgrade changed pre-existing Windows settings.' }
    Assert-CurrentHistory
}
Write-Output "Evidence: $fixtureRoot"
Write-Output 'No complete updater download/signature/UI or owned-receipt uninstall test is implied by this scenario.'
