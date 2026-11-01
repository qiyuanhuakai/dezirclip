param(
    [ValidateSet('run', 'cleanup')]
    [string]$Mode = 'run',
    [string]$Root,
    [ValidateSet('release', 'debug')]
    [string]$Profile = 'release'
)

$ErrorActionPreference = 'Stop'
if ($Mode -eq 'cleanup') {
    if ([string]::IsNullOrWhiteSpace($Root)) { throw 'The exact owned diagnostics root is required' }
    $diagnosticArguments = @('-B', (Join-Path $PSScriptRoot 'windows-crash-diagnostics.py'), 'cleanup', '--root', $Root)
} else {
    $desktop = if ($Profile -eq 'debug') { 'target/debug/dezirclip.exe' } else { 'target/release/dezirclip.exe' }
    $diagnosticArguments = @('-B', (Join-Path $PSScriptRoot 'windows-crash-sequence.py')) + @(
        '--desktop', $desktop,
        '--helper', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe',
        '--helper-proof', 'artifacts/dzc-surface/x86_64-pc-windows-msvc/dzc-surface.exe.proof.json'
    )
    if ($Profile -eq 'debug') {
        $diagnosticArguments += @('--profile', 'debug', '--desktop-pdb', 'target/debug/dezirclip.pdb',
            '--build-receipt', 'artifacts/windows-debug-build/dezirclip.debug-build.json')
    }
}
& python @diagnosticArguments
exit $LASTEXITCODE
