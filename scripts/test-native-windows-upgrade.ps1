param(
    [ValidateSet('OldToNew', 'PublishedAlphaToAlpha', 'LegacyUninstall', 'OwnedUpgrade', 'OwnedExternalEdit')][string]$Scenario = 'OldToNew',
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
$publishedAlpha = $Scenario -eq 'PublishedAlphaToAlpha'
$oldDigest = if ($publishedAlpha) { '31D8B88064E1185C9F74B5FDD327902B9BC860E523B9A5AC0D71620EC7ADBB87' } else { 'D20033124C74BEA451301716AFB46623607602BAE37647748AE7D25AB7DD4B8E' }
if (-not $owned) { $OldInstaller = Resolve-RunnerFixturePath $OldInstaller -RequireFile }
if (-not $owned -and (Get-FileHash -LiteralPath $OldInstaller -Algorithm SHA256).Hash -ne $oldDigest) {
    throw 'The old installer differs from the explicitly selected pinned published x64 artifact.'
}
function Assert-CurrentBuildInfo($Info, [string]$Role, [string]$Version, [string]$Revision) {
    if ($Role -notin @('desktop', 'cli', 'service') -or $Version -notin @('0.2.0-alpha.1', '0.2.0-alpha.2') -or $Revision -cnotmatch '^[a-f0-9]{40}$') { throw 'Invalid independent build policy inputs.' }
    if ($null -eq $Info -or $Info.component -cne $Role -or $Info.component_version -cne $Version -or $Info.source_revision -cne $Revision -or $Info.build_id -cne ($Version + '@' + $Revision)) { throw 'Component build identity differs from the independent candidate policy.' }
    if ($Info.source_state -cnotin @('verified_checkout', 'modified_checkout') -or $Info.target -cne 'x86_64-pc-windows-msvc') { throw 'Component source provenance or target is unavailable.' }
    foreach ($field in @('metadata_version', 'database_schema_version')) {
        if ($Info.$field -isnot [int] -and $Info.$field -isnot [long]) { throw 'Build metadata fields must be JSON integers.' }
    }
    if ($Info.metadata_version -ne 1 -or $Info.database_schema_version -ne 24) { throw 'Component metadata or database schema differs from alpha.2 policy.' }
    foreach ($field in @('major', 'minor')) {
        if ($Info.protocol_version.$field -isnot [int] -and $Info.protocol_version.$field -isnot [long]) { throw 'Protocol fields must be JSON integers.' }
    }
    if ($Info.protocol_version.major -ne 1 -or $Info.protocol_version.minor -ne 0) { throw 'Component wire protocol differs from alpha.2 policy.' }
    $names = @('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')
    if ($Role -eq 'service') { $names += 'dzc-service' }
    $linked = @($Info.linked_components)
    if ($linked.Count -ne $names.Count) { throw 'Actual linked inventory differs from alpha.2 policy.' }
    for ($index = 0; $index -lt $names.Count; $index++) {
        if ($linked[$index].component -cne $names[$index] -or $linked[$index].version -cne $Version) { throw 'Actual linked inventory differs from alpha.2 policy.' }
    }
}
function Assert-NsisCompilerInputManifest($Manifest) {
    if ($Manifest.schemaVersion -ne 4 -or $Manifest.nsisInputLayout -ne 'cargo-cli-and-service-v1') { throw 'Candidate NSIS input layout is invalid.' }
    $inputs = @($Manifest.nsisInputs)
    if ($inputs.Count -ne 1) { throw 'Candidate must retain exactly one Cargo CLI compiler input.' }
    $cargo = @($Manifest.sidecarPreparation.binaryPaths | Where-Object { $_.role -eq 'cargo-host-default' })
    if ($cargo.Count -ne 1 -or $cargo[0].sha256 -ne $Manifest.sidecarSha256 -or $Manifest.sidecarSha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Candidate lacks a unique proven Cargo host CLI.' }
    $input = $inputs[0]
    if ($input.role -ne 'cargo-binary' -or $input.destination -ne 'dzc.exe' -or $input.sha256 -ne $Manifest.sidecarSha256 -or [System.IO.Path]::GetFullPath($input.source) -ine [System.IO.Path]::GetFullPath($cargo[0].path)) { throw 'Candidate compiler input differs from the exact proven Cargo CLI.' }
    $service = @($Manifest.sidecarPreparation.serviceHelper.binaryPaths | Where-Object { $_.role -eq 'cargo-host-default' })
    $serviceInputs = @($Manifest.serviceInputs)
    if ($service.Count -ne 1 -or $serviceInputs.Count -ne 1 -or $Manifest.serviceSha256 -notmatch '^[a-fA-F0-9]{64}$' -or $service[0].sha256 -ne $Manifest.serviceSha256) { throw 'Candidate lacks a unique proven Cargo service.' }
    $input = $serviceInputs[0]
    if ($input.role -ne 'cargo-binary' -or $input.destination -ne 'dzc-service.exe' -or $input.sha256 -ne $Manifest.serviceSha256 -or [System.IO.Path]::GetFullPath($input.source) -ine [System.IO.Path]::GetFullPath($service[0].path)) { throw 'Service compiler input differs from its standalone helper.' }
}
function Read-CandidateManifest([string]$Installer) {
    $Installer = Resolve-RunnerFixturePath $Installer -RequireFile
    $manifestPath = Resolve-RunnerFixturePath ($Installer + '.manifest.json') -RequireFile
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schemaVersion -ne 4 -or $manifest.kind -ne 'dezirclip-native-nsis-fixture' -or $manifest.version -notin @('0.2.0-alpha.1', '0.2.0-alpha.2')) { throw 'Candidate manifest is invalid.' }
    if ($manifest.target -ne 'x86_64-pc-windows-msvc' -or $manifest.profile -ne 'debug' -or $manifest.updaterArtifacts -ne $false -or $manifest.sourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Candidate build provenance is invalid.' }
    if ($manifest.sidecarVersion -notmatch ('(?:^|\s)' + [regex]::Escape($manifest.version) + '(?:\s|$)')) { throw 'Candidate manifest CLI version is invalid.' }
    if ($manifest.buildInfo.component_version -ne $manifest.version -or $manifest.buildInfo.source_revision -ne $manifest.sourceCommit -or $manifest.buildInfo.build_id -ne ($manifest.version + '@' + $manifest.sourceCommit)) { throw 'Candidate build identity differs from its source/version set.' }
    if ($manifest.buildInfo.metadata_version -ne 1 -or $manifest.buildInfo.component -ne 'cli' -or $manifest.buildInfo.target -ne $manifest.target -or @($manifest.buildInfo.linked_components).Count -ne 4) { throw 'Candidate component metadata is incomplete.' }
    if ($manifest.serviceBuildInfo.component -ne 'service' -or $manifest.serviceBuildInfo.build_id -ne $manifest.buildInfo.build_id -or @($manifest.serviceBuildInfo.linked_components).Count -ne 5) { throw 'Candidate service component metadata is incomplete.' }
    Assert-CurrentBuildInfo $manifest.buildInfo 'cli' $manifest.version $manifest.sourceCommit
    Assert-CurrentBuildInfo $manifest.serviceBuildInfo 'service' $manifest.version $manifest.sourceCommit
    if ($manifest.applicationPatch -ne 'tauri-nsis-bundle-token-v1' -or $manifest.applicationPatchOffset -lt 64 -or $manifest.sidecarSource -ne 'standalone-no-default-features' -or $manifest.packaging -ne 'shared-before-build-and-before-bundle') { throw 'Candidate packaging provenance is invalid.' }
    if ($manifest.sidecarPreparation.phase -ne 'bundle' -or $manifest.sidecarPreparation.sha256 -ne $manifest.sidecarSha256 -or $manifest.sidecarPreparation.portable -ne $false -or $manifest.sidecarPreparation.profile -ne 'debug') { throw 'Candidate lacks shared beforeBundle sidecar evidence.' }
    if ($manifest.applicationPatchOffset -isnot [int] -and $manifest.applicationPatchOffset -isnot [long]) { throw 'Candidate patch offset must be an integer.' }
    foreach ($digest in @($manifest.installerSha256, $manifest.applicationSha256, $manifest.applicationRawSha256, $manifest.sidecarSha256, $manifest.cargoLockSha256, $manifest.configurationSha256, $manifest.installerHookSha256, $manifest.nsisScriptSha256, $manifest.overrideConfigurationSha256)) {
        if ($digest -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Candidate manifest digest is invalid.' }
    }
    if ((Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash -ne $manifest.installerSha256) { throw 'Candidate installer differs from its build manifest.' }
    $nsisScript = Resolve-RunnerFixturePath ($Installer + '.nsi') -RequireFile
    if ((Get-FileHash -LiteralPath $nsisScript -Algorithm SHA256).Hash -ne $manifest.nsisScriptSha256) { throw 'Candidate NSIS compiler script differs from its build manifest.' }
    Assert-NsisCompilerInputManifest $manifest
    return $manifest
}
$candidate = if ($Scenario -ne 'LegacyUninstall') { Read-CandidateManifest $NewInstaller } else { $null }
if ($candidate -and $candidate.version -ne '0.2.0-alpha.2') { throw 'Current candidate must be the alpha.2 fixture.' }
$previous = if ($Scenario -eq 'OwnedUpgrade') { Read-CandidateManifest $PreviousCandidate } else { $null }
if ($previous -and $previous.version -ne '0.2.0-alpha.1') { throw 'Previous same-source ownership fixture must be labelled alpha.1; it is not a historical package migration.' }
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
if ($candidate) { Copy-Item -LiteralPath ($NewInstaller + '.nsi') -Destination (Join-Path $fixtureRoot 'candidate-installer.nsi') }
if ($previous) { Copy-Item -LiteralPath ($PreviousCandidate + '.nsi') -Destination (Join-Path $fixtureRoot 'previous-installer.nsi') }
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
function Get-InstalledApplicationProof([string]$Path, $Manifest) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ([Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes)) -ne $Manifest.applicationSha256) { throw 'Installed GUI differs from the exact expected packaged bytes.' }
    $offset = [long]$Manifest.applicationPatchOffset
    $nsis = '__TAURI_BUNDLE_TYPE_VAR_NSS'
    if ($offset -lt 64 -or $offset -gt $bytes.Length - $nsis.Length -or [System.Text.Encoding]::ASCII.GetString($bytes, [int]$offset, $nsis.Length) -ne $nsis) { throw 'Installed GUI lacks the exact expected NSIS package marker.' }
    [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK'), 0, $bytes, $offset, $nsis.Length)
    $rawSha256 = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes))
    if ($rawSha256 -ne $Manifest.applicationRawSha256) { throw 'Installed GUI does not match its original compiled build after the single documented NSIS token patch.' }
    return [pscustomobject]@{ rawSha256 = $rawSha256; patchOffset = $offset }
}
function Assert-InstalledCandidate($Manifest, [string]$Label) {
    $application = Join-Path $installDirectory 'dezirclip.exe'
    $sidecar = Join-Path $installDirectory 'dzc.exe'
    $service = Join-Path $installDirectory 'dzc-service.exe'
    $actual = @{
        version = $Manifest.version
        applicationSha256 = (Get-FileHash -LiteralPath $application -Algorithm SHA256).Hash
        sidecarSha256 = (Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash
        serviceSha256 = (Get-FileHash -LiteralPath $service -Algorithm SHA256).Hash
    }
    $actual | ConvertTo-Json | Set-Content (Join-Path $fixtureRoot ($Label + '-installed.json'))
    if ($actual.applicationSha256 -ne $Manifest.applicationSha256 -or $actual.sidecarSha256 -ne $Manifest.sidecarSha256 -or $actual.serviceSha256 -ne $Manifest.serviceSha256) { throw 'Installed GUI, CLI or service differs from the candidate build.' }
    $proof = Get-InstalledApplicationProof $application $Manifest
    $actual.applicationRawSha256 = $proof.rawSha256
    $actual.applicationPatchOffset = $proof.patchOffset
    $actual | ConvertTo-Json | Set-Content (Join-Path $fixtureRoot ($Label + '-installed.json'))
    $version = & $sidecar --version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch ('(?:^|\s)' + [regex]::Escape($Manifest.version) + '(?:\s|$)')) { throw 'Installed CLI has the wrong built-in version.' }
    $version | Set-Content (Join-Path $fixtureRoot ($Label + '-cli-version.txt'))
    $buildInfo = & $sidecar build-info --json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $buildInfo.component -ne 'cli' -or $buildInfo.build_id -ne $Manifest.buildInfo.build_id -or $buildInfo.component_version -ne $Manifest.version -or $buildInfo.source_revision -ne $Manifest.buildInfo.source_revision -or $buildInfo.source_state -ne $Manifest.buildInfo.source_state -or $buildInfo.metadata_version -ne $Manifest.buildInfo.metadata_version -or $buildInfo.database_schema_version -ne $Manifest.buildInfo.database_schema_version -or $buildInfo.protocol_version.major -ne $Manifest.buildInfo.protocol_version.major -or $buildInfo.protocol_version.minor -ne $Manifest.buildInfo.protocol_version.minor -or $buildInfo.target -ne $Manifest.buildInfo.target -or (ConvertTo-Json -InputObject @($buildInfo.linked_components) -Depth 5 -Compress) -ne (ConvertTo-Json -InputObject @($Manifest.buildInfo.linked_components) -Depth 5 -Compress)) { throw 'Installed CLI has a different component build identity.' }
    $buildInfo | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot ($Label + '-cli-build-info.json'))
    $serviceInfo = & $service --build-info | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or (ConvertTo-Json $serviceInfo -Depth 6 -Compress) -ne (ConvertTo-Json $Manifest.serviceBuildInfo -Depth 6 -Compress)) { throw 'Installed service identity differs from its build receipt.' }
    $serviceInfo | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot ($Label + '-service-build-info.json'))
}
function Assert-CliHistory([string]$Text, [Nullable[long]]$ExpectedId, [string]$Label) {
    $output = & (Join-Path $installDirectory 'dzc.exe') search $Text --json
    if ($LASTEXITCODE -ne 0) { throw 'Installed CLI history query failed.' }
    $output | Set-Content (Join-Path $fixtureRoot ($Label + '-cli-history.json'))
    $entries = @(($output | Out-String | ConvertFrom-Json) | Where-Object { $_.content -eq $Text })
    if ($entries.Count -ne 1 -or ($null -ne $ExpectedId -and $entries[0].id -ne $ExpectedId)) { throw 'CLI and GUI did not observe the same single stored entry.' }
}
function Get-ActualProcessImageSnapshot([System.Diagnostics.Process]$Application) {
    $observedPath = $null
    $hasExited = $null
    $errors = @()
    $fresh = $null
    try {
        $Application.Refresh()
        $hasExited = $Application.HasExited
        if (-not $hasExited) {
            $fresh = Get-Process -Id $Application.Id -ErrorAction Stop
            $fresh.Refresh()
            $observedPath = $fresh.Path
            # Recheck the original process handle so a PID reused after exit
            # cannot establish the identity of the process we launched.
            $hasExited = $null
            $Application.Refresh()
            $hasExited = $Application.HasExited
        }
    } catch {
        $errors += $_.Exception.GetType().FullName + ': ' + $_.Exception.Message
        $hasExited = $null
        try { $Application.Refresh(); $hasExited = $Application.HasExited }
        catch { $errors += $_.Exception.GetType().FullName + ': ' + $_.Exception.Message }
    } finally { if ($fresh) { $fresh.Dispose() } }
    return [pscustomobject]@{ processId = $Application.Id; observedPath = $observedPath; hasExited = $hasExited; error = ($errors -join '; '); pathSource = 'fresh Get-Process.Path' }
}
function Wait-ActualProcessImage {
    param(
        [Parameter(Mandatory)][string]$ExpectedPath,
        [Parameter(Mandatory)][scriptblock]$ReadProcess,
        [scriptblock]$Delay = { param($Milliseconds) Start-Sleep -Milliseconds $Milliseconds },
        [ValidateRange(1, 10000)][int]$TimeoutMilliseconds = 2000,
        [ValidateRange(1, 1000)][int]$PollMilliseconds = 50,
        [ValidateRange(1, 201)][int]$MaximumAttempts = 41
    )
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $result = [ordered]@{ expectedPath = $ExpectedPath; verified = $false; reason = 'image_path_unavailable'; timeoutMilliseconds = $TimeoutMilliseconds; maximumAttempts = $MaximumAttempts; attempts = @() }
    for ($attempt = 1; $attempt -le $MaximumAttempts; $attempt++) {
        try { $snapshot = & $ReadProcess }
        catch { $snapshot = [pscustomobject]@{ observedPath = $null; hasExited = $null; error = $_.Exception.GetType().FullName + ': ' + $_.Exception.Message } }
        $observed = $snapshot.observedPath
        $result.attempts += [pscustomobject]@{ attempt = $attempt; elapsedMilliseconds = $timer.ElapsedMilliseconds; processId = $snapshot.processId; observedPath = $observed; hasExited = $snapshot.hasExited; error = $snapshot.error; pathSource = $snapshot.pathSource }
        if ($snapshot.hasExited -eq $true) { $result.reason = 'process_exited'; break }
        if ($null -ne $observed -and $observed -cne '') {
            if ($observed -isnot [string] -or -not [string]::Equals($observed, $ExpectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
                $result.reason = 'unexpected_image_path'
                break
            }
            if ($snapshot.hasExited -is [bool] -and -not $snapshot.hasExited) {
                $result.verified = $true
                $result.reason = 'identity_verified'
                break
            }
            $result.reason = 'process_state_unavailable'
        }
        $remaining = $TimeoutMilliseconds - $timer.ElapsedMilliseconds
        if ($attempt -eq $MaximumAttempts -or $remaining -le 0) { break }
        & $Delay ([int][Math]::Min($PollMilliseconds, $remaining))
    }
    $result.elapsedMilliseconds = $timer.ElapsedMilliseconds
    return [pscustomobject]$result
}
function Convert-GuiConfigurationJson($Manifest, [string]$ProfileDirectory, [string]$Marker, [bool]$AcquireOwnership, [bool]$VerifyCapture) {
    return (@{ expectedVersion = $Manifest.version; expectedBuildInfo = $Manifest.buildInfo; profileDirectory = $ProfileDirectory; marker = $Marker; acquireOwnership = $AcquireOwnership; verifyCapture = $VerifyCapture } | ConvertTo-Json -Depth 10)
}
function Invoke-ActualGui($Manifest, [bool]$AcquireOwnership, [bool]$VerifyCapture, [string]$Label) {
    Assert-InstalledCandidate $Manifest $Label
    $port = 9222
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
    $listener.Start(); $listener.Stop()
    $configuration = Join-Path $fixtureRoot ($Label + '-gui-input.json')
    Convert-GuiConfigurationJson $Manifest $profileDirectory $marker $AcquireOwnership $VerifyCapture |
        Set-Content -LiteralPath $configuration -Encoding utf8
    $evidence = Join-Path $fixtureRoot ($Label + '-gui-evidence.json')
    $stdout = Join-Path $fixtureRoot ($Label + '-stdout.txt')
    $stderr = Join-Path $fixtureRoot ($Label + '-stderr.txt')
    $expectedApplication = Join-Path $installDirectory 'dezirclip.exe'
    $identity = $null
    $application = Start-Process -FilePath $expectedApplication -WorkingDirectory $installDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        $identity = Wait-ActualProcessImage -ExpectedPath $expectedApplication -ReadProcess { Get-ActualProcessImageSnapshot $application }
        $identity | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $fixtureRoot ($Label + '-process-identity.json'))
        if (-not $identity.verified) { throw "Launched GUI process identity was not verified: $($identity.reason)." }
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
        @{ Id = $application.Id; HasExited = $application.HasExited; ExitCode = $(if ($application.HasExited) { $application.ExitCode } else { $null }); Port = $port; ExpectedPath = $expectedApplication; Identity = $identity } |
            ConvertTo-Json -Depth 7 | Set-Content (Join-Path $fixtureRoot ($Label + '-process.json'))
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
    foreach ($name in @('dezirclip.exe', 'dzc.exe', 'dzc-service.exe')) {
        if (Test-Path -LiteralPath (Join-Path $installDirectory $name)) { throw "Uninstall left $name installed." }
    }
    @{ applicationMissing = $true; sidecarMissing = $true } | ConvertTo-Json | Set-Content (Join-Path $fixtureRoot 'uninstalled-files.json')
}

try {
    $env:DEZIRCLIP_DB_PATH = Join-Path $profileDirectory 'clipboard.db'
    Initialize-SharedProfile
    $initial = if ($Scenario -eq 'OwnedUpgrade') { $PreviousCandidate } elseif ($owned) { $NewInstaller } else { $OldInstaller }
    if ([string]::IsNullOrWhiteSpace($initial) -or -not (Test-Path -LiteralPath $initial -PathType Leaf)) { throw 'Initial installer fixture is missing.' }
    $initialDigest = if ($previous) { $previous.installerSha256 } elseif ($owned) { $candidate.installerSha256 } else { $oldDigest }
    Run-Installer -Path $initial -Arguments @('/S', "/D=$installDirectory") -ExpectedSha256 $initialDigest
    foreach ($name in @('dezirclip.exe', 'dzc.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDirectory $name) -PathType Leaf)) { throw 'Initial installed GUI or sidecar is missing.' }
    }
    & (Join-Path $installDirectory 'dzc.exe') add $marker | Set-Content (Join-Path $fixtureRoot 'old-add.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Initial CLI fixture initialization failed.' }
    if ($publishedAlpha) {
        $publishedInfo = & (Join-Path $installDirectory 'dzc.exe') build-info --json | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $publishedInfo.component -cne 'cli' -or $publishedInfo.component_version -cne '0.2.0-alpha.1' -or $publishedInfo.source_revision -cne '90ba9960744b6cd57caaaf597f68533a7e41be45' -or $publishedInfo.source_state -cne 'verified_checkout' -or $publishedInfo.database_schema_version -ne 21 -or @($publishedInfo.linked_components).Count -ne 3) { throw 'Published alpha.1 runtime identity or original schema differs from the pinned release.' }
        $publishedInfo | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $fixtureRoot 'published-alpha1-cli-build-info.json')
    }
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
