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
function Get-NsisApplicationProof([string]$Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $rawSha256 = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes))
    $unknown = '__TAURI_BUNDLE_TYPE_VAR_UNK'
    $nsis = [System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS')
    $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
    $offset = $ascii.IndexOf($unknown, [System.StringComparison]::Ordinal)
    if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a -or $offset -lt 64 -or $ascii.IndexOf($unknown, $offset + $unknown.Length, [System.StringComparison]::Ordinal) -ne -1) {
        throw 'Expected exactly one Tauri bundle-type placeholder in the compiled Windows GUI.'
    }
    # Tauri 2.12.3 patches this token for NSIS, then restores the build output.
    # Predict the exact packaged bytes without changing the compiled executable.
    [Array]::Copy($nsis, 0, $bytes, $offset, $nsis.Length)
    return [pscustomobject]@{
        rawSha256 = $rawSha256
        packagedSha256 = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes))
        patchOffset = $offset
    }
}
function Get-NsisCompilerInputs([string]$Script, [string]$Application, [string]$CargoBinary, [string]$ExpectedSha256, [ValidateSet('dzc.exe', 'dzc-service.exe')][string]$Destination = 'dzc.exe') {
    if ($ExpectedSha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Expected a complete standalone CLI SHA-256.' }
    $text = [System.IO.File]::ReadAllText($Script)
    $main = [regex]::Matches($text, '(?m)^\s*!define MAINBINARYSRCPATH "([^"]+)"\s*$')
    if ($main.Count -ne 1 -or [System.IO.Path]::GetFullPath($main[0].Groups[1].Value) -ine [System.IO.Path]::GetFullPath($Application)) { throw 'NSIS did not select the proven GUI build.' }
    $entries = [regex]::Matches($text, ('(?im)^\s*File\s+/a\s+"/oname=' + [regex]::Escape($Destination) + '"\s+"([^"]+)"\s*$'))
    if ($entries.Count -ne 1) { throw 'Expected exactly one NSIS Cargo CLI input; investigate any bundler layout change.' }
    $source = [System.IO.Path]::GetFullPath($entries[0].Groups[1].Value)
    if ($source -ine [System.IO.Path]::GetFullPath($CargoBinary)) { throw 'NSIS selected a CLI source other than the exact Cargo binary.' }
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $ExpectedSha256) { throw 'NSIS selected a CLI other than the proven standalone build.' }
    [pscustomobject]@{ source = $source; destination = $Destination; sha256 = $ExpectedSha256; role = 'cargo-binary' }
}
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$temporaryRoot = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/')
$temporaryPrefix = $temporaryRoot + [System.IO.Path]::DirectorySeparatorChar
if (-not $destinationPath.StartsWith($temporaryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture installers must remain beneath RUNNER_TEMP.' }
Assert-NoReparsePoint $destinationPath $temporaryRoot
if ((Test-Path -LiteralPath $destinationPath) -or (Test-Path -LiteralPath ($destinationPath + '.manifest.json')) -or (Test-Path -LiteralPath ($destinationPath + '.nsi'))) { throw 'Fixture output already exists; use a new destination.' }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $destinationPath) -PathType Container)) { throw 'Fixture destination parent is missing.' }
& node scripts/set-release-version.mjs $Version
if ($LASTEXITCODE -ne 0) { throw 'Fixture version update failed.' }
$metadata = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Fixture metadata failed.' }
$triple = (& rustc --print host-tuple).Trim()
if ($LASTEXITCODE -ne 0 -or $triple -ne 'x86_64-pc-windows-msvc') { throw 'Native NSIS fixtures require the Windows x64 MSVC host.' }
$sidecar = "src-tauri/binaries/dzc-$triple.exe"
$override = Join-Path $env:RUNNER_TEMP ('dezirclip-native-fixture-' + [guid]::NewGuid().ToString('N') + '.json')
$configuration = Get-Content -LiteralPath src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
if ($configuration.build.beforeBundleCommand -ne 'pnpm run tauri:prepare-sidecars --phase bundle') { throw 'Native acceptance requires the production shared beforeBundle sidecar hook.' }
$configuration.app.windows[0] | Add-Member -NotePropertyName additionalBrowserArgs -NotePropertyValue '--remote-debugging-port=9222' -Force
@{bundle=@{createUpdaterArtifacts=$false};app=@{windows=$configuration.app.windows}} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $override -Encoding utf8
$buildStarted = [DateTime]::UtcNow
& pnpm exec tauri build --debug --bundles nsis --no-sign --config $override -- --locked
if ($LASTEXITCODE -ne 0) { throw 'Fixture desktop/NSIS build failed.' }
$application = Join-Path $metadata.target_directory 'debug/dezirclip.exe'
$cargoSidecar = Join-Path $metadata.target_directory 'debug/dzc.exe'
foreach ($binary in @($application, $sidecar)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Fixture build did not retain both candidate binaries.' }
}
$applicationProof = Get-NsisApplicationProof $application
$sidecarProof = Get-Content -LiteralPath ($sidecar + '.proof.json') -Raw | ConvertFrom-Json
if ($sidecarProof.schemaVersion -ne 1 -or $sidecarProof.kind -ne 'dezirclip-standalone-sidecar' -or $sidecarProof.phase -ne 'bundle' -or $sidecarProof.profile -ne 'debug' -or $sidecarProof.portable -ne $false -or $sidecarProof.target -ne $triple -or $sidecarProof.host -ne $triple) { throw 'Shared beforeBundle hook did not produce the expected native debug sidecar.' }
if ([DateTimeOffset]::Parse($sidecarProof.createdAt).UtcDateTime -lt $buildStarted) { throw 'Sidecar preparation evidence predates this build.' }
$expectedCommand = @('cargo', 'build', '--manifest-path', 'src-tauri/Cargo.toml', '--bin', 'dzc', '--locked', '--no-default-features', '--target', $triple)
if ((ConvertTo-Json -InputObject @($sidecarProof.command) -Compress) -ne (ConvertTo-Json -InputObject $expectedCommand -Compress)) { throw 'Sidecar was not built with the expected standalone Cargo arguments.' }
if ($sidecarProof.helperSha256 -ne (Get-FileHash -LiteralPath scripts/prepare-tauri-sidecars.mjs -Algorithm SHA256).Hash -or $sidecarProof.cargoLockSha256 -ne (Get-FileHash -LiteralPath Cargo.lock -Algorithm SHA256).Hash) { throw 'Shared sidecar preparation evidence does not match the build sources.' }
$sidecarDigest = $sidecarProof.sha256
if ($sidecarDigest -notmatch '^[a-fA-F0-9]{64}$' -or @($sidecarProof.binaryPaths).Count -ne 3) { throw 'Sidecar input hashes are missing.' }
$expectedPaths = @([System.IO.Path]::GetFullPath($sidecar), [System.IO.Path]::GetFullPath($cargoSidecar), [System.IO.Path]::GetFullPath((Join-Path $metadata.target_directory "$triple/debug/dzc.exe")))
$observedPaths = @()
foreach ($compilerInput in $sidecarProof.binaryPaths) {
    $path = [System.IO.Path]::GetFullPath($compilerInput.path)
    if ($path -notin $expectedPaths -or $path -in $observedPaths -or $compilerInput.sha256 -ne $sidecarDigest -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $sidecarDigest) { throw 'A sidecar source differs from the shared standalone build.' }
    $observedPaths += $path
}
$sidecarVersion = (& $sidecar --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $sidecarVersion -notmatch ('(?:^|\s)' + [regex]::Escape($Version) + '(?:\s|$)')) { throw 'Packaged standalone CLI has the wrong version.' }
$buildInfo = $sidecarProof.buildInfo
if ($sidecarProof.buildInfoVerification -ne 'executed_native_cli' -or $buildInfo.component -ne 'cli' -or $buildInfo.component_version -ne $Version -or $buildInfo.source_revision -notmatch '^[a-fA-F0-9]{40}$' -or $buildInfo.build_id -ne ($Version + '@' + $buildInfo.source_revision)) { throw 'Fixture CLI lacks matching runtime build identity.' }
$candidate = Join-Path $metadata.target_directory "debug/bundle/nsis/DezirClip_${Version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw 'Fixture installer was not generated.' }
Copy-Item -LiteralPath $candidate -Destination $destinationPath -Force
if ((Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash -ne $sidecarDigest) { throw 'The staged standalone CLI changed during the desktop build.' }
if ((Get-FileHash -LiteralPath $cargoSidecar -Algorithm SHA256).Hash -ne $sidecarDigest) { throw 'The Cargo sidecar input changed during bundling.' }
if ((Get-FileHash -LiteralPath $application -Algorithm SHA256).Hash -ne $applicationProof.rawSha256) { throw 'Tauri did not restore the original GUI after bundling.' }
$nsisScript = Join-Path $metadata.target_directory 'debug/nsis/x64/installer.nsi'
$compilerInputs = @(Get-NsisCompilerInputs $nsisScript $application $cargoSidecar $sidecarDigest)
$serviceProof = $sidecarProof.serviceHelper
$serviceBinary = Join-Path $metadata.target_directory 'debug/dzc-service.exe'
if ($serviceProof.buildInfoVerification -ne 'executed_native_service' -or $serviceProof.buildInfo.component -ne 'service' -or $serviceProof.buildInfo.build_id -ne $buildInfo.build_id -or $serviceProof.buildInfo.database_schema_version -ne $buildInfo.database_schema_version) { throw 'Packaged service has a different build set.' }
if (@($serviceProof.binaryPaths).Count -ne 3) { throw 'Service requires three synchronized native build inputs.' }
$expectedServicePaths = @([System.IO.Path]::GetFullPath("src-tauri/binaries/dzc-service-$triple.exe"), [System.IO.Path]::GetFullPath($serviceBinary), [System.IO.Path]::GetFullPath((Join-Path $metadata.target_directory "$triple/debug/dzc-service.exe")))
$observedServicePaths = @()
foreach ($compilerInput in $serviceProof.binaryPaths) {
    $path = [System.IO.Path]::GetFullPath($compilerInput.path)
    if ($path -notin $expectedServicePaths -or $path -in $observedServicePaths -or $compilerInput.sha256 -ne $serviceProof.sha256 -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $serviceProof.sha256) { throw 'A service source differs from the shared helper build.' }
    $observedServicePaths += $path
}
$serviceInputs = @(Get-NsisCompilerInputs $nsisScript $application $serviceBinary $serviceProof.sha256 'dzc-service.exe')
Copy-Item -LiteralPath $nsisScript -Destination ($destinationPath + '.nsi')
$sourceCommit = (& git rev-parse HEAD | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $sourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Fixture source commit is unavailable.' }
@{
    schemaVersion = 4; kind = 'dezirclip-native-nsis-fixture'; version = $Version; target = $triple
    installerSha256 = (Get-FileHash -LiteralPath $destinationPath -Algorithm SHA256).Hash
    applicationSha256 = $applicationProof.packagedSha256
    applicationRawSha256 = $applicationProof.rawSha256; applicationPatchOffset = $applicationProof.patchOffset
    applicationPatch = 'tauri-nsis-bundle-token-v1'
    sidecarSha256 = (Get-FileHash -LiteralPath $sidecar -Algorithm SHA256).Hash
    sidecarVersion = $sidecarVersion; sidecarSource = 'standalone-no-default-features'
    nsisInputLayout = 'cargo-cli-and-service-v1'; nsisInputs = $compilerInputs
    sidecarPreparation = $sidecarProof
    serviceSha256 = $serviceProof.sha256; serviceBuildInfo = $serviceProof.buildInfo; serviceInputs = $serviceInputs
    buildInfo = $buildInfo
    nsisScriptSha256 = (Get-FileHash -LiteralPath ($destinationPath + '.nsi') -Algorithm SHA256).Hash
    overrideConfigurationSha256 = (Get-FileHash -LiteralPath $override -Algorithm SHA256).Hash
    sourceCommit = $sourceCommit; packaging = 'shared-before-build-and-before-bundle'
    cargoLockSha256 = (Get-FileHash -LiteralPath Cargo.lock -Algorithm SHA256).Hash
    configurationSha256 = (Get-FileHash -LiteralPath src-tauri/tauri.conf.json -Algorithm SHA256).Hash
    installerHookSha256 = (Get-FileHash -LiteralPath src-tauri/nsis/uninstall.nsh -Algorithm SHA256).Hash
    profile = 'debug'; updaterArtifacts = $false
} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath ($destinationPath + '.manifest.json') -Encoding utf8
