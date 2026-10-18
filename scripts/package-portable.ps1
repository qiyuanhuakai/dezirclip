param(
    [string]$TargetDirectory = (Join-Path $PSScriptRoot '../src-tauri/target/release'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../artifacts/portable')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sourcePath = [System.IO.Path]::GetFullPath($TargetDirectory)
$outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$productPaths = @('dezirclip.exe', 'dzc.exe') | ForEach-Object {
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

try {
    Compress-Archive -LiteralPath $productPaths -DestinationPath $temporaryArchive -CompressionLevel Optimal
    Move-Item -LiteralPath $temporaryArchive -Destination $archivePath -Force
    Write-Output $archivePath
}
finally {
    if (Test-Path -LiteralPath $temporaryArchive -PathType Leaf) {
        Remove-Item -LiteralPath $temporaryArchive -Force
    }
}
