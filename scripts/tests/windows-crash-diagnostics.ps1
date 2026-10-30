param(
    [ValidateSet('run', 'cleanup')]
    [string]$Mode = 'run',
    [string]$Root
)

$ErrorActionPreference = 'Stop'
$diagnosticArguments = @('-B', (Join-Path $PSScriptRoot 'windows-crash-diagnostics.py'), $Mode)
if ($Mode -eq 'cleanup') {
    if ([string]::IsNullOrWhiteSpace($Root)) { throw 'The exact owned diagnostics root is required' }
    $diagnosticArguments += @('--root', $Root)
} else {
    $diagnosticArguments += @(
        '--desktop', 'target/release/dezirclip.exe',
        '--helper', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe',
        '--helper-proof', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe.proof.json'
    )
}
& python @diagnosticArguments
exit $LASTEXITCODE
