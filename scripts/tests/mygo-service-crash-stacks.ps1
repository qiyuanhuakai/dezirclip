param([Parameter(Mandatory)][string]$Desktop)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    [Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
    throw 'Service crash diagnostics require a disposable GitHub-hosted Windows STA session'
}
. (Join-Path $PSScriptRoot 'service-process-receipt.ps1')
$binary = (Resolve-Path -LiteralPath $Desktop).Path
$binaryDirectory = Split-Path $binary
$service = Join-Path $binaryDirectory 'dzc-service.exe'
if (!(Test-Path -LiteralPath $service) -or !(Test-Path -LiteralPath (Join-Path $binaryDirectory 'dzc_service.pdb'))) {
    throw 'Build the Service with matching debug symbols before collecting stacks'
}
$debugger = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Debuggers/x64/cdb.exe'
if (!(Test-Path -LiteralPath $debugger)) {
    # Microsoft Windows SDK download page, SDK 10.0.26100.9457. Install only its debuggers.
    $installer = Join-Path $env:RUNNER_TEMP ('dzc-sdk-' + [guid]::NewGuid().ToString('N') + '.exe')
    Invoke-WebRequest 'https://go.microsoft.com/fwlink/?linkid=2382321' -OutFile $installer
    $signature = Get-AuthenticodeSignature -LiteralPath $installer
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw 'Windows SDK installer signature is not valid Microsoft code'
    }
    $setup = Start-Process -FilePath $installer -ArgumentList '/features OptionId.WindowsDesktopDebuggers /quiet /norestart' -WindowStyle Hidden -PassThru
    if (!$setup.WaitForExit(600000)) { $setup.Kill(); throw 'Windows debugger installation timed out' }
    if ($setup.ExitCode -notin @(0,3010) -or !(Test-Path -LiteralPath $debugger)) { throw 'Windows debugger installation failed' }
}

$root = Join-Path ([IO.Path]::GetFullPath($env:RUNNER_TEMP)) ('dzc-service-stacks-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root
$registry = 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\dzc-service.exe'
if (Test-Path -LiteralPath $registry) { throw 'Existing Service crash configuration is not modified' }
$configured = $false
$originalRoot = $env:DZC_SERVICE_CRASH_ROOT
try {
    $null = New-Item -Path $registry -Force
    $configured = $true
    $null = New-ItemProperty -LiteralPath $registry -Name DumpFolder -Value $root -PropertyType ExpandString
    $null = New-ItemProperty -LiteralPath $registry -Name DumpType -Value 1 -PropertyType DWord
    $null = New-ItemProperty -LiteralPath $registry -Name DumpCount -Value 1 -PropertyType DWord
    $env:DZC_SERVICE_CRASH_ROOT = $root
    $probe = Start-RecordedService $service '--build-info' $root
    try {
        if (!$probe.Process.WaitForExit(5000)) { $probe.Process.Kill(); $probe.Process.WaitForExit(); throw 'Service identity probe did not exit' }
        $receipt = Get-Content -LiteralPath $probe.Receipt -Raw | ConvertFrom-Json
        if ($probe.Process.ExitCode -ne 0 -or $receipt.pid -ne $probe.Process.Id -or $receipt.executable -cne $service -or !$receipt.created_at) {
            throw 'Fast-exit Service process lost its launch identity'
        }
        Write-Host 'Verified launch receipt survives a Service exit without waiting for readiness'
    } finally { $probe.Process.Dispose() }
    for ($cycle = 1; $cycle -le 32; $cycle++) {
        Write-Host "Service stack diagnostic cycle $cycle of 32"
        powershell -NoProfile -NonInteractive -STA -ExecutionPolicy Bypass -File ./scripts/tests/mygo-desktop-windows.ps1 -Desktop $binary -Emoji
        if ($LASTEXITCODE) { throw "Service diagnostic cycle $cycle failed: $LASTEXITCODE" }
    }
} finally {
    # Analyze only a PID recorded by this fixture before its process exited.
    # Raw dumps and debugger output stay on the ephemeral runner, never artifacts.
    try {
        foreach ($receiptFile in @(Get-ChildItem -LiteralPath $root -Filter 'service-*.json' -File)) {
            $receipt = Get-Content -LiteralPath $receiptFile.FullName -Raw | ConvertFrom-Json
            if ($receipt.executable -cne $service -or $receipt.pid -le 0) { throw 'Unexpected Service receipt' }
            $dump = Join-Path $root ("dzc-service.exe.{0}.dmp" -f $receipt.pid)
            if (!$receipt.crashed) { continue }
            $deadline = (Get-Date).AddSeconds(30)
            while (!(Test-Path -LiteralPath $dump) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
            if (!(Test-Path -LiteralPath $dump)) { Write-Host "No WER dump for recorded Service PID $($receipt.pid)"; continue }
            $log = Join-Path $root ("stack-{0}.txt" -f $receipt.pid)
            $errors = Join-Path $root ("debugger-{0}.txt" -f $receipt.pid)
            $symbols = "$binaryDirectory;srv*$root\symbols*https://msdl.microsoft.com/download/symbols"
            $arguments = @('-z', ('"' + $dump + '"'), '-y', ('"' + $symbols + '"'), '-c', '".reload; .ecxr; kn 40; q"')
            $process = Start-Process -FilePath $debugger -ArgumentList $arguments -RedirectStandardOutput $log -RedirectStandardError $errors -WindowStyle Hidden -PassThru
            if (!$process.WaitForExit(180000)) { $process.Kill(); Write-Host 'Service stack symbolization timed out'; continue }
            Write-Host "Service crash stack: pid=$($receipt.pid), exit=$($receipt.exit), sha256=$($receipt.sha256)"
            $frames = @(Get-Content -LiteralPath $log | Where-Object { $_ -match '^\s*[0-9a-fA-F]{2}\s+[0-9a-fA-F`]+\s+[0-9a-fA-F`]+\s+\S' })
            if (!$frames.Count) { Write-Host "No stack frames resolved; debugger exit=$($process.ExitCode)" }
            $frames | ForEach-Object { Write-Host $_ }
        }
    } catch { Write-Host ("Stack diagnostics failed: " + $_.Exception.Message) }
    finally {
        $env:DZC_SERVICE_CRASH_ROOT = $originalRoot
        if ($configured) { Remove-Item -LiteralPath $registry -Recurse }
        $resolvedRoot = [IO.Path]::GetFullPath($root)
        $temporary = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
        if (!$resolvedRoot.StartsWith($temporary, [StringComparison]::OrdinalIgnoreCase) -or
            (Split-Path $resolvedRoot -Leaf) -notmatch '^dzc-service-stacks-[a-f0-9]{32}$') { throw 'Invalid diagnostic cleanup root' }
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
