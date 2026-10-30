param(
    [ValidateSet('run', 'cleanup')]
    [string]$Mode = 'run',
    [string]$Root
)

$ErrorActionPreference = 'Stop'
if ($Mode -eq 'cleanup') {
    if ([string]::IsNullOrWhiteSpace($Root)) { throw 'The exact owned diagnostics root is required' }
    $diagnosticArguments = @('-B', (Join-Path $PSScriptRoot 'windows-crash-diagnostics.py'), 'cleanup', '--root', $Root)
} else {
    $diagnosticArguments = @('-B', (Join-Path $PSScriptRoot 'windows-crash-sequence.py')) + @(
        '--desktop', 'target/release/dezirclip.exe',
        '--helper', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe',
        '--helper-proof', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe.proof.json'
    )
}
& python @diagnosticArguments
exit $LASTEXITCODE
