param(
    [string]$TargetDirectory = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { Join-Path $PSScriptRoot '../target/release' }),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../artifacts/portable')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourcePath = [System.IO.Path]::GetFullPath($TargetDirectory)
$outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$productPaths = @('dezirclip.exe', 'dzc.exe', 'dzc-service.exe') | ForEach-Object {
    $productPath = Join-Path $sourcePath $_
    if (-not (Test-Path -LiteralPath $productPath -PathType Leaf)) {
        throw "Missing product executable: $productPath"
    }
    if ((Get-Item -LiteralPath $productPath).Length -eq 0) {
        throw "Empty product executable: $productPath"
    }
    $productPath
}

[System.IO.Directory]::CreateDirectory($outputPath) | Out-Null
$archivePath = Join-Path $outputPath 'dezirclip-portable.zip'
$temporaryArchive = Join-Path $outputPath ('.dezirclip-portable-' + [Guid]::NewGuid().ToString('N') + '.zip')
$stagingPath = [System.IO.Path]::GetFullPath((Join-Path $outputPath ('.dezirclip-portable-' + [Guid]::NewGuid().ToString('N'))))
$outputPrefix = $outputPath.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $stagingPath.StartsWith($outputPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Portable staging directory must remain inside the output directory"
}
$configDirectory = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../src-tauri'))
$config = Get-Content -LiteralPath (Join-Path $configDirectory 'tauri.conf.json') -Raw | ConvertFrom-Json

try {
    [System.IO.Directory]::CreateDirectory($stagingPath) | Out-Null
    foreach ($productPath in $productPaths) {
        Copy-Item -LiteralPath $productPath -Destination $stagingPath
    }
    foreach ($resource in $config.bundle.resources.PSObject.Properties) {
        $source = [System.IO.Path]::GetFullPath((Join-Path $configDirectory $resource.Name))
        $destination = [System.IO.Path]::GetFullPath((Join-Path $stagingPath $resource.Value))
        if (-not $destination.StartsWith(($stagingPath + [System.IO.Path]::DirectorySeparatorChar), [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Portable resource target escapes the staging directory: $($resource.Value)"
        }
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or
            ((Get-Item -LiteralPath $source).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "Missing or linked portable resource: $source"
        }
        [System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($destination)) | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination
    }
    foreach ($required in @('skills/dzc-cli/SKILL.md', 'skills/dezirclip-theme/SKILL.md', 'skills/dezirclip-theme/assets/forest.theme.json')) {
        if (-not (Test-Path -LiteralPath (Join-Path $stagingPath $required) -PathType Leaf)) {
            throw "Missing portable Skill resource: $required"
        }
    }
    $packagePaths = Get-ChildItem -LiteralPath $stagingPath | ForEach-Object { $_.FullName }
    Compress-Archive -LiteralPath $packagePaths -DestinationPath $temporaryArchive -CompressionLevel Optimal
    Move-Item -LiteralPath $temporaryArchive -Destination $archivePath -Force
    Write-Output $archivePath
}
finally {
    if (Test-Path -LiteralPath $temporaryArchive -PathType Leaf) {
        Remove-Item -LiteralPath $temporaryArchive -Force
    }
    if (Test-Path -LiteralPath $stagingPath -PathType Container) {
        Remove-Item -LiteralPath $stagingPath -Recurse -Force
    }
}
