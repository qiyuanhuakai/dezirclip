param([Parameter(Mandatory)][string]$Desktop)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    [Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
    throw 'The clipboard and foreground-window test requires a disposable GitHub-hosted Windows STA session'
}
Add-Type -AssemblyName System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MyGoInput {
    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int x, y; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
}
'@
$null = [MyGoInput]::SetProcessDPIAware()

function Wait-Until([scriptblock]$Check, [string]$Failure, [int]$Seconds = 30) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
        [Windows.Forms.Application]::DoEvents()
        $result = & $Check
        if ($result) { return $result }
        Start-Sleep -Milliseconds 80
    }
    throw $Failure
}

$exe = (Resolve-Path -LiteralPath $Desktop).Path
$cli = Join-Path (Split-Path $exe) 'dzc.exe'
$dir = New-Item -ItemType Directory -Path (Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString()))
$env:DEZIRCLIP_DB_PATH = Join-Path $dir.FullName 'clipboard.db'
$marker = 'MyGo desktop fixture ' + [guid]::NewGuid().ToString('N')
$form = New-Object Windows.Forms.Form
$form.Text = 'Independent MyGo paste target'
$edit = New-Object Windows.Forms.TextBox
$form.Controls.Add($edit)
$resident = $null
try {
    & $cli add $marker | Out-Null
    if ($LASTEXITCODE) { throw 'Could not seed isolated clipboard history' }
    [Windows.Forms.Clipboard]::SetText('Before ' + $marker)
    $form.Show()
    $form.Activate()
    $null = $edit.Focus()
    $null = Wait-Until { $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Editor did not acquire foreground'
    $resident = Start-Process -FilePath $exe -RedirectStandardError (Join-Path $dir.FullName 'desktop.log') -PassThru
    $main = Wait-Until {
        if ($resident.HasExited) { throw 'Resident exited during startup' }
        Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
            $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
        } | Select-Object -First 1
    } 'Packaged MyGo main window did not appear'
    $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
    $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $marker)
    $point = Wait-Until {
        $row = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
        if ($row -and !$row.Current.IsOffscreen -and !$row.Current.BoundingRectangle.IsEmpty) {
            $bounds = $row.Current.BoundingRectangle
            if ($bounds.Width -gt 0 -and $bounds.Height -gt 0) {
                return [System.Drawing.Point]::new([int]($bounds.Left + $bounds.Width / 2), [int]($bounds.Top + $bounds.Height / 2))
            }
        }
    } 'Synthetic history row missing from the shared WebView2 accessibility tree'
    if ([MyGoInput]::GetForegroundWindow() -ne $main.MainWindowHandle) { throw 'MyGo main window lost focus before row click' }
    if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not position pointer on the history row' }
    $hit = New-Object MyGoInput+Point
    $hit.x, $hit.y = $point.X, $point.Y
    if ([MyGoInput]::GetAncestor([MyGoInput]::WindowFromPoint($hit), 2) -ne $main.MainWindowHandle) {
        throw 'Physical row coordinates do not belong to the original MyGo WebView window'
    }
    [MyGoInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [MyGoInput]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    $null = Wait-Until {
        $edit.Text -ceq $marker -and $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle
    } 'Click did not paste the exact history text into the original editor'
    if ([Windows.Forms.Clipboard]::GetText() -cne $marker) { throw 'Windows clipboard differs from selected history text' }
    $null = Wait-Until { $main.HasExited } 'Unpinned WebView2 main window did not exit'
    if ($resident.HasExited) { throw 'Native resident exited with the main window' }
    Write-Output 'Verified shared WebView2 row click, exact clipboard and editor text, restored focus, and independent native resident'
} catch {
    $failure = $_
    Write-Host ("Editor length={0}, exact={1}, focused={2}, foreground={3}, editor window={4}" -f
        $edit.Text.Length, ($edit.Text -ceq $marker), $edit.Focused, [MyGoInput]::GetForegroundWindow(), $form.Handle)
    try {
      if ($main -and !$main.HasExited -and $root) {
        $names = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
            ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Last 40
        Write-Host ("Synthetic main-window accessibility tree: " + ($names -join ' | '))
      }
      Write-Host ("Clipboard matches selected entry: " + ([Windows.Forms.Clipboard]::GetText() -ceq $marker))
    } catch { Write-Host ("Diagnostic query failed: " + $_.Exception.Message) }
    $log = Join-Path $dir.FullName 'desktop.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 12 | Write-Host }
    throw $failure
} finally {
    if ($resident -and !$resident.HasExited) { $resident.Kill(); $resident.WaitForExit() }
    & $cli service stop | Out-Null
    $form.Dispose()
}
