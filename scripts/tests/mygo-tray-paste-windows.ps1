# Runs only inside the disposable Windows desktop fixture.
$null = Wait-Until { Get-ControlPoint $root $marker } 'History was not ready for tray paste'
$handle = $main.MainWindowHandle
foreach ($mode in @('left-click', 'menu', 'pinned-reactivation')) {
    if ($mode -ne 'pinned-reactivation') {
        if (![MyGoInput]::ClickResidentTray($resident.Id)) { throw 'Cannot hide the main window from tray' }
        $null = Wait-Until { ![MyGoInput]::IsWindowVisible($handle) } 'Tray did not hide the main window'
    }
    $form.Activate()
    $null = $edit.Focus()
    $edit.Clear()
    $null = Wait-Until { $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Paste editor did not receive focus'
    # Let the native foreground event arrive before Explorer takes activation.
    Start-Sleep -Milliseconds 100
    if ($mode -ne 'pinned-reactivation') {
        $taskbar = [MyGoInput]::Taskbar()
        if (!$taskbar -or ![MyGoInput]::SetForegroundWindow($taskbar)) { throw 'Cannot reproduce taskbar foreground activation' }
        $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $taskbar } 'Taskbar did not become foreground'
        if ($mode -eq 'left-click') {
            if (![MyGoInput]::ClickResidentTray($resident.Id)) { throw 'Cannot show the main window from tray' }
        } else {
            if (![MyGoInput]::OpenResidentTrayMenu($resident.Id)) { throw 'Cannot open tray menu for paste' }
            $null = Wait-Until { [MyGoInput]::ResidentTrayMenuVisible($resident.Id) } 'Tray menu did not open'
            $showRect = [MyGoInput+Rect]::new()
            $popup = [MyGoInput]::ResidentTrayItem($resident.Id, 0, [ref]$showRect)
            if ($popup -eq [IntPtr]::Zero) { throw 'Tray show menu item missing' }
            Click-WindowPoint $popup ([Drawing.Point]::new(($showRect.left+$showRect.right)/2, ($showRect.top+$showRect.bottom)/2))
        }
        $null = Wait-Until { [MyGoInput]::IsWindowVisible($handle) -and [MyGoInput]::GetForegroundWindow() -eq $handle } 'Tray did not restore the main window'
        $title = Get-ControlPoint $root 'DezirClip' ([Windows.Automation.ControlType]::Text)
        $before = [MyGoInput+Rect]::new()
        $null = [MyGoInput]::GetWindowRect($handle, [ref]$before)
        $null = [MyGoInput]::SetCursorPos($title.X, $title.Y)
        [MyGoInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        $null = [MyGoInput]::SetCursorPos($title.X - 80, $title.Y + 50)
        Start-Sleep -Milliseconds 100
        [MyGoInput]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
        $null = Wait-Until {
            $after = [MyGoInput+Rect]::new()
            [MyGoInput]::GetWindowRect($handle, [ref]$after) -and ($after.left -ne $before.left -or $after.top -ne $before.top)
        } 'Physical title drag did not move the pinned window'
        if ($mode -eq 'left-click') {
            Click-Control $main (Get-ControlPoint $root 'Pin' ([Windows.Automation.ControlType]::Button))
            $null = Wait-Until { ([MyGoInput]::GetWindowLongPtr($handle, -20).ToInt64() -band 8) -ne 0 } 'Dragged window did not pin'
        }
    }
    Click-WindowPoint $handle (Get-ControlPoint $root $marker)
    $null = Wait-Until { $edit.Text -ceq $marker -and $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } "Exact click-to-paste failed after $mode"
    if (![MyGoInput]::IsWindowVisible($handle)) { throw 'Pinned window hid after paste' }
    Write-Output "Verified exact single-click paste after $mode, including taskbar focus, title drag and pin"
}
