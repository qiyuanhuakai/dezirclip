# Load with: dzc pwsh | Out-String | Invoke-Expression. Requires PowerShell 7 and PSReadLine.
if (-not $env:DZC_BIN) { $env:DZC_BIN = __DZC_EXECUTABLE__ }
if (-not $env:DZC_PICKER) { $env:DZC_PICKER = 'tui' }

function Invoke-DzcInsert {
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $env:DZC_BIN
    $processInfo.UseShellExecute = $false
    $processInfo.RedirectStandardOutput = $true
    $processInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $arguments = if ($env:DZC_PICKER -eq 'tui') {
        @('tui', '--action', 'insert')
    } else {
        @('pick', '--backend', $env:DZC_PICKER, '--action', 'insert')
    }
    foreach ($argument in $arguments) {
        $processInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $processInfo
    try {
        if ($process.Start()) {
            $selection = $process.StandardOutput.ReadToEnd()
            $process.WaitForExit()
            if ($process.ExitCode -eq 0) {
                $dzcLine = ''; $dzcCursor = 0
                [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$dzcLine, [ref]$dzcCursor)
                [Microsoft.PowerShell.PSConsoleReadLine]::Replace($dzcCursor, 0, $selection)
            }
        }
    }
    catch {
        [Console]::Error.WriteLine('Could not run dzc; check DZC_BIN and its dependencies.')
    }
    finally {
        $process.Dispose()
    }
}

if (Get-Module PSReadLine) {
    Set-PSReadLineKeyHandler -Chord Ctrl+g -ScriptBlock { Invoke-DzcInsert }
}
