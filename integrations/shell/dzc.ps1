# Dot-source in PowerShell 7 with PSReadLine. Ctrl-G inserts selected clipboard text.
if (-not $env:DZC_ADAPTER) {
    $env:DZC_ADAPTER = Join-Path (Split-Path -Parent $PSScriptRoot) 'dzc-integrations.py'
}
if (-not $env:DZC_PYTHON) { $env:DZC_PYTHON = 'python' }
if (-not $env:DZC_PICKER) { $env:DZC_PICKER = 'fzf' }

function Invoke-DzcInsert {
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $env:DZC_PYTHON
    $processInfo.UseShellExecute = $false
    $processInfo.RedirectStandardOutput = $true
    $processInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
    foreach ($argument in @($env:DZC_ADAPTER, 'pick', '--backend', $env:DZC_PICKER, '--action', 'insert')) {
        $processInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $processInfo
    try {
        if ($process.Start()) {
            $selection = $process.StandardOutput.ReadToEnd()
            $process.WaitForExit()
            if ($process.ExitCode -eq 0) {
                [Microsoft.PowerShell.PSConsoleReadLine]::Insert($selection)
            }
        }
    }
    finally {
        $process.Dispose()
    }
}

if (Get-Module PSReadLine) {
    Set-PSReadLineKeyHandler -Chord Ctrl+g -ScriptBlock { Invoke-DzcInsert }
}
