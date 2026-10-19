param([Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT' -or -not $env:RUNNER_TEMP) {
    throw 'Native fixture builds run only in disposable GitHub-hosted Windows jobs.'
}
if ($Version -notin @('0.2.0-alpha.0', '0.2.0-alpha.1')) { throw 'Only unpublished fixture versions are allowed.' }
function Assert-NoReparsePoint([string]$Path, [string]$Boundary) {
    $current = $Path
    while ($current.Length -ge $Boundary.Length) {
        if (Test-Path -LiteralPath $current) {
            if ((Get-Item -LiteralPath $current -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Fixture paths cannot cross a reparse point.' }
        }
        if ($current -ieq $Boundary) { break }
        $current = Split-Path -Parent $current
    }
}
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$temporaryRoot = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/')
$temporaryPrefix = $temporaryRoot + [System.IO.Path]::DirectorySeparatorChar
if (-not $destinationPath.StartsWith($temporaryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture installers must remain beneath RUNNER_TEMP.' }
Assert-NoReparsePoint $destinationPath $temporaryRoot
if ((Test-Path -LiteralPath $destinationPath) -or (Test-Path -LiteralPath ($destinationPath + '.manifest.json'))) { throw 'Fixture output already exists; use a new destination.' }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $destinationPath) -PathType Container)) { throw 'Fixture destination parent is missing.' }
& node scripts/set-release-version.mjs $Version
if ($LASTEXITCODE -ne 0) { throw 'Fixture version update failed.' }
& cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin dzc
if ($LASTEXITCODE -ne 0) { throw 'Fixture CLI build failed.' }
$metadata = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Fixture metadata failed.' }
$triple = (& rustc --print host-tuple).Trim()
if ($LASTEXITCODE -ne 0 -or $triple -ne 'x86_64-pc-windows-msvc') { throw 'Native NSIS fixtures require the Windows x64 MSVC host.' }
New-Item -ItemType Directory -Force -Path src-tauri/binaries | Out-Null
$sidecar = "src-tauri/binaries/dzc-$triple.exe"
Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'debug/dzc.exe') -Destination $sidecar -Force
$sidecarDigest = (Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash
$sidecarVersion = (& $sidecar --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $sidecarVersion -notmatch ('(?:^|\s)' + [regex]::Escape($Version) + '(?:\s|$)')) { throw 'Packaged standalone CLI has the wrong version.' }
$override = Join-Path $env:RUNNER_TEMP ('dezirclip-native-fixture-' + [guid]::NewGuid().ToString('N') + '.json')
$configuration = Get-Content -LiteralPath src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
$configuration.app.windows[0] | Add-Member -NotePropertyName additionalBrowserArgs -NotePropertyValue '--remote-debugging-port=9222' -Force
@{build=@{beforeBuildCommand='pnpm run build'};bundle=@{createUpdaterArtifacts=$false};app=@{windows=$configuration.app.windows}} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $override -Encoding utf8
& pnpm exec tauri build --debug --bundles nsis --config $override
if ($LASTEXITCODE -ne 0) { throw 'Fixture NSIS build failed.' }
$candidate = Join-Path $metadata.target_directory "debug/bundle/nsis/DezirClip_${Version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw 'Fixture installer was not generated.' }
Copy-Item -LiteralPath $candidate -Destination $destinationPath -Force
$application = Join-Path $metadata.target_directory 'debug/dezirclip.exe'
foreach ($binary in @($application, $sidecar)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Fixture build did not retain both candidate binaries.' }
}
if ((Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash -ne $sidecarDigest) { throw 'The staged standalone CLI changed during the desktop build.' }
$sourceCommit = (& git rev-parse HEAD | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $sourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Fixture source commit is unavailable.' }
@{
    schemaVersion = 1; kind = 'dezirclip-native-nsis-fixture'; version = $Version; target = $triple
    installerSha256 = (Get-FileHash -LiteralPath $destinationPath -Algorithm SHA256).Hash
    applicationSha256 = (Get-FileHash -LiteralPath $application -Algorithm SHA256).Hash
    sidecarSha256 = (Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash
    sidecarVersion = $sidecarVersion; sourceCommit = $sourceCommit
    cargoLockSha256 = (Get-FileHash -LiteralPath Cargo.lock -Algorithm SHA256).Hash
    configurationSha256 = (Get-FileHash -LiteralPath src-tauri/tauri.conf.json -Algorithm SHA256).Hash
    installerHookSha256 = (Get-FileHash -LiteralPath src-tauri/nsis/uninstall.nsh -Algorithm SHA256).Hash
    profile = 'debug'; updaterArtifacts = $false
} | ConvertTo-Json | Set-Content -LiteralPath ($destinationPath + '.manifest.json') -Encoding utf8
