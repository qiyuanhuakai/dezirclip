function Start-RecordedService([string]$Executable, [string]$Arguments, [string]$Root) {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
        throw 'Recorded Service launches require a disposable GitHub runner'
    }
    $path = (Resolve-Path -LiteralPath $Executable).Path
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $path
    $process.StartInfo.Arguments = $Arguments
    $process.StartInfo.WorkingDirectory = Split-Path $path
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.CreateNoWindow = $true
    if (!$process.Start()) { throw 'Could not create the diagnostic Service process' }
    try {
        # Process.Start retains the CreateProcess handle, including if the child
        # exits immediately. No readiness request or later PID lookup precedes this receipt.
        $null = $process.Handle
        $receipt = Join-Path $Root ("service-{0}.json" -f $process.Id)
        [pscustomobject]@{
            pid = $process.Id
            executable = $path
            sha256 = $hash
            created_at = $process.StartTime.ToUniversalTime().ToString('o')
            crashed = $false
            exit = $null
        } | ConvertTo-Json | Set-Content -LiteralPath $receipt -Encoding UTF8
        return [pscustomobject]@{ Process = $process; Receipt = $receipt }
    } catch {
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
        throw
    }
}
