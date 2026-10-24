param(
    [Parameter(Mandatory)][string]$FixtureDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedSource,
    [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]*$')][string]$ExpectedReleaseId,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedPolicySha256
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$version = '0.2.0-alpha.2'
$repository = Split-Path -Parent $PSScriptRoot
function Resolve-ApplicationPath([Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Name) {
    $application = Get-Command -Name $Name -CommandType Application -ErrorAction Stop | Select-Object -First 1
    if ($application -isnot [System.Management.Automation.ApplicationInfo]) { throw 'A single application command is required.' }
    $path = $application.Path
    if ([string]::IsNullOrWhiteSpace($path) -or -not [System.IO.Path]::IsPathFullyQualified($path) -or
        -not [System.IO.File]::Exists($path)) { throw 'The resolved application must be one existing absolute file.' }
    return $path
}
if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or -not $env:RUNNER_TEMP -or
    $env:GITHUB_REPOSITORY -cne 'qiyuanhuakai/dezirclip') {
    throw 'Final release upgrade execution requires the exact tag on a disposable GitHub-hosted runner.'
}
if ($env:OS -cne 'Windows_NT' -or [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'X64' -or
    [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64' -or $env:PROCESSOR_ARCHITEW6432 -or
    $env:PROCESSOR_ARCHITECTURE -ine 'AMD64') { throw 'Actual native x64 Windows is required.' }
$workflowSource = $null
if ($env:DZC_RELEASE_RECOVERY -and $env:DZC_RELEASE_RECOVERY -cnotin @('0', '1')) { throw 'Invalid release recovery mode.' }
if ($env:DZC_RELEASE_RECOVERY -ceq '1') {
    $authorityOutput = & (Resolve-ApplicationPath 'python') (Join-Path $PSScriptRoot 'release_recovery.py') authority $ExpectedSource
    if ($LASTEXITCODE -ne 0) { throw 'The independent release recovery authority refused this execution.' }
    $authority = ($authorityOutput -join "`n") | ConvertFrom-Json -Depth 20
    if ($authority.schema_version -ne 1 -or $authority.mode -cne 'recovery' -or $authority.source_revision -cne $ExpectedSource -or
        $authority.workflow_source_revision -cnotmatch '^[a-f0-9]{40}$' -or $authority.workflow_source_revision -cne $env:DZC_RELEASE_TOOLING_SOURCE -or
        $authority.workflow_source_revision -cne $env:GITHUB_SHA -or $authority.tag -cne ('v' + $version) -or
        $authority.release_id.ToString() -cne $ExpectedReleaseId -or $authority.run_id -cne $env:GITHUB_RUN_ID -or
        $authority.run_attempt -cne $env:GITHUB_RUN_ATTEMPT) { throw 'The recovery source/release/run receipt differs.' }
    $workflowSource = $authority.workflow_source_revision
} else {
    if ($env:GITHUB_REF -cne ('refs/tags/v' + $version) -or $env:GITHUB_SHA -cne $ExpectedSource) {
        throw 'Final release upgrade execution requires the exact tag on a disposable GitHub-hosted runner.'
    }
    $head = & git -C $repository rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $head -cne $ExpectedSource) { throw 'The checkout is not the final expected source.' }
    $changes = & git -C $repository status --porcelain=v1 --untracked-files=no
    if ($LASTEXITCODE -ne 0 -or $changes) { throw 'The final source checkout must be clean.' }
}

function Resolve-OwnedPath([string]$Path, [switch]$File) {
    $boundary = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/')
    $full = [System.IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($boundary + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture path escapes RUNNER_TEMP.' }
    for ($current = $full; ; $current = Split-Path -Parent $current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { throw 'Fixture paths cannot cross reparse points.' }
        if ($current -ieq $boundary) { break }
    }
    if ($File -and -not (Test-Path -LiteralPath $full -PathType Leaf)) { throw 'Required fixture file is missing.' }
    return $full
}
function Read-OwnedJson([string]$Path, [long]$Limit = 1048576) {
    $full = Resolve-OwnedPath $Path -File
    if ((Get-Item -LiteralPath $full).Length -gt $Limit) { throw 'Fixture JSON exceeds its bound.' }
    return [System.IO.File]::ReadAllText($full) | ConvertFrom-Json -Depth 40
}
function Get-Digest([string]$Path) { return (Get-FileHash -LiteralPath (Resolve-OwnedPath $Path -File) -Algorithm SHA256).Hash.ToLowerInvariant() }
function Save-Json([string]$Path, $Value) {
    [System.IO.File]::WriteAllText((Resolve-OwnedPath $Path), (ConvertTo-Json -InputObject $Value -Depth 40) + "`n", [System.Text.UTF8Encoding]::new($false))
}
$fixture = Resolve-OwnedPath $FixtureDirectory
if ((Split-Path -Parent $fixture) -ine [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/') -or
    (Split-Path -Leaf $fixture) -cnotmatch '^dezirclip-upgrade-[a-f0-9]{32}$' -or -not (Test-Path -LiteralPath $fixture -PathType Container)) { throw 'A new directly owned upgrade fixture is required.' }
foreach ($name in @('raw-native.json', 'report.json', 'installed', 'profile', 'extracted-roles.json')) {
    if (Test-Path -LiteralPath (Join-Path $fixture $name)) { throw 'Native acceptance output already exists; do not overwrite an earlier attempt.' }
}
$inputPath = Join-Path $fixture 'inputs.json'
$inputs = Read-OwnedJson $inputPath
$expectedOld = @{
    LegacyToAlpha2 = @{ version = '0.1.20'; sha256 = 'd20033124c74bea451301716afb46623607602bae37647748ae7d25ab7dd4b8e' }
    PublishedAlphaToAlpha2 = @{ version = '0.2.0-alpha.1'; sha256 = '31d8b88064e1185c9f74b5fdd327902b9bc860e523b9a5ac0d71620ec7adbb87'; source_revision = '90ba9960744b6cd57caaaf597f68533a7e41be45' }
}
if ($inputs.schema_version -ne 1 -or $inputs.kind -cne 'release_windows_upgrade_inputs' -or $inputs.version -cne $version -or
    $inputs.source_revision -cne $ExpectedSource -or $inputs.release_id.ToString() -cne $ExpectedReleaseId -or
    $inputs.policy_sha256 -cne $ExpectedPolicySha256 -or -not $expectedOld.ContainsKey($inputs.scenario) -or
    $inputs.marker -cnotmatch '^nativeupgradefixturemarker[a-f0-9]{32}$' -or @($inputs.signed_assets).Count -ne 8) { throw 'Staged inputs do not match the independent final release policy.' }
$inputWorkflow = $inputs.PSObject.Properties['workflow_source_revision']
if ($workflowSource) {
    if (-not $inputWorkflow -or $inputWorkflow.Value -cne $workflowSource) { throw 'Staged inputs belong to another recovery workflow source.' }
} elseif ($inputWorkflow) { throw 'A normal tag run cannot consume recovery workflow inputs.' }
$old = $expectedOld[$inputs.scenario]
if ($inputs.old.version -cne $old.version -or $inputs.old.sha256 -cne $old.sha256) { throw 'The historical package identity is not the pinned published installer.' }
if ($inputs.scenario -ceq 'PublishedAlphaToAlpha2' -and $inputs.old.source_revision -cne $old.source_revision) { throw 'The historical alpha.1 source differs.' }
$installDirectory = Join-Path $fixture 'installed'
$profileDirectory = Join-Path $fixture 'profile'
$database = Join-Path $profileDirectory 'clipboard.db'
$marker = $inputs.marker
$controller = Join-Path $PSScriptRoot 'release-windows-upgrade.mjs'
$webviewPolicy = Join-Path $fixture 'final-webview-policy.json'
$webviewPolicyRestored = $false
$checkNames = @('original_installer_hash', 'candidate_signature_and_hash', 'actual_old_install', 'actual_old_profile_created',
    'actual_upgrade_same_directory', 'installed_three_role_bytes', 'actual_three_role_build_info', 'history_and_schema24',
    'frozen_preferences_preserved', 'windows_settings_preserved', 'explorer_not_restarted', 'controlled_processes_exited',
    'actual_release_uninstall', 'uninstall_preserves_profile_and_settings')
$raw = [ordered]@{ schema_version = 1; kind = 'release_windows_upgrade_native'; status = 'failed'; version = $version;
    source_revision = $ExpectedSource; release_id = [long]$ExpectedReleaseId; policy_sha256 = $ExpectedPolicySha256;
    scenario = $inputs.scenario; old = $inputs.old; signed_assets = @($inputs.signed_assets); roles = @{}; installed = @(); packaged = @();
    checks = @($checkNames | ForEach-Object { [ordered]@{ name = $_; status = 'not_run' } }); started_at = [DateTime]::UtcNow.ToString('o');
    processes = @(); redirects = @(); cleanup = @(); failure = 'Native acceptance is incomplete.' }
if ($workflowSource) { $raw['workflow_source_revision'] = $workflowSource }
$controlled = [System.Collections.Generic.List[object]]::new()
$redirects = [System.Collections.Generic.List[object]]::new()
function Save-Raw { Save-Json (Join-Path $fixture 'raw-native.json') $raw }
function Pass-Check([string]$Name, $Evidence) {
    $found = @($raw.checks | Where-Object { $_.name -ceq $Name })
    if ($found.Count -ne 1 -or $found[0].status -cne 'not_run') { throw 'Invalid or repeated native check.' }
    $found[0].status = 'passed'; $found[0].evidence = $Evidence; Save-Raw
}

# Children receive the disposable OS/profile environment, never Actions or publishing credentials.
function New-ChildEnvironment {
    $allowed = @('SYSTEMROOT', 'WINDIR', 'COMSPEC', 'OS', 'PATH', 'PATHEXT', 'PROGRAMFILES', 'PROGRAMFILES(X86)', 'PROGRAMW6432',
        'USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP',
        'PROCESSOR_ARCHITECTURE', 'NUMBER_OF_PROCESSORS', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_TEMP',
        'GITHUB_REPOSITORY', 'GITHUB_REF', 'GITHUB_REF_NAME', 'GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_EVENT_NAME',
        'GITHUB_WORKFLOW_REF', 'DZC_RELEASE_RECOVERY', 'DZC_RELEASE_TOOLING_SOURCE', 'RUNNER_NAME', 'RUNNER_OS', 'RUNNER_ARCH')
    $environment = @{}
    foreach ($pair in [Environment]::GetEnvironmentVariables().GetEnumerator()) { if ($pair.Key.ToString().ToUpperInvariant() -in $allowed) { $environment[$pair.Key] = $pair.Value.ToString() } }
    $environment['DEZIRCLIP_DB_PATH'] = $database
    $environment['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $fixture 'webview-profile'
    return $environment
}
function Start-OwnedProcess([string]$Executable, [string[]]$Arguments, [string]$Label, [string]$NsisLastArgument = '', [hashtable]$ExtraEnvironment = @{}, [bool]$RecordObservation = $true) {
    if ($Label -cnotmatch '^[a-z0-9-]+$') { throw 'Invalid process evidence label.' }
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable; $start.WorkingDirectory = $fixture; $start.UseShellExecute = $false
    $start.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden; $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $start.Environment.Clear()
    foreach ($pair in (New-ChildEnvironment).GetEnumerator()) { $start.Environment[$pair.Key] = $pair.Value }
    foreach ($pair in $ExtraEnvironment.GetEnumerator()) { $start.Environment[$pair.Key] = $pair.Value }
    if ($NsisLastArgument) {
        if (@($Arguments | Where-Object { $_ -cnotin @('/S', '/UPDATE') }).Count -ne 0 -or $NsisLastArgument -cnotmatch '^(?:/D=|_\?=)') { throw 'Unexpected NSIS arguments.' }
        $start.Arguments = (($Arguments + $NsisLastArgument) -join ' ')
    } else { foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) } }
    $stdoutPath = Join-Path $fixture ($Label + '.stdout.log'); $stderrPath = Join-Path $fixture ($Label + '.stderr.log')
    $stdout = [System.IO.File]::Open($stdoutPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    $stderr = [System.IO.File]::Open($stderrPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    $process = [System.Diagnostics.Process]::new(); $process.StartInfo = $start
    $record = [ordered]@{ label = $Label; executable = $Executable; arguments = $Arguments; nsis_last_argument = $NsisLastArgument;
        started_at = [DateTime]::UtcNow.ToString('o'); status = 'starting'; pid = $null; original_handle_retained = $false; exited = $false; exit_code = $null }
    $entry = [pscustomobject]@{ Process = $process; Handle = $null; Stdout = $stdout; Stderr = $stderr; OutTask = $null; ErrTask = $null; Record = $record; Closed = $false; StdoutPath = $stdoutPath; RecordObservation = $RecordObservation }
    $controlled.Add($entry)
    if ($RecordObservation) { $raw.processes += $record; Save-Raw }
    try {
        if (-not $process.Start()) { throw 'Owned process did not start.' }
        $entry.Handle = $process.SafeHandle
        $record.pid = $process.Id; $record.original_handle_retained = $true; $record.status = 'running'
        $process.StandardInput.Close()
        $entry.OutTask = $process.StandardOutput.BaseStream.CopyToAsync($stdout)
        $entry.ErrTask = $process.StandardError.BaseStream.CopyToAsync($stderr)
        if ($RecordObservation) { Save-Raw }
        return $entry
    } catch { $record.status = 'failed'; $record.error = $_.Exception.Message; throw }
}
function Stop-OwnedProcess($Entry) {
    if ($Entry.Record.original_handle_retained -and -not $Entry.Process.HasExited) {
        # Kill uses this retained original Process object, never a name or a newly looked-up PID.
        $Entry.Process.Kill($true)
        if (-not $Entry.Process.WaitForExit(20000)) { throw 'The original owned process did not terminate.' }
        $Entry.Record.terminated_original_tree = $true
    }
}
function Close-OwnedProcess($Entry) {
    if ($Entry.Closed) { return }
    if ($Entry.Record.original_handle_retained -and -not $Entry.Process.HasExited) { throw 'Refuse to close evidence for a running owned process.' }
    try {
        foreach ($task in @($Entry.OutTask, $Entry.ErrTask)) { if ($task -and -not $task.Wait(5000)) { throw 'Owned process output did not drain.' } }
        if ($Entry.Record.original_handle_retained) {
            $Entry.Record.exited = $Entry.Process.HasExited; $Entry.Record.exit_code = $Entry.Process.ExitCode
            $Entry.Record.finished_at = [DateTime]::UtcNow.ToString('o')
        }
        if ($Entry.Stdout.Length -gt 8388608 -or $Entry.Stderr.Length -gt 8388608) { throw 'Owned process logs exceeded their bound.' }
    } finally { $Entry.Stdout.Dispose(); $Entry.Stderr.Dispose(); $Entry.Closed = $true; if ($Entry.RecordObservation) { Save-Raw } }
}
function Wait-OwnedProcess($Entry, [int]$TimeoutMilliseconds = 120000) {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        while (-not $Entry.Process.WaitForExit(100)) {
            if ($timer.ElapsedMilliseconds -ge $TimeoutMilliseconds) { throw 'Owned process exceeded its deadline.' }
            if ($Entry.Stdout.Length -gt 8388608 -or $Entry.Stderr.Length -gt 8388608) { throw 'Owned process logs exceeded their bound.' }
        }
        if ($Entry.Process.ExitCode -ne 0) { throw "Owned process failed with exit code $($Entry.Process.ExitCode)." }
        $Entry.Record.status = 'passed'
    } catch { $Entry.Record.status = 'failed'; $Entry.Record.error = $_.Exception.Message; throw }
    finally { Stop-OwnedProcess $Entry; Close-OwnedProcess $Entry }
    return [System.IO.File]::ReadAllText($Entry.StdoutPath)
}
function Invoke-OwnedCommand([string]$Executable, [string[]]$Arguments, [string]$Label, [int]$TimeoutMilliseconds = 120000, [string]$NsisLastArgument = '', [bool]$RecordObservation = $true) {
    $entry = Start-OwnedProcess $Executable $Arguments $Label $NsisLastArgument @{} $RecordObservation
    return Wait-OwnedProcess $entry $TimeoutMilliseconds
}
function Invoke-Controller([string]$Command, [string]$Label, [switch]$Policy) {
    $arguments = @($controller, $Command, $ExpectedSource, $ExpectedReleaseId, $fixture)
    if ($Policy) { $arguments += $ExpectedPolicySha256 }
    # The sealer reads a frozen raw observation; its own original process is still awaited,
    # but cannot be part of the report it is currently validating.
    return Invoke-OwnedCommand (Resolve-ApplicationPath 'node') $arguments $Label 120000 '' ($Command -cne 'seal')
}
function Assert-BuildInfo($Info, [string]$Role) {
    if ($Info.component -cne $Role -or $Info.component_version -cne $version -or $Info.source_revision -cne $ExpectedSource -or
        $Info.build_id -cne ($version + '@' + $ExpectedSource) -or $Info.source_state -cne 'verified_checkout' -or
        $Info.revision_origin -cnotin @('explicit_checkout', 'local_git') -or $Info.target -cne 'x86_64-pc-windows-msvc' -or
        $Info.metadata_version -ne 1 -or $Info.database_schema_version -ne 24 -or $Info.protocol_version.major -ne 1 -or $Info.protocol_version.minor -ne 0) { throw 'Actual installed BuildInfo differs from final release policy.' }
    foreach ($number in @($Info.metadata_version, $Info.database_schema_version, $Info.protocol_version.major, $Info.protocol_version.minor)) {
        if ($number -isnot [int] -and $number -isnot [long]) { throw 'BuildInfo numeric fields must be actual JSON integers.' }
    }
    $names = @('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')
    if ($Role -ceq 'service') { $names += 'dzc-service' }
    if (@($Info.linked_components).Count -ne $names.Count) { throw 'Actual linked inventory differs.' }
    foreach ($name in $names) { if (@($Info.linked_components | Where-Object { $_.component -ceq $name -and $_.version -ceq $version }).Count -ne 1) { throw 'Actual linked inventory differs.' } }
}
function Snapshot-WindowsSettings {
    foreach ($target in @(
        @('Software\Microsoft\Clipboard', 'EnableClipboardHistory'), @('Software\Microsoft\Clipboard', 'EnableCloudClipboard'),
        @('Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced', 'DisabledHotkeys'),
        @('Software\Microsoft\Windows\CurrentVersion\Policies\Explorer', 'DisallowClipboardHistory'),
        @('Software\Policies\Microsoft\Windows\System', 'AllowClipboardHistory'), @('Software\Policies\Microsoft\Windows\System', 'AllowCrossDeviceClipboard'))) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($target[0])
        try {
            $exists = $key -and $key.GetValueNames().Contains($target[1])
            [pscustomobject]@{ key = $target[0]; name = $target[1]; exists = [bool]$exists;
                kind = $(if ($exists) { $key.GetValueKind($target[1]).ToString() } else { $null });
                value = $(if ($exists) { $key.GetValue($target[1], $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } else { $null }) }
        } finally { if ($key) { $key.Dispose() } }
    }
}
function Snapshot-Explorer {
    foreach ($process in @(Get-Process explorer -ErrorAction SilentlyContinue | Sort-Object Id)) {
        try { [pscustomobject]@{ pid = $process.Id; started_at = $process.StartTime.ToUniversalTime().ToString('o') } } finally { $process.Dispose() }
    }
}
function Assert-JsonEqual($Before, $After, [string]$Message) {
    if ((ConvertTo-Json -InputObject $Before -Depth 20 -Compress) -cne (ConvertTo-Json -InputObject $After -Depth 20 -Compress)) { throw $Message }
}
function Assert-NoRegisteredInstallation {
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
            $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view); $key = $null
            try { $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\DezirClip'); if ($key) { throw 'A registered DezirClip installation is present.' } }
            finally { if ($key) { $key.Dispose() }; $base.Dispose() }
        }
    }
}
function Assert-FreshMachine {
    if (Get-Process dezirclip, dzc, dzc-service -ErrorAction SilentlyContinue) { throw 'A pre-existing DezirClip process is present.' }
    Assert-NoRegisteredInstallation
    $ownership = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\io.github.qiyuanhuakai.dezirclip\SystemSettingsOwnership\v1')
    try { if ($ownership -and $ownership.GetValueNames().Count -ne 0) { throw 'Pre-existing settings ownership is present.' } } finally { if ($ownership) { $ownership.Dispose() } }
    $roaming = [Environment]::GetFolderPath('ApplicationData')
    if (-not $roaming -or ((Get-Item -LiteralPath $roaming -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { throw 'A regular known-folder RoamingAppData directory is required.' }
    foreach ($identifier in @('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard')) {
        if (Test-Path -LiteralPath (Join-Path $roaming $identifier)) { throw 'A pre-existing known-folder application profile is present.' }
    }
    return $roaming
}
function Assert-InstalledFiles {
    $actual = @()
    foreach ($expected in $raw.packaged) {
        $name = @{ desktop = 'dezirclip.exe'; cli = 'dzc.exe'; service = 'dzc-service.exe' }[$expected.role]
        $path = Resolve-OwnedPath (Join-Path $installDirectory $name) -File
        $record = [ordered]@{ role = $expected.role; bytes = (Get-Item -LiteralPath $path).Length; sha256 = Get-Digest $path }
        if ($record.bytes -ne $expected.bytes -or $record.sha256 -cne $expected.sha256) { throw 'Installed final role differs from its own extracted NSIS bytes.' }
        $actual += $record
    }
    return $actual
}
function Invoke-ProfileCheck([string]$Operation, [string]$Label) {
    $python = Resolve-ApplicationPath 'python'
    $text = Invoke-OwnedCommand $python @('-I', '-B', (Join-Path $fixture 'profile-check.py'), $Operation, $fixture, (Join-Path $PSScriptRoot 'sqlite_history_fixture.py')) $Label
    return $text | ConvertFrom-Json -Depth 30
}

Save-Raw
$failure = $null
try {
    Invoke-Controller 'verify-staged' 'verify-staged' -Policy | Write-Output
    if ((Get-Digest (Join-Path $fixture 'old-setup.exe')) -cne $old.sha256) { throw 'The original installer hash changed.' }
    Pass-Check 'original_installer_hash' @{ path = 'old-setup.exe'; sha256 = Get-Digest (Join-Path $fixture 'old-setup.exe'); expected = $old.sha256 }
    Pass-Check 'candidate_signature_and_hash' @{ verifier = 'verify-staged'; candidate = $inputs.candidate; signature = $inputs.signature }
    Invoke-Controller 'extract' 'extract-packaged-roles' | Write-Output
    $raw.packaged = @(Read-OwnedJson (Join-Path $fixture 'extracted-roles.json'))
    if ($raw.packaged.Count -ne 3 -or (@($raw.packaged.role | Sort-Object) -join ',') -cne 'cli,desktop,service') { throw 'The final NSIS role inventory must be exactly desktop/cli/service.' }
    foreach ($role in $raw.packaged) {
        if (($role.bytes -isnot [long] -and $role.bytes -isnot [int]) -or $role.bytes -le 0 -or $role.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            $role.member -isnot [string] -or -not $role.member -or $role.member.StartsWith('/') -or $role.member.Contains('\') -or $role.member.Contains(':') -or
            $role.member.Contains([char]0) -or @($role.member.Split('/') | Where-Object { $_ -in @('', '.', '..') }).Count -ne 0) { throw 'The extracted final member receipt is invalid.' }
        $extractedPath = Resolve-OwnedPath (Join-Path (Join-Path $fixture 'extracted') $role.member) -File
        if ((Get-Digest $extractedPath) -cne $role.sha256 -or (Get-Item -LiteralPath $extractedPath).Length -ne $role.bytes) { throw 'Extracted final member bytes changed.' }
    }
    $raw.input_sha256 = Get-Digest $inputPath
    $roaming = Assert-FreshMachine
    New-Item -ItemType Directory -Path $profileDirectory | Out-Null
    foreach ($identifier in @('io.github.qiyuanhuakai.dezirclip', 'com.tiez.clipboard')) {
        $directory = Join-Path $roaming $identifier
        New-Item -ItemType Directory -Path $directory | Out-Null
        $redirect = Join-Path $directory 'datapath.txt'
        $created = [pscustomobject]@{ directory = $directory; redirect = $redirect; target = $profileDirectory }
        $redirects.Add($created); $raw.redirects += $created; Save-Raw
        [System.IO.File]::WriteAllText($redirect, $profileDirectory, [System.Text.UTF8Encoding]::new($false))
    }
    Invoke-OwnedCommand (Join-Path $fixture 'old-setup.exe') @('/S') 'install-original-published' 120000 ('/D=' + $installDirectory) | Write-Output
    foreach ($name in @('dezirclip.exe', 'dzc.exe')) { Resolve-OwnedPath (Join-Path $installDirectory $name) -File | Out-Null }
    $oldVersion = Invoke-OwnedCommand (Join-Path $installDirectory 'dzc.exe') @('--version') 'old-cli-version'
    if ($oldVersion.Trim() -cne ('dzc ' + $old.version)) { throw 'The actual historical CLI version differs from its pinned installer.' }
    Pass-Check 'actual_old_install' @{ directory = $installDirectory; cli_version = $oldVersion.Trim(); installer_sha256 = $old.sha256 }
    if (Test-Path -LiteralPath $database) { throw 'Historical profile must be created by the actual old CLI.' }
    Invoke-OwnedCommand (Join-Path $installDirectory 'dzc.exe') @('add', $marker) 'old-cli-create-profile' | Write-Output
    Resolve-OwnedPath $database -File | Out-Null
    if ($inputs.scenario -ceq 'PublishedAlphaToAlpha2') {
        $oldInfo = (Invoke-OwnedCommand (Join-Path $installDirectory 'dzc.exe') @('build-info', '--json') 'old-cli-build-info') | ConvertFrom-Json -Depth 20
        if ($oldInfo.component -cne 'cli' -or $oldInfo.component_version -cne $old.version -or $oldInfo.source_revision -cne $old.source_revision -or
            $oldInfo.build_id -cne ($old.version + '@' + $old.source_revision) -or $oldInfo.source_state -cne 'verified_checkout' -or
            $oldInfo.database_schema_version -ne 21 -or $oldInfo.target -cne 'x86_64-pc-windows-msvc' -or @($oldInfo.linked_components).Count -ne 3) { throw 'Actual published alpha.1 CLI source/schema/inventory differs.' }
        foreach ($name in @('dzc-core', 'dzc-platform', 'dzc-protocol')) { if (@($oldInfo.linked_components | Where-Object { $_.component -ceq $name -and $_.version -ceq $old.version }).Count -ne 1) { throw 'Historical alpha.1 linked inventory differs.' } }
        Save-Json (Join-Path $fixture 'old-build-info.json') $oldInfo
    }
    $profileProgram = @'
import importlib.util, json, os, pathlib, sqlite3, sys
operation, fixture_arg, helper_arg = sys.argv[1:]
fixture = pathlib.Path(fixture_arg).resolve(strict=True)
assert fixture.parent == pathlib.Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
inputs = json.loads((fixture/'inputs.json').read_text(encoding='utf-8-sig'))
database = fixture/'profile/clipboard.db'
assert database.is_file() and not database.is_symlink()
marker = inputs['marker']
frozen = {'app.quick_paste_hotkey':'Ctrl+Alt+Shift+F9','app.classification_enabled':'false','app.hidden_filter_chips':'release-upgrade-fixture','app.screenshot_enabled':'false','app.ocr_enabled':'false'}
spec = importlib.util.spec_from_file_location('owned_fixture_sqlite', helper_arg)
helper = importlib.util.module_from_spec(spec); spec.loader.exec_module(helper)
connection = helper.connect_fixture_database(database.as_uri()+'?mode='+('rw' if operation=='seed' else 'ro'), uri=True)
with connection:
    assert connection.execute('PRAGMA quick_check').fetchone()==('ok',)
    version = connection.execute('SELECT MAX(version) FROM schema_migrations').fetchone()[0]
    rows = connection.execute('SELECT id,content,content_type,tags,is_pinned FROM clipboard_history WHERE content=?',(marker,)).fetchall()
    assert len(rows)==1 and rows[0][2]=='text'
    entry_id = rows[0][0]
    if operation=='seed':
        assert not (fixture/'old-profile-backup.db').exists()
        assert isinstance(version,int) and 0<version<=21
        if inputs['scenario']=='PublishedAlphaToAlpha2': assert version==21
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',frozen.items())
        connection.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',{'app.update_auto_check':'false','app.silent_start':'false','app.idle_destroy_enabled':'false','app.sequential_mode':'false'}.items())
        connection.execute('UPDATE clipboard_history SET is_pinned=1,tags=? WHERE id=?',(json.dumps(['releaseupgradetag']),entry_id))
        tables={row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if 'entry_tags' in tables: connection.execute('INSERT OR IGNORE INTO entry_tags(entry_id,tag) VALUES(?,?)',(entry_id,'releaseupgradetag'))
    else:
        assert operation=='check'
        assert version==24
        before=json.loads((fixture/'old-profile.json').read_text(encoding='utf-8'))
        assert entry_id==before['marker']['id']
    connection.commit()
    row=connection.execute('SELECT id,content,content_type,tags,is_pinned FROM clipboard_history WHERE id=?',(entry_id,)).fetchone()
    assert row[1]==marker and row[2]=='text' and row[4]==1 and json.loads(row[3])==['releaseupgradetag']
    settings=dict(connection.execute('SELECT key,value FROM settings'))
    assert all(settings.get(key)==value for key,value in frozen.items())
    result={'status':'passed','database':str(database),'schema_version':version,'frozen_preferences':{key:settings[key] for key in frozen},'marker':dict(zip(('id','content','content_type','tags','is_pinned'),row)),'sqlite_version':sqlite3.sqlite_version}
    if operation=='seed':
        result['settings']=settings
        result['schema']=[list(row) for row in connection.execute('SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name')]
        result['migration_versions']=[row[0] for row in connection.execute('SELECT version FROM schema_migrations ORDER BY version')]
        with sqlite3.connect(fixture/'old-profile-backup.db') as backup: connection.backup(backup)
        (fixture/'old-profile.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print(json.dumps(result))
'@
    [System.IO.File]::WriteAllText((Join-Path $fixture 'profile-check.py'), $profileProgram, [System.Text.UTF8Encoding]::new($false))
    $oldProfile = Invoke-ProfileCheck 'seed' 'seed-owned-old-profile'
    $raw.old_profile = $oldProfile
    $raw.old_profile_backup_sha256 = Get-Digest (Join-Path $fixture 'old-profile-backup.db')
    Pass-Check 'actual_old_profile_created' @{ actual = $oldProfile; backup_sha256 = $raw.old_profile_backup_sha256 }
    $registryOriginal = @(Snapshot-WindowsSettings)
    Save-Json (Join-Path $fixture 'windows-settings-original.json') $registryOriginal
    foreach ($target in $registryOriginal) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($target.key)
        try {
            if ($target.name -ceq 'DisabledHotkeys') { $key.SetValue($target.name, 'XV', [Microsoft.Win32.RegistryValueKind]::String) }
            else { $key.SetValue($target.name, 7, [Microsoft.Win32.RegistryValueKind]::DWord) }
        } finally { $key.Dispose() }
    }
    $settingsBefore = @(Snapshot-WindowsSettings); $explorerBefore = @(Snapshot-Explorer)
    Save-Json (Join-Path $fixture 'windows-settings-before-upgrade.json') $settingsBefore
    Save-Json (Join-Path $fixture 'explorer-before-upgrade.json') $explorerBefore
    if ($explorerBefore.Count -eq 0) { throw 'Explorer preservation requires at least one active original Explorer before upgrading.' }
    Invoke-OwnedCommand (Join-Path $fixture 'candidate-setup.exe') @('/S', '/UPDATE') 'install-final-release-upgrade' 120000 ('/D=' + $installDirectory) | Write-Output
    Pass-Check 'actual_upgrade_same_directory' @{ old_directory = $installDirectory; final_directory = $installDirectory; arguments = @('/S', '/UPDATE', ('/D=' + $installDirectory)); installer_sha256 = Get-Digest (Join-Path $fixture 'candidate-setup.exe') }
    $raw.installed = @(Assert-InstalledFiles)
    Pass-Check 'installed_three_role_bytes' $raw.installed
    $cli = Join-Path $installDirectory 'dzc.exe'; $service = Join-Path $installDirectory 'dzc-service.exe'
    $raw.roles.cli = (Invoke-OwnedCommand $cli @('build-info', '--json') 'final-cli-build-info') | ConvertFrom-Json -Depth 20
    $raw.roles.service = (Invoke-OwnedCommand $service @('--build-info') 'final-service-build-info') | ConvertFrom-Json -Depth 20
    Assert-BuildInfo $raw.roles.cli 'cli'; Assert-BuildInfo $raw.roles.service 'service'
    $cliHistory = (Invoke-OwnedCommand $cli @('search', $marker, '--json') 'final-cli-retained-history') | ConvertFrom-Json -Depth 20
    $cliMarker = @($cliHistory | Where-Object { $_.content -ceq $marker })
    if ($cliMarker.Count -ne 1 -or $cliMarker[0].id -ne $oldProfile.marker.id) { throw 'The actual final CLI did not retain the original marker ID.' }
    $upgradedProfile = Invoke-ProfileCheck 'check' 'upgraded-profile-check'
    Save-Json (Join-Path $fixture 'upgraded-profile.json') $upgradedProfile
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $guiInput = Join-Path $fixture 'final-gui-input.json'; $guiEvidence = Join-Path $fixture 'final-gui-evidence.json'
    Save-Json $guiInput @{ expectedVersion = $version; expectedBuildInfo = $raw.roles.cli; profileDirectory = $profileDirectory; marker = $marker; acquireOwnership = $false; verifyCapture = $false }
    $webviewProfile = Join-Path $fixture 'webview-profile'
    [System.IO.Directory]::CreateDirectory((Resolve-OwnedPath $webviewProfile)) | Out-Null
    $webviewHelper = Join-Path $PSScriptRoot 'release_windows_webview.py'
    Invoke-OwnedCommand (Resolve-ApplicationPath 'python') @('-B', $webviewHelper, 'configure', '--source', $ExpectedSource, '--root', $fixture,
        '--profile', $webviewProfile, '--port', $port.ToString(), '--state', $webviewPolicy) 'configure-final-webview' 60000 | Write-Output
    $raw.webview_policy = 'final-webview-policy.json'
    $gui = Start-OwnedProcess (Join-Path $installDirectory 'dezirclip.exe') @() 'actual-final-gui' '' @{ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = ('--remote-debugging-port=' + $port) }
    try {
        $identity = [ordered]@{ expected_path = Join-Path $installDirectory 'dezirclip.exe'; verified = $false; attempts = @() }
        $raw.gui_process_identity = $identity
        Save-Raw
        for ($attempt = 0; $attempt -lt 41; $attempt++) {
            if ($gui.Process.HasExited) { throw 'Actual installed GUI exited before identity verification.' }
            $fresh = $null; $observed = $null; $errorText = $null
            try { $fresh = Get-Process -Id $gui.Process.Id -ErrorAction Stop; $observed = $fresh.Path }
            catch { $errorText = $_.Exception.Message }
            finally { if ($fresh) { $fresh.Dispose() } }
            $originalExited = $gui.Process.HasExited
            $identity.attempts += @{ attempt = $attempt; observed_path = $observed; original_exited = $originalExited; error = $errorText }
            if ($originalExited) { throw 'Original GUI process exited during image verification.' }
            if ($observed) {
                if ($observed -ine $identity.expected_path) { throw 'Original GUI image path differs from the installed executable.' }
                $identity.verified = $true; break
            }
            Start-Sleep -Milliseconds 50
        }
        Save-Json (Join-Path $fixture 'final-gui-process-identity.json') $identity
        if (-not $identity.verified) { throw 'Actual installed GUI image identity was unavailable.' }
        Invoke-OwnedCommand (Resolve-ApplicationPath 'python') @('-B', $webviewHelper, 'inspect', '--source', $ExpectedSource, '--root', $fixture,
            '--state', $webviewPolicy, '--pid', $gui.Process.Id.ToString(), '--executable', $identity.expected_path) 'inspect-final-webview-token' 60000 | Write-Output
        Invoke-OwnedCommand (Resolve-ApplicationPath 'node') @((Join-Path $PSScriptRoot 'tests/native-windows-ownership.mjs'), $port.ToString(), $guiEvidence, $guiInput) 'actual-final-gui-bridge' 180000 | Write-Output
        $observed = Read-OwnedJson $guiEvidence 8388608
        if ($observed.result -cne 'passed' -or $observed.gui.oldMarkerId -ne $oldProfile.marker.id -or -not $observed.gui.oldHistoryRendered -or
            [System.IO.Path]::GetFullPath($observed.gui.dataPath).TrimEnd('\') -ine $profileDirectory.TrimEnd('\') -or @($observed.nativeClipboardWrites).Count -ne 0) { throw 'Actual final GUI did not confirm the original private profile and rendered marker.' }
        $raw.roles.desktop = $observed.gui.buildInfo
        Assert-BuildInfo $raw.roles.desktop 'desktop'
        $gui.Record.status = 'passed'
    } finally {
        Stop-OwnedProcess $gui; Close-OwnedProcess $gui
        $portFree = $false
        for ($attempt = 0; $attempt -lt 50; $attempt++) {
            $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
            try { $probe.Start(); $portFree = $true; break }
            catch { Start-Sleep -Milliseconds 100 }
            finally { $probe.Stop() }
        }
        $raw.gui_exit = @{ original_process_exited = $gui.Process.HasExited; cdp_port = $port; cdp_port_released = $portFree }
        Save-Raw
        if (-not $portFree) { throw 'The owned GUI process tree did not release its private CDP port.' }
    }
    Invoke-OwnedCommand (Resolve-ApplicationPath 'python') @('-B', $webviewHelper, 'restore', '--source', $ExpectedSource,
        '--root', $fixture, '--state', $webviewPolicy) 'restore-final-webview' 60000 | Write-Output
    $webviewPolicyRestored = $true
    $raw.installed = @(Assert-InstalledFiles)
    Pass-Check 'actual_three_role_build_info' $raw.roles
    $afterGui = Invoke-ProfileCheck 'check' 'after-gui-profile-check'
    Pass-Check 'history_and_schema24' @{ before = $oldProfile.marker; after = $afterGui; actual_gui_evidence = 'final-gui-evidence.json' }
    Pass-Check 'frozen_preferences_preserved' @{ before = $oldProfile.frozen_preferences; after = $afterGui.frozen_preferences }
    $settingsAfter = @(Snapshot-WindowsSettings); $explorerAfter = @(Snapshot-Explorer)
    Save-Json (Join-Path $fixture 'windows-settings-after-upgrade.json') $settingsAfter
    Save-Json (Join-Path $fixture 'explorer-after-upgrade.json') $explorerAfter
    Assert-JsonEqual $settingsBefore $settingsAfter 'The final release upgrade or GUI changed pre-existing Windows settings.'
    Assert-JsonEqual $explorerBefore $explorerAfter 'The final release upgrade or GUI restarted or spawned Explorer.'
    Pass-Check 'windows_settings_preserved' @{ before = $settingsBefore; after = $settingsAfter }
    $uninstaller = Resolve-OwnedPath (Join-Path $installDirectory 'uninstall.exe') -File
    $raw.uninstaller_sha256 = Get-Digest $uninstaller
    Invoke-OwnedCommand $uninstaller @('/S') 'uninstall-actual-final-release' 120000 ('_?=' + $installDirectory) | Write-Output
    foreach ($name in @('dezirclip.exe', 'dzc.exe', 'dzc-service.exe')) { if (Test-Path -LiteralPath (Join-Path $installDirectory $name)) { throw 'The actual final release uninstall left a role executable.' } }
    Assert-NoRegisteredInstallation
    Pass-Check 'actual_release_uninstall' @{ uninstaller_sha256 = $raw.uninstaller_sha256; missing = @('dezirclip.exe', 'dzc.exe', 'dzc-service.exe'); registered_installation_absent = $true }
    $afterUninstall = Invoke-ProfileCheck 'check' 'uninstalled-profile-check'
    $settingsFinal = @(Snapshot-WindowsSettings); $explorerFinal = @(Snapshot-Explorer)
    Save-Json (Join-Path $fixture 'windows-settings-after-uninstall.json') $settingsFinal
    Save-Json (Join-Path $fixture 'explorer-after-uninstall.json') $explorerFinal
    Assert-JsonEqual $settingsBefore $settingsFinal 'The final release uninstall changed the synthetic external Windows settings.'
    Assert-JsonEqual $explorerBefore $explorerFinal 'The final release uninstall restarted or spawned Explorer.'
    Pass-Check 'explorer_not_restarted' @{ before = $explorerBefore; after_upgrade = $explorerAfter; after_uninstall = $explorerFinal; had_existing_explorer = ($explorerBefore.Count -gt 0) }
    Pass-Check 'uninstall_preserves_profile_and_settings' @{ profile = $afterUninstall; windows_settings = $settingsFinal }
    if ((Get-Digest $inputPath) -cne $raw.input_sha256 -or (Get-Digest (Join-Path $fixture 'old-setup.exe')) -cne $old.sha256 -or
        (Get-Digest (Join-Path $fixture 'candidate-setup.exe')) -cne $inputs.candidate.sha256 -or
        (Get-Digest (Join-Path $fixture 'old-profile-backup.db')) -cne $raw.old_profile_backup_sha256) { throw 'An original input or preserved old backup changed.' }
    foreach ($entry in $controlled) { if (-not $entry.Record.original_handle_retained -or -not $entry.Record.exited -or -not $entry.Process.HasExited -or -not $entry.Closed) { throw 'An original controlled process was not confirmed exited.' } }
    Pass-Check 'controlled_processes_exited' @{ count = $controlled.Count; original_handles_verified = $true }
} catch { $failure = $_; $raw.failure = $_.Exception.ToString() }
finally {
    foreach ($entry in $controlled) {
        try { Stop-OwnedProcess $entry; Close-OwnedProcess $entry }
        catch { if (-not $failure) { $failure = $_ }; $raw.cleanup += @{ process = $entry.Record.label; error = $_.Exception.ToString() } }
    }
    if (-not $webviewPolicyRestored -and (Test-Path -LiteralPath $webviewPolicy -PathType Leaf)) {
        try {
            Invoke-OwnedCommand (Resolve-ApplicationPath 'python') @('-B', (Join-Path $PSScriptRoot 'release_windows_webview.py'), 'restore',
                '--source', $ExpectedSource, '--root', $fixture, '--state', $webviewPolicy) 'restore-final-webview-after-failure' 60000 | Write-Output
            $webviewPolicyRestored = $true
        } catch {
            if (-not $failure) { $failure = $_ }
            $raw.cleanup += @{ webview_policy = 'final-webview-policy.json'; error = $_.Exception.ToString() }
        }
    }
    foreach ($created in $redirects) {
        try {
            if (-not (Test-Path -LiteralPath $created.directory)) {
                $raw.cleanup += @{ redirect = $created.redirect; status = 'already_absent' }
                continue
            }
            if ((Get-Item -LiteralPath $created.directory -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Created redirect directory changed into a reparse point.' }
            if (Test-Path -LiteralPath $created.redirect) {
                if (((Get-Item -LiteralPath $created.redirect -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint) -or
                    [System.IO.File]::ReadAllText($created.redirect) -cne $profileDirectory) { throw 'Created redirect changed; preserve it for runner disposal.' }
                [System.IO.File]::Delete($created.redirect)
            }
            if (@(Get-ChildItem -LiteralPath $created.directory -Force).Count -eq 0) { [System.IO.Directory]::Delete($created.directory, $false) }
            $raw.cleanup += @{ redirect = $created.redirect; status = 'removed_only_owned_redirect' }
        } catch { if (-not $failure) { $failure = $_ }; $raw.cleanup += @{ redirect = $created.redirect; error = $_.Exception.ToString() } }
    }
    $raw.finished_at = [DateTime]::UtcNow.ToString('o')
    if ($failure) { $raw.status = 'failed'; $raw.failure = $failure.Exception.ToString() }
    elseif (@($raw.checks | Where-Object { $_.status -cne 'passed' }).Count -eq 0) { $raw.status = 'passed'; $raw.failure = $null }
    else { $raw.status = 'failed'; $raw.failure = 'At least one required native check did not run.' }
    Save-Raw
}
if ($failure) { throw $failure }
if ($raw.status -cne 'passed') { throw $raw.failure }
# Only the independent controller can validate and seal this raw observation into a gate report.
try { Invoke-Controller 'seal' 'seal-native-report' | Write-Output }
catch { $raw.status = 'failed'; $raw.failure = $_.Exception.ToString(); Save-Raw; throw }
finally { foreach ($entry in $controlled) { $entry.Process.Dispose() } }
Write-Output 'Final release installer upgrade observations completed; no debug fixture results were substituted.'
