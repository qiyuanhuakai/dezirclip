param([Parameter(Mandatory)][string]$Desktop, [switch]$Emoji, [switch]$Capture)

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
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, System.Text.StringBuilder title, int capacity);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr window, uint attribute, out uint value, uint size);
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

function Get-ControlPoint($root, [string]$Name) {
    $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Name, [Windows.Automation.PropertyConditionFlags]::IgnoreCase)
    return Wait-Until {
        $row = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
        if ($row -and $row.Current.IsOffscreen) {
            $scroll = $null
            if ($row.TryGetCurrentPattern([Windows.Automation.ScrollItemPattern]::Pattern, [ref]$scroll)) { $scroll.ScrollIntoView() }
        }
        if ($row -and !$row.Current.IsOffscreen -and !$row.Current.BoundingRectangle.IsEmpty) {
            $bounds = $row.Current.BoundingRectangle
            if ($bounds.Width -gt 0 -and $bounds.Height -gt 0) {
                return [System.Drawing.Point]::new([int]($bounds.Left + $bounds.Width / 2), [int]($bounds.Top + $bounds.Height / 2))
            }
        }
    } "Control missing from the shared WebView2 accessibility tree: $Name"
}

function Click-Control($main, $point) {
    if ([MyGoInput]::GetForegroundWindow() -ne $main.MainWindowHandle) { throw 'MyGo main window lost focus before click' }
    if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not position pointer on the control' }
    $hit = New-Object MyGoInput+Point
    $hit.x, $hit.y = $point.X, $point.Y
    if ([MyGoInput]::GetAncestor([MyGoInput]::WindowFromPoint($hit), 2) -ne $main.MainWindowHandle) {
        throw 'Control coordinates do not belong to the original MyGo WebView window'
    }
    [MyGoInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [MyGoInput]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
}

function Get-ResidentPanel($Resident, [string]$Title) {
    return Wait-Until {
        $window = [MyGoInput]::GetForegroundWindow()
        [uint32]$ownerId = 0
        $null = [MyGoInput]::GetWindowThreadProcessId($window, [ref]$ownerId)
        if ($ownerId -eq $Resident.Id) {
            $caption = [Text.StringBuilder]::new(128)
            $null = [MyGoInput]::GetWindowText($window, $caption, $caption.Capacity)
            if ($caption.ToString() -ceq $Title) { return $window }
        }
    } "Resident did not focus $Title" 12
}

function Test-ResidentPanel($Resident, [string]$Shortcut, [string]$Title, $Editor, $Form) {
    $Form.Activate()
    $null = $Editor.Focus()
    $null = Wait-Until { $Editor.Focused -and [MyGoInput]::GetForegroundWindow() -eq $Form.Handle } 'Independent editor did not regain foreground'
    [Windows.Forms.SendKeys]::SendWait($Shortcut)
    $panel = Get-ResidentPanel $Resident $Title
    [Windows.Forms.SendKeys]::SendWait('{ESC}')
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($panel) } "Resident did not dismiss $Title" 8
}

function Get-DesktopProcesses([int[]]$RootIds) {
    $processes = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,WorkingSetSize,PrivatePageCount)
    $owned = [Collections.Generic.HashSet[int]]::new()
    foreach ($rootId in $RootIds) { $null = $owned.Add($rootId) }
    do {
        $count = $owned.Count
        foreach ($p in $processes) {
            if ($owned.Contains([int]$p.ParentProcessId)) { $null = $owned.Add([int]$p.ProcessId) }
        }
    } while ($owned.Count -gt $count)
    $processes | Where-Object { $owned.Contains([int]$_.ProcessId) }
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
    @'
import os, sqlite3
with sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]) as db:
    db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                   [("app.emoji_panel_enabled", "true"), ("app.language","en"), ("app.theme", "acrylic"), ("app.color_mode", "dark"), ("app.show_app_border", "false")])
'@ | python -
    if ($LASTEXITCODE) { throw 'Could not seed isolated window appearance' }
    if ($Emoji) {
        & $cli service start --json | Out-Null
        if ($LASTEXITCODE) { throw 'Could not start the independent Service before MyGo' }
    }
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
    $webProcesses = @(Wait-Until {
        Get-DesktopProcesses @($main.Id) | Where-Object { $_.Name -eq 'msedgewebview2.exe' }
    } 'No WebView2 descendants found for the shared main window')
    $build = [int](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
    if ($build -ge 22621) {
        $observed = @{}
        try {
            $null = Wait-Until {
                $matched = $true
                foreach ($expected in @(@(38,3), @(20,1), @(34,([uint32]::MaxValue-1)))) {
                    [uint32]$value = 0
                    $hr = [MyGoInput]::DwmGetWindowAttribute($main.MainWindowHandle, $expected[0], [ref]$value, 4)
                    $observed[$expected[0]] = '0x{0:X8}/{1}' -f $hr,$value
                    # Some DWM versions accept BORDER_COLOR on set and return E_INVALIDARG on get.
                    if ($expected[0] -eq 34 -and $hr -eq -2147024809) { continue }
                    if ($hr -ne 0 -or $value -ne $expected[1]) { $matched = $false }
                }
                return $matched
            } 'MyGo window did not apply the expected DWM appearance'
        } finally {
            Write-Output ("DWM HRESULT/value, backdrop={0}, dark={1}, border={2}" -f $observed[38],$observed[20],$observed[34])
        }
    }
    if ($Capture) {
        Click-Control $main (Get-ControlPoint $root 'Settings')
        Click-Control $main (Get-ControlPoint $root 'Tools')
        $point = Get-ControlPoint $root 'Full Screen Capture'
        $bounds = [Windows.Forms.Screen]::FromPoint($point).Bounds
        Click-Control $main $point
        $null = Wait-Until {
            try { $image = [Windows.Forms.Clipboard]::GetImage() }
            catch [Runtime.InteropServices.ExternalException] { return $false }
            if ($image) {
                try { return $image.Width -eq $bounds.Width -and $image.Height -eq $bounds.Height }
                finally { $image.Dispose() }
            }
        } 'Full Screen Capture did not copy the selected display'
        Click-Control $main (Get-ControlPoint $root 'Region Capture')
        $panel = Get-ResidentPanel $resident ([regex]::Unescape('DezirClip \u622a\u56fe'))
        [Windows.Forms.SendKeys]::SendWait('{ESC}')
        $null = Wait-Until { ![MyGoInput]::IsWindowVisible($panel) } 'Region Capture did not dismiss'
        Write-Output 'Verified WebView2 buttons copy the full display and present the native resident region selector'
        return
    }
    if ($Emoji) {
        Click-Control $main (Get-ControlPoint $root 'Stickers')
        $marker = [char]::ConvertFromUtf32(0x1f600)
    }
    Click-Control $main (Get-ControlPoint $root $marker)
    $null = Wait-Until {
        $edit.Text -ceq $marker -and $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle
    } 'Click did not paste the exact history text into the original editor'
    if ([Windows.Forms.Clipboard]::GetText() -cne $marker) { throw 'Windows clipboard differs from selected history text' }
    if ($Emoji) {
        Write-Output 'Verified physical WebView2 emoji click pasted into the original editor using an already-running Service'
        return
    }
    $null = Wait-Until { $main.HasExited } 'Unpinned WebView2 main window did not exit'
    if ($resident.HasExited) { throw 'Native resident exited with the main window' }
    Write-Output 'Verified shared WebView2 row click, exact clipboard and editor text, restored focus, and independent native resident'
    # The file must also parse as Windows PowerShell 5.1 without a UTF-8 BOM.
    Test-ResidentPanel $resident '^+v' ([regex]::Unescape('DezirClip \u5feb\u901f\u7c98\u8d34')) $edit $form
    Test-ResidentPanel $resident '^+a' ([regex]::Unescape('DezirClip \u622a\u56fe')) $edit $form
    if ($edit.Text -cne $marker -or [Windows.Forms.Clipboard]::GetText() -cne $marker) { throw 'Cancelled native panel changed clipboard or editor' }
    $endpoint = & $cli service desktop-endpoint | ConvertFrom-Json
    if ($LASTEXITCODE -or !$endpoint.pid) { throw 'Cannot locate the isolated Rust Service' }
    $service = Get-Process -Id $endpoint.pid
    if ($service.Path -ne (Join-Path (Split-Path $exe) 'dzc-service.exe')) { throw 'Unexpected Service process' }
    $roots = @($resident.Id, $main.Id, $service.Id) + @($webProcesses | ForEach-Object { [int]$_.ProcessId })
    $null = Wait-Until {
        !(Get-DesktopProcesses $roots | Where-Object { $_.Name -eq 'msedgewebview2.exe' })
    } 'WebView2 children remained after closing the main window and both native panels'
    $samples = @(foreach ($index in 1..3) {
        Start-Sleep -Seconds 1
        if ($resident.HasExited -or $service.HasExited) { throw 'Resident or Service exited during idle measurement' }
        $members = @(Get-DesktopProcesses $roots)
        if ($members.Name -contains 'msedgewebview2.exe') { throw 'WebView2 restarted during native idle' }
        [pscustomobject]@{ processes = $members.Count; private_MiB = [Math]::Round(($members | Measure-Object PrivatePageCount -Sum).Sum / 1MB, 2);
            working_set_MiB = [Math]::Round(($members | Measure-Object WorkingSetSize -Sum).Sum / 1MB, 2) }
    })
    $table = $samples | Format-Table -AutoSize | Out-String
    Write-Output "Windows x64 MyGo idle after quick paste and screenshot dismissal (resident, Service and descendants):`n$table"
    if ($env:GITHUB_STEP_SUMMARY) {
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Encoding UTF8 -Value "### Windows x64 MyGo idle memory`n`nQuick paste and screenshot were opened and dismissed in the resident process. No WebView2 process was present in the observed process tree. Three samples, MiB.`n`n~~~`n$table~~~`n"
    }
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
    if ($main -and !$main.HasExited) { $main.Kill(); $main.WaitForExit() }
    & $cli service stop | Out-Null
    $form.Dispose()
}
