$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'service-process-receipt.ps1')
$temporary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$root = Join-Path $temporary ('dzc-receipt-test-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root
try {
    $path = Join-Path $root 'service-42.json'
    $executable = 'C:\synthetic\dzc-service.exe'
    foreach ($case in @(
        @{ Exited=$false; Code=-1073740940; Selected=$false },
        @{ Exited=$true; Code=0; Selected=$false },
        @{ Exited=$true; Code=2; Selected=$false },
        @{ Exited=$true; Code=-1073740940; Selected=$true }
    )) {
        @{ pid=42; executable=$executable; crashed=$false; exit=$null } | ConvertTo-Json | Set-Content -LiteralPath $path -Encoding UTF8
        $child = [pscustomobject]@{ Id=42; HasExited=$case.Exited; ExitCode=$case.Code }
        $child | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
        Update-RecordedServiceExit $child $path
        $selected = Select-ServiceCrashReceipt $path $executable
        if ([bool]$selected -ne $case.Selected) { throw 'Natural process exit classification failed' }
        if ($selected -and ($selected.pid -ne 42 -or $selected.exit -ne -1073740940)) { throw 'Analyzer selected the wrong process' }
        $record = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
        if (!$case.Exited -and ($record.crashed -or $null -ne $record.exit)) { throw 'Live process was classified before active cleanup' }
    }
    Update-RecordedServiceExit $null $null
    if ($env:DZC_TEST_RECEIPT_PROCESS -eq '1') {
        $shell = (Get-Command powershell.exe).Source
        $launch = Start-RecordedService $shell '-NoProfile -NonInteractive -Command "[Environment]::Exit(-1073740940)"' $root
        try {
            if (!$launch.Process.WaitForExit(10000)) { throw 'Synthetic negative-exit child did not finish' }
            Update-RecordedServiceExit $launch.Process $launch.Receipt
            $selected = Select-ServiceCrashReceipt $launch.Receipt $shell
            if (!$selected -or $selected.pid -ne $launch.Process.Id -or $selected.exit -ne -1073740940) {
                throw 'Negative exit before readiness was not selected for analysis'
            }
            Write-Host 'Verified actual negative exit before readiness retains a selectable process receipt'
        } finally {
            if (!$launch.Process.HasExited) { $launch.Process.Kill(); $launch.Process.WaitForExit() }
            $launch.Process.Dispose()
        }
    }
    Write-Host 'Verified exited, live, successful and negative-exit receipt selection'
} finally {
    $resolved = [IO.Path]::GetFullPath($root)
    if (!$resolved.StartsWith($temporary, [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path $resolved -Leaf) -notmatch '^dzc-receipt-test-[a-f0-9]{32}$') { throw 'Invalid receipt test cleanup root' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
