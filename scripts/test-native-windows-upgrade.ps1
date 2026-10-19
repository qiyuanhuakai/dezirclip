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
if ($env:OS -ne 'Windows_NT' -or (Get-Process dezirclip, dzc -ErrorAction SilentlyContinue)) {
    throw 'A fresh Windows runner without a running DezirClip is required.'
}
function Resolve-RunnerFixturePath([string]$Path, [switch]$RequireFile) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'Fixture path is missing.' }
    $boundary = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/')
    $full = [System.IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($boundary + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture paths must remain beneath RUNNER_TEMP.' }
    $current = $full
    while ($current.Length -ge $boundary.Length) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { throw 'Fixture paths cannot cross a reparse point.' }
        if ($current -ieq $boundary) { break }
        $current = Split-Path -Parent $current
    }
    if ($RequireFile -and -not (Test-Path -LiteralPath $full -PathType Leaf)) { throw 'Fixture file is missing.' }
    return $full
}
function Get-ReceiptCount {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\io.github.qiyuanhuakai.dezirclip\SystemSettingsOwnership\v1')
    try { if ($key) { return $key.GetValueNames().Count }; return 0 } finally { if ($key) { $key.Dispose() } }
}
if ((Get-ReceiptCount) -ne 0) { throw 'Existing DezirClip ownership is present; use a fresh runner.' }
foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view)
        $existing = $null
        try {
            $existing = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip')
            if ($existing) { throw 'An existing registered DezirClip installation is present; use a fresh runner.' }
        } finally { if ($existing) { $existing.Dispose() }; $base.Dispose() }
    }
}
$owned = $Scenario -in @('OwnedUpgrade', 'OwnedExternalEdit')
if (-not $owned) { $OldInstaller = Resolve-RunnerFixturePath $OldInstaller -RequireFile }
if (-not $owned -and (Get-FileHash -LiteralPath $OldInstaller -Algorithm SHA256).Hash -ne 'D20033124C74BEA451301716AFB46623607602BAE37647748AE7D25AB7DD4B8E') {
    throw 'The old installer is not the verified published v0.1.20 x64 artifact.'
}
function Read-CandidateManifest([string]$Installer) {
    $Installer = Resolve-RunnerFixturePath $Installer -RequireFile
    $manifestPath = Resolve-RunnerFixturePath ($Installer + '.manifest.json') -RequireFile
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schemaVersion -ne 1 -or $manifest.kind -ne 'dezirclip-native-nsis-fixture' -or $manifest.version -notin @('0.2.0-alpha.0', '0.2.0-alpha.1')) { throw 'Candidate manifest is invalid.' }
    if ($manifest.target -ne 'x86_64-pc-windows-msvc' -or $manifest.profile -ne 'debug' -or $manifest.updaterArtifacts -ne $false -or $manifest.sourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Candidate build provenance is invalid.' }
    if ($manifest.sidecarVersion -notmatch ('(?:^|\s)' + [regex]::Escape($manifest.version) + '(?:\s|$)')) { throw 'Candidate manifest CLI version is invalid.' }
    foreach ($digest in @($manifest.installerSha256, $manifest.applicationSha256, $manifest.sidecarSha256, $manifest.cargoLockSha256, $manifest.configurationSha256, $manifest.installerHookSha256)) {
        if ($digest -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Candidate manifest digest is invalid.' }
    }
    if ((Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash -ne $manifest.installerSha256) { throw 'Candidate installer differs from its build manifest.' }
    return $manifest
}
$candidate = if ($Scenario -ne 'LegacyUninstall') { Read-CandidateManifest $NewInstaller } else { $null }
if ($candidate -and $candidate.version -ne '0.2.0-alpha.1') { throw 'Current candidate must be the alpha.1 fixture.' }
$previous = if ($Scenario -eq 'OwnedUpgrade') { Read-CandidateManifest $PreviousCandidate } else { $null }
if ($previous -and $previous.version -ne '0.2.0-alpha.0') { throw 'Previous candidate must be the alpha.0 fixture.' }
$fixtureRoot = Join-Path $env:RUNNER_TEMP ('dezirclip-upgrade-' + [guid]::NewGuid().ToString('N'))
$fixtureRoot = Resolve-RunnerFixturePath $fixtureRoot
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$installDirectory = Join-Path $fixtureRoot 'installed'
$profileDirectory = Join-Path $fixtureRoot 'profile'
New-Item -ItemType Directory -Path $profileDirectory | Out-Null
$previousDbOverride = $env:DEZIRCLIP_DB_PATH
$marker = 'nativeupgradefixturemarker' + [guid]::NewGuid().ToString('N')
if ($candidate) { Copy-Item -LiteralPath ($NewInstaller + '.manifest.json') -Destination (Join-Path $fixtureRoot 'candidate-build.json') }
if ($previous) { Copy-Item -LiteralPath ($PreviousCandidate + '.manifest.json') -Destination (Join-Path $fixtureRoot 'previous-build.json') }
$runEvidence = [ordered]@{ schemaVersion = 1; scenario = $Scenario; status = 'running'; startedAt = [DateTime]::UtcNow.ToString('o'); fixtureRoot = $fixtureRoot; installers = @(); cleanup = @() }
function Save-RunEvidence { $runEvidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $fixtureRoot 'acceptance.json') -Encoding utf8 }
Save-RunEvidence
Write-Output "Evidence: $fixtureRoot"

# Tauri resolves its Windows data directory through the known-folder API. Merely
# changing APPDATA or the CLI-only DB override would not prove a shared database.
$redirects = @()
function Initialize-SharedProfile {
    $roaming = [Environment]::GetFolderPath('ApplicationData')
    if ([string]::IsNullOrWhiteSpace($roaming) -or ((Get-Item -LiteralPath $roaming -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { throw 'A regular fresh RoamingAppData folder is required.' }
    foreach ($identifier in @('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard')) {
        if (Test-Path -LiteralPath (Join-Path $roaming $identifier)) { throw 'Existing GUI data directory is present; use a fresh runner.' }
    }
    foreach ($identifier in @('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard')) {
        $directory = Join-Path $roaming $identifier
        New-Item -ItemType Directory -Path $directory | Out-Null
        $redirect = Join-Path $directory 'datapath.txt'
        $script:redirects += [pscustomobject]@{ identifier = $identifier; directory = $directory; redirect = $redirect; profileDirectory = $profileDirectory }
        [System.IO.File]::WriteAllText($redirect, $profileDirectory, [System.Text.UTF8Encoding]::new($false))
    }
    @{ cliDatabase = $env:DEZIRCLIP_DB_PATH; marker = $marker; redirects = @($redirects) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $fixtureRoot 'shared-profile.json')
}
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
function Snapshot-Explorer {
    foreach ($process in @(Get-Process explorer -ErrorAction SilentlyContinue | Sort-Object Id)) {
        [pscustomobject]@{ Id = $process.Id; StartTime = $process.StartTime.ToUniversalTime().ToString('o') }
    }
}
$explorerBefore = @(Snapshot-Explorer)
function Assert-Explorer([bool]$RequireUnchanged) {
    $after = @(Snapshot-Explorer)
    $unchanged = (ConvertTo-Json -InputObject @($explorerBefore) -Depth 4) -eq (ConvertTo-Json -InputObject @($after) -Depth 4)
    $spawned = @($after | Where-Object { $observed = $_; -not ($explorerBefore | Where-Object { $_.Id -eq $observed.Id -and $_.StartTime -eq $observed.StartTime }) })
    @{ Scenario = $Scenario; ExplorerBefore = $explorerBefore; ExplorerAfter = $after; AssertedUnchanged = $RequireUnchanged; existingExplorerRestartVerified = ($RequireUnchanged -and $unchanged -and $explorerBefore.Count -gt 0); noExplorerSpawned = ($spawned.Count -eq 0) } |
        ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'process-evidence.json')
    if ($RequireUnchanged -and -not $unchanged) {
        throw 'The Explorer process set or start time changed.'
    }
}
function Run-Installer([string]$Path, [string[]]$Arguments, [string]$ExpectedSha256) {
    $resolved = Resolve-RunnerFixturePath $Path -RequireFile
    $digest = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
    if ($ExpectedSha256 -and $digest -ne $ExpectedSha256) { throw 'Installer changed after its provenance was validated.' }
    $record = [ordered]@{ path = $resolved; sha256 = $digest; expectedSha256 = $ExpectedSha256; arguments = $Arguments; startedAt = [DateTime]::UtcNow.ToString('o'); status = 'running' }
    $runEvidence.installers += $record
    Save-RunEvidence
    $process = $null
    try {
        $process = Start-Process -FilePath $resolved -ArgumentList $Arguments -WindowStyle Hidden -PassThru
        $record.processId = $process.Id
        if (-not $process.WaitForExit(120000)) { throw 'Fixture installer timed out.' }
        $record.exitCode = $process.ExitCode
        if ($process.ExitCode -ne 0) { throw "Installer failed with $($process.ExitCode)." }
        $record.status = 'passed'
    } catch { $record.status = 'failed'; $record.error = $_.Exception.Message; throw }
    finally {
        if ($process -and -not $process.HasExited) { $process.Kill($true); $process.WaitForExit(20000) | Out-Null }
        $record.finishedAt = [DateTime]::UtcNow.ToString('o')
        Save-RunEvidence
    }
}
function Assert-InstalledCandidate($Manifest, [string]$Label) {
    $application = Join-Path $installDirectory 'dezirclip.exe'
    $sidecar = Join-Path $installDirectory 'dzc.exe'
    $actual = @{
        version = $Manifest.version
        applicationSha256 = (Get-FileHash -LiteralPath $application -Algorithm SHA256).Hash
        sidecarSha256 = (Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash
    }
    $actual | ConvertTo-Json | Set-Content (Join-Path $fixtureRoot ($Label + '-installed.json'))
    if ($actual.applicationSha256 -ne $Manifest.applicationSha256 -or $actual.sidecarSha256 -ne $Manifest.sidecarSha256) { throw 'Installed GUI or CLI differs from the candidate build.' }
    $version = & $sidecar --version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch ('(?:^|\s)' + [regex]::Escape($Manifest.version) + '(?:\s|$)')) { throw 'Installed CLI has the wrong built-in version.' }
    $version | Set-Content (Join-Path $fixtureRoot ($Label + '-cli-version.txt'))
}
function Assert-CliHistory([string]$Text, [Nullable[long]]$ExpectedId, [string]$Label) {
    $output = & (Join-Path $installDirectory 'dzc.exe') search $Text --json
    if ($LASTEXITCODE -ne 0) { throw 'Installed CLI history query failed.' }
    $output | Set-Content (Join-Path $fixtureRoot ($Label + '-cli-history.json'))
    $entries = @(($output | Out-String | ConvertFrom-Json) | Where-Object { $_.content -eq $Text })
    if ($entries.Count -ne 1 -or ($null -ne $ExpectedId -and $entries[0].id -ne $ExpectedId)) { throw 'CLI and GUI did not observe the same single stored entry.' }
}
function Invoke-ActualGui($Manifest, [bool]$AcquireOwnership, [bool]$VerifyCapture, [string]$Label) {
    Assert-InstalledCandidate $Manifest $Label
    $port = 9222
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
    $listener.Start(); $listener.Stop()
    $configuration = Join-Path $fixtureRoot ($Label + '-gui-input.json')
    @{ expectedVersion = $Manifest.version; profileDirectory = $profileDirectory; marker = $marker; acquireOwnership = $AcquireOwnership; verifyCapture = $VerifyCapture } |
        ConvertTo-Json | Set-Content -LiteralPath $configuration -Encoding utf8
    $evidence = Join-Path $fixtureRoot ($Label + '-gui-evidence.json')
    $stdout = Join-Path $fixtureRoot ($Label + '-stdout.txt')
    $stderr = Join-Path $fixtureRoot ($Label + '-stderr.txt')
    $application = Start-Process -FilePath (Join-Path $installDirectory 'dezirclip.exe') -WorkingDirectory $installDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not [string]::Equals($application.Path, (Join-Path $installDirectory 'dezirclip.exe'), [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Launched GUI process is not the installed candidate.' }
        & node (Join-Path $PSScriptRoot 'tests/native-windows-ownership.mjs') $port $evidence $configuration
        if ($LASTEXITCODE -ne 0) { throw 'Actual GUI profile, version, ownership or native capture acceptance failed.' }
        $observed = Get-Content -LiteralPath $evidence -Raw | ConvertFrom-Json
        Assert-CliHistory $marker $observed.gui.oldMarkerId ($Label + '-retained')
        if ($VerifyCapture) {
            Assert-CliHistory $observed.capture.firstText $observed.capture.firstId ($Label + '-captured-a')
            Assert-CliHistory $observed.capture.secondText $observed.capture.secondId ($Label + '-captured-b')
        }
    } finally {
        $application.Refresh()
        @{ Id = $application.Id; HasExited = $application.HasExited; ExitCode = $(if ($application.HasExited) { $application.ExitCode } else { $null }); Port = $port } |
            ConvertTo-Json | Set-Content (Join-Path $fixtureRoot ($Label + '-process.json'))
        Get-Content -LiteralPath $stderr -Tail 60 -ErrorAction SilentlyContinue | Write-Output
        if (-not $application.HasExited) {
            $application.Kill($true)
            if (-not $application.WaitForExit(20000)) { throw 'Fixture GUI process did not exit.' }
        }
        $portFree = $false
        for ($attempt = 0; $attempt -lt 50; $attempt++) {
            $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
            try { $probe.Start(); $portFree = $true; break }
            catch { Start-Sleep -Milliseconds 100 }
            finally { $probe.Stop() }
        }
        if (-not $portFree) { throw 'Fixture WebView process tree did not release its CDP port.' }
    }
}
function Assert-Uninstalled {
    foreach ($name in @('dezirclip.exe', 'dzc.exe')) {
        if (Test-Path -LiteralPath (Join-Path $installDirectory $name)) { throw "Uninstall left $name installed." }
    }
    @{ applicationMissing = $true; sidecarMissing = $true } | ConvertTo-Json | Set-Content (Join-Path $fixtureRoot 'uninstalled-files.json')
}

try {
    $env:DEZIRCLIP_DB_PATH = Join-Path $profileDirectory 'clipboard.db'
    Initialize-SharedProfile
    $initial = if ($Scenario -eq 'OwnedUpgrade') { $PreviousCandidate } elseif ($owned) { $NewInstaller } else { $OldInstaller }
    if ([string]::IsNullOrWhiteSpace($initial) -or -not (Test-Path -LiteralPath $initial -PathType Leaf)) { throw 'Initial installer fixture is missing.' }
    $initialDigest = if ($previous) { $previous.installerSha256 } elseif ($owned) { $candidate.installerSha256 } else { 'D20033124C74BEA451301716AFB46623607602BAE37647748AE7D25AB7DD4B8E' }
    Run-Installer -Path $initial -Arguments @('/S', "/D=$installDirectory") -ExpectedSha256 $initialDigest
    foreach ($name in @('dezirclip.exe', 'dzc.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDirectory $name) -PathType Leaf)) { throw 'Initial installed GUI or sidecar is missing.' }
    }
    & (Join-Path $installDirectory 'dzc.exe') add $marker | Set-Content (Join-Path $fixtureRoot 'old-add.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Initial CLI fixture initialization failed.' }
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
    if ($owned) {
        $initialManifest = if ($previous) { $previous } else { $candidate }
        Invoke-ActualGui $initialManifest $true ($Scenario -eq 'OwnedExternalEdit') 'initial'
        $acquired = @(Snapshot-Settings)
        if ($acquired[0].Value -ne 0 -or $acquired[1].Value -ne 0 -or $acquired[2].Value -ne 'XV') { throw 'Application did not persist expected owned values.' }
        if ((Get-ReceiptCount) -ne 3) { throw 'Application did not persist all three ownership receipts.' }
        if ($Scenario -eq 'OwnedUpgrade') {
            Run-Installer -Path $NewInstaller -Arguments @('/S', '/UPDATE', "/D=$installDirectory") -ExpectedSha256 $candidate.installerSha256
            if (($acquired | ConvertTo-Json -Depth 6) -ne (@(Snapshot-Settings) | ConvertTo-Json -Depth 6)) { throw 'Upgrade changed owned values.' }
            if ((Get-ReceiptCount) -ne 3) { throw 'Upgrade removed ownership receipts.' }
            Invoke-ActualGui $candidate $false $true 'upgraded'
        } else {
            $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Microsoft\Clipboard')
            try { $key.SetValue('EnableClipboardHistory', 9, [Microsoft.Win32.RegistryValueKind]::DWord) } finally { $key.Dispose() }
            $before[0].Value = 9
            $beforeJson = $before | ConvertTo-Json -Depth 6
        }
        Run-Installer (Join-Path $installDirectory 'uninstall.exe') @('/S', "_?=$installDirectory")
        Assert-Uninstalled
        $restored = @(Snapshot-Settings)
        if ($beforeJson -ne ($restored | ConvertTo-Json -Depth 6)) { throw 'Owned uninstall did not restore typed originals or preserve the newer external edit.' }
        if ((Get-ReceiptCount) -ne 0) { throw 'Uninstall left ownership receipts.' }
        $restored | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'settings-restored.json')
        Assert-Explorer $true
    } elseif ($Scenario -eq 'LegacyUninstall') {
        Run-Installer (Join-Path $installDirectory 'uninstall.exe') @('/S', "_?=$installDirectory")
        Assert-Uninstalled
        $after = @(Snapshot-Settings)
        $after | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'settings-after.json')
        if ($beforeJson -eq ($after | ConvertTo-Json -Depth 6)) { throw 'Expected the published old hook to demonstrate its destructive reset behavior.' }
        if ($after[0].Value -ne 1 -or $after[1].Value -ne 1 -or $after[2].Value -ne 'X' -or $after[3].Exists -or $after[4].Exists -or $after[5].Exists) { throw 'Published old hook behavior differed; retain evidence and investigate.' }
        Assert-Explorer $false
    } else {
        Run-Installer -Path $NewInstaller -Arguments @('/S', '/UPDATE', "/D=$installDirectory") -ExpectedSha256 $candidate.installerSha256
        if ($beforeJson -ne (@(Snapshot-Settings) | ConvertTo-Json -Depth 6)) { throw 'Upgrade changed pre-existing Windows settings.' }
        Invoke-ActualGui $candidate $false $true 'upgraded'
        Run-Installer (Join-Path $installDirectory 'uninstall.exe') @('/S', "_?=$installDirectory")
        Assert-Uninstalled
        if ($beforeJson -ne (@(Snapshot-Settings) | ConvertTo-Json -Depth 6)) { throw 'Unowned uninstall changed pre-existing Windows settings.' }
        if ((Get-ReceiptCount) -ne 0) { throw 'Unowned uninstall left ownership receipts.' }
        Assert-Explorer $true
    }
    $runEvidence.status = 'passed'
} catch {
    $runEvidence.status = 'failed'
    $runEvidence.error = $_.Exception.ToString()
    throw
} finally {
    if (-not (Test-Path -LiteralPath (Join-Path $fixtureRoot 'process-evidence.json'))) {
        try { Assert-Explorer $false } catch { $runEvidence.processSnapshotError = $_.Exception.Message }
    }
    foreach ($created in $redirects) {
        $cleanup = [ordered]@{ redirect = $created.redirect; result = 'already absent' }
        try {
            $directoryIsReparsePoint = (Test-Path -LiteralPath $created.directory) -and ((Get-Item -LiteralPath $created.directory -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)
            if ($directoryIsReparsePoint) {
                $cleanup.result = 'preserved changed directory'
            } elseif (Test-Path -LiteralPath $created.redirect) {
                $file = Get-Item -LiteralPath $created.redirect -Force
                if (($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -or [System.IO.File]::ReadAllText($created.redirect) -ne $profileDirectory) {
                    $cleanup.result = 'preserved changed redirect'
                } else {
                    [System.IO.File]::Delete($created.redirect)
                    $cleanup.result = 'removed owned redirect'
                }
            }
            if (-not $directoryIsReparsePoint -and (Test-Path -LiteralPath $created.directory -PathType Container) -and @(Get-ChildItem -LiteralPath $created.directory -Force).Count -eq 0) {
                [System.IO.Directory]::Delete($created.directory, $false)
            }
        } catch { $cleanup.result = 'cleanup error; preserved for runner disposal'; $cleanup.error = $_.Exception.Message }
        $runEvidence.cleanup += $cleanup
    }
    $env:DEZIRCLIP_DB_PATH = $previousDbOverride
    $runEvidence.finishedAt = [DateTime]::UtcNow.ToString('o')
    Save-RunEvidence
}
Write-Output 'Unpublished debug package acceptance does not establish production updater signing or download acceptance.'
