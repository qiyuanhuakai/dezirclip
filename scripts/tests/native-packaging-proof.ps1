param([string]$EvidenceZip, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ('dzc-packaging-proof-' + [guid]::NewGuid().ToString('N')) }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Proof tests require a new output directory.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
[System.IO.File]::WriteAllText((Join-Path $OutputDirectory '.dzc-packaging-proof-test'), 'static-bytes-only')
$evidence = [ordered]@{ schemaVersion = 1; kind = 'static-packaging-proof'; status = 'running'; tests = @(); outputDirectory = $OutputDirectory }
function Save-Evidence { $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'evidence.json') -Encoding utf8 }
function Check([string]$Name, [scriptblock]$Body) {
    try { & $Body; $evidence.tests += @{ name = $Name; status = 'passed' } }
    catch { $evidence.tests += @{ name = $Name; status = 'failed'; error = $_.Exception.Message }; throw }
    finally { Save-Evidence }
}
function Assert-Equal($Actual, $Expected) { if ($Actual -cne $Expected) { throw "Expected '$Expected', received '$Actual'." } }
function Assert-Rejected([scriptblock]$Body) {
    $rejected = $false
    try { & $Body | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw 'Malformed proof was accepted.' }
}

# Import only these pure function declarations. No installer, registry,
# clipboard, GUI or main script statements are evaluated by this test.
foreach ($spec in @(
    @{ script = 'scripts/build-native-windows-fixture.ps1'; functions = @('Get-NsisApplicationProof', 'Get-NsisCompilerInputs') },
    @{ script = 'scripts/test-native-windows-upgrade.ps1'; functions = @('Get-InstalledApplicationProof', 'Assert-NsisCompilerInputManifest') }
)) {
    $tokens = $null; $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $repository $spec.script), [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
    foreach ($name in $spec.functions) {
        $functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true))
        if ($functions.Count -ne 1) { throw "Expected exactly one pure function: $name" }
        . ([scriptblock]::Create($functions[0].Extent.Text))
    }
}

Save-Evidence
try {
    $raw = [byte[]]::new(768)
    $raw[0] = 0x4d; $raw[1] = 0x5a
    [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK'), 0, $raw, 128, 27)
    [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS'), 0, $raw, 256, 27)
    $rawPath = Join-Path $OutputDirectory 'synthetic-raw.bin'
    [System.IO.File]::WriteAllBytes($rawPath, $raw)
    $proof = Get-NsisApplicationProof $rawPath
    $packaged = [byte[]]$raw.Clone()
    [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS'), 0, $packaged, 128, 27)
    $packagedPath = Join-Path $OutputDirectory 'synthetic-packaged.bin'
    [System.IO.File]::WriteAllBytes($packagedPath, $packaged)
    $manifest = [pscustomobject]@{ applicationSha256 = $proof.packagedSha256; applicationRawSha256 = $proof.rawSha256; applicationPatchOffset = $proof.patchOffset }
    Check 'one documented token patch predicts the entire packaged executable' {
        Assert-Equal $proof.patchOffset 128
        Assert-Equal $proof.rawSha256 (Get-FileHash -LiteralPath $rawPath -Algorithm SHA256).Hash
        Assert-Equal $proof.packagedSha256 (Get-FileHash -LiteralPath $packagedPath -Algorithm SHA256).Hash
        Assert-Equal (Get-InstalledApplicationProof $packagedPath $manifest).rawSha256 $proof.rawSha256
        Assert-Equal (Get-FileHash -LiteralPath $rawPath -Algorithm SHA256).Hash $proof.rawSha256
        Assert-Equal (Get-FileHash -LiteralPath $packagedPath -Algorithm SHA256).Hash $proof.packagedSha256
    }
    Check 'unrelated byte changes are rejected by the whole packaged hash' {
        $corrupt = [byte[]]$packaged.Clone(); $corrupt[600] = 1
        $path = Join-Path $OutputDirectory 'synthetic-corrupt.bin'
        [System.IO.File]::WriteAllBytes($path, $corrupt)
        Assert-Rejected { Get-InstalledApplicationProof $path $manifest }
    }
    Check 'wrong offset and wrong original build digest are rejected' {
        Assert-Rejected { Get-InstalledApplicationProof $packagedPath ([pscustomobject]@{ applicationSha256 = $proof.packagedSha256; applicationRawSha256 = $proof.rawSha256; applicationPatchOffset = 256 }) }
        Assert-Rejected { Get-InstalledApplicationProof $packagedPath ([pscustomobject]@{ applicationSha256 = $proof.packagedSha256; applicationRawSha256 = ('0' * 64); applicationPatchOffset = 128 }) }
    }
    Check 'missing or duplicate original token is rejected' {
        Assert-Rejected { Get-NsisApplicationProof $packagedPath }
        $duplicate = [byte[]]$raw.Clone()
        [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK'), 0, $duplicate, 400, 27)
        $path = Join-Path $OutputDirectory 'synthetic-duplicate.bin'
        [System.IO.File]::WriteAllBytes($path, $duplicate)
        Assert-Rejected { Get-NsisApplicationProof $path }
    }
    $staged = Join-Path $OutputDirectory 'external-sidecar.bin'
    $cargo = Join-Path $OutputDirectory 'cargo-sidecar.bin'
    [System.IO.File]::WriteAllText($staged, 'standalone fixture')
    [System.IO.File]::WriteAllText($cargo, 'standalone fixture')
    $sidecarDigest = (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash
    $script = Join-Path $OutputDirectory 'installer.nsi'
    $validScript = "!define MAINBINARYSRCPATH `"$rawPath`"`nFile /a `"/oname=dzc.exe`" `"$cargo`"`n"
    [System.IO.File]::WriteAllText($script, $validScript)
    Check 'the sole NSIS destination input must be the exact Cargo CLI and standalone bytes' {
        $inputs = @(Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest)
        Assert-Equal $inputs.Count 1
        Assert-Equal $inputs[0].source ([System.IO.Path]::GetFullPath($cargo))
        Assert-Equal $inputs[0].sha256 $sidecarDigest
        Assert-Equal $inputs[0].role 'cargo-binary'
        Assert-Equal $inputs[0].destination 'dzc.exe'
    }
    Check 'overwriting Cargo sidecar and unexpected main executable are rejected' {
        [System.IO.File]::WriteAllText($cargo, 'desktop-feature wrong bytes')
        Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest }
        [System.IO.File]::WriteAllText($cargo, 'standalone fixture')
        Assert-Rejected { Get-NsisCompilerInputs $script $packagedPath $cargo $sidecarDigest }
    }
    Check 'duplicate Cargo or legacy staged-plus-Cargo compiler entries are rejected' {
        foreach ($extra in @($cargo, $staged)) {
            [System.IO.File]::WriteAllText($script, $validScript + "File /a `"/oname=dzc.exe`" `"$extra`"`n")
            Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest }
        }
        [System.IO.File]::WriteAllText($script, $validScript)
    }
    Check 'staged or unknown source is rejected even with identical standalone bytes' {
        $unknown = Join-Path $OutputDirectory 'unknown-same-bytes.bin'
        [System.IO.File]::WriteAllText($unknown, 'standalone fixture')
        foreach ($other in @($staged, $unknown)) {
            [System.IO.File]::WriteAllText($script, $validScript.Replace($cargo, $other))
            Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest }
        }
        [System.IO.File]::WriteAllText($script, $validScript)
    }
    Check 'missing CLI or duplicate GUI declaration and invalid complete hash are rejected' {
        [System.IO.File]::WriteAllText($script, "!define MAINBINARYSRCPATH `"$rawPath`"`n")
        Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest }
        [System.IO.File]::WriteAllText($script, $validScript + "!define MAINBINARYSRCPATH `"$rawPath`"`n")
        Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest }
        [System.IO.File]::WriteAllText($script, $validScript)
        Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo 'abcd' }
        Assert-Rejected { Get-NsisCompilerInputs $script $rawPath $cargo ('0' * 64) }
    }
    $compilerManifest = [pscustomobject]@{
        schemaVersion = 3; nsisInputLayout = 'single-cargo-binary-v1'; sidecarSha256 = $sidecarDigest
        nsisInputs = @(Get-NsisCompilerInputs $script $rawPath $cargo $sidecarDigest)
        sidecarPreparation = [pscustomobject]@{ binaryPaths = @(
            [pscustomobject]@{ role = 'cargo-target'; path = (Join-Path $OutputDirectory 'explicit-target.bin'); sha256 = $sidecarDigest },
            [pscustomobject]@{ role = 'external-bin'; path = $staged; sha256 = $sidecarDigest },
            [pscustomobject]@{ role = 'cargo-host-default'; path = $cargo; sha256 = $sidecarDigest }
        ) }
    }
    Check 'schema3 consumer accepts only the unique proven Cargo compiler input' {
        Assert-NsisCompilerInputManifest $compilerManifest
    }
    Check 'consumer rejects old schema and duplicate or mismatched compiler records' {
        $original = $compilerManifest | ConvertTo-Json -Depth 8
        foreach ($change in @('old-schema', 'two-inputs', 'staged-source', 'wrong-sha', 'wrong-role', 'duplicate-cargo-proof')) {
            $changed = $original | ConvertFrom-Json
            switch ($change) {
                'old-schema' { $changed.schemaVersion = 2 }
                'two-inputs' { $changed.nsisInputs = @($changed.nsisInputs[0], $changed.nsisInputs[0]) }
                'staged-source' { $changed.nsisInputs[0].source = $staged }
                'wrong-sha' { $changed.nsisInputs[0].sha256 = '0' * 64 }
                'wrong-role' { $changed.nsisInputs[0].role = 'external-bin' }
                'duplicate-cargo-proof' { $changed.sidecarPreparation.binaryPaths += $changed.sidecarPreparation.binaryPaths[-1] }
            }
            Assert-Rejected { Assert-NsisCompilerInputManifest $changed }
        }
    }

    if ($EvidenceZip) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zipPath = [System.IO.Path]::GetFullPath($EvidenceZip)
        $zip = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
        try {
            $entry = @($zip.Entries | Where-Object { $_.FullName.EndsWith('/installed/dezirclip.exe', [System.StringComparison]::Ordinal) })
            $build = @($zip.Entries | Where-Object { $_.FullName.EndsWith('/candidate-build.json', [System.StringComparison]::Ordinal) })
            if ($entry.Count -ne 1 -or $build.Count -ne 1) { throw 'Expected one original installed GUI and build manifest.' }
            $memory = [System.IO.MemoryStream]::new()
            $stream = $entry[0].Open()
            try { $stream.CopyTo($memory); $installed = $memory.ToArray() } finally { $stream.Dispose(); $memory.Dispose() }
            $reader = [System.IO.StreamReader]::new($build[0].Open())
            try { $oldManifest = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
        } finally { $zip.Dispose() }
        $actualPath = Join-Path $OutputDirectory 'ci-installed-gui.bin'
        [System.IO.File]::WriteAllBytes($actualPath, $installed)
        # This offset and two full-file digests come from the retained b2fa6b8
        # failed CI artifact. Only bytes are read; neither binary is executed.
        $offset = 0x1e1c2a8
        Assert-Equal ([System.Text.Encoding]::ASCII.GetString($installed, $offset, 27)) '__TAURI_BUNDLE_TYPE_VAR_NSS'
        [Array]::Copy([System.Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK'), 0, $installed, $offset, 27)
        $originalPath = Join-Path $OutputDirectory 'ci-reconstructed-raw-gui.bin'
        [System.IO.File]::WriteAllBytes($originalPath, $installed)
        Check 'retained failed CI bytes are explained exactly by the documented Tauri transformation' {
            $actualProof = Get-NsisApplicationProof $originalPath
            Assert-Equal $actualProof.patchOffset $offset
            Assert-Equal $actualProof.rawSha256 $oldManifest.applicationSha256
            Assert-Equal $actualProof.packagedSha256 'A349578497C3678C6AC82D0D035053405403611C627F3DDDFDEC0CCA6774F958'
            $newManifest = [pscustomobject]@{ applicationSha256 = $actualProof.packagedSha256; applicationRawSha256 = $actualProof.rawSha256; applicationPatchOffset = $offset }
            Assert-Equal (Get-InstalledApplicationProof $actualPath $newManifest).rawSha256 $oldManifest.applicationSha256
            $evidence.originalCi = @{ zipSha256 = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash; rawSha256 = $actualProof.rawSha256; packagedSha256 = $actualProof.packagedSha256; patchOffset = $offset }
        }
    }
    $evidence.status = 'passed'
} catch { $evidence.status = 'failed'; $evidence.error = $_.Exception.Message; throw }
finally { Save-Evidence; Write-Output "Static evidence: $OutputDirectory" }
