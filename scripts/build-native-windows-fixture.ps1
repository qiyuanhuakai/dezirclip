param([Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
    throw 'Native fixture builds run only in disposable GitHub-hosted Windows jobs.'
}
if ($Version -notin @('0.2.0-alpha.0', '0.2.0-alpha.1')) { throw 'Only unpublished fixture versions are allowed.' }
& node scripts/set-release-version.mjs $Version
if ($LASTEXITCODE -ne 0) { throw 'Fixture version update failed.' }
& cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin dzc
if ($LASTEXITCODE -ne 0) { throw 'Fixture CLI build failed.' }
$metadata = & cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Fixture metadata failed.' }
$triple = (& rustc --print host-tuple).Trim()
New-Item -ItemType Directory -Force -Path src-tauri/binaries | Out-Null
Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'debug/dzc.exe') -Destination "src-tauri/binaries/dzc-$triple.exe" -Force
$override = Join-Path $env:RUNNER_TEMP 'dezirclip-native-fixture.json'
$configuration = Get-Content -LiteralPath src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
$configuration.app.windows[0] | Add-Member -NotePropertyName additionalBrowserArgs -NotePropertyValue '--remote-debugging-port=9222' -Force
@{build=@{beforeBuildCommand='pnpm run build'};bundle=@{createUpdaterArtifacts=$false};app=@{windows=$configuration.app.windows}} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $override -Encoding utf8
& pnpm exec tauri build --debug --bundles nsis --config $override
if ($LASTEXITCODE -ne 0) { throw 'Fixture NSIS build failed.' }
$candidate = Join-Path $metadata.target_directory "debug/bundle/nsis/DezirClip_${Version}_x64-setup.exe"
if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw 'Fixture installer was not generated.' }
Copy-Item -LiteralPath $candidate -Destination $Destination -Force
