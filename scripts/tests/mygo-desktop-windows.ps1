param([Parameter(Mandatory)][string]$Desktop, [switch]$Emoji, [switch]$Capture, [switch]$PinPreview, [switch]$Elevation, [switch]$InstalledElevation, [switch]$MiddleShortcut, [switch]$RichPaste, [switch]$DisabledPanels, [switch]$Quit, [switch]$RecoverService, [switch]$QuickPasteInput, [switch]$FileDrag, [switch]$Lifecycle, [switch]$WakeTiming, [switch]$WakeBackground, [ValidateSet(5, 16)][int]$ExpectedPointerMargin = 16)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    [Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
    throw 'The clipboard and foreground-window test requires a disposable GitHub-hosted Windows STA session'
}
. (Join-Path $PSScriptRoot 'service-process-receipt.ps1')
Add-Type -AssemblyName System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing @'
using System;
using System.Runtime.InteropServices;
public sealed class MyGoPasteTargetTextBox : System.Windows.Forms.TextBox {
    readonly System.Collections.Generic.Queue<string> messages = new System.Collections.Generic.Queue<string>();
    [DllImport("user32.dll")] static extern short GetKeyState(int key);
    [DllImport("user32.dll")] static extern IntPtr GetFocus();
    [DllImport("user32.dll")] static extern IntPtr GetOpenClipboardWindow();
    void Record(string phase, int message, IntPtr parameter) {
        IntPtr clipboardWindow = GetOpenClipboardWindow();
        uint clipboardPid;
        MyGoInput.GetWindowThreadProcessId(clipboardWindow, out clipboardPid);
        if (messages.Count == 128) messages.Dequeue();
        messages.Enqueue(String.Format(
            "tick={0} phase={1} msg=0x{2:x} key=0x{3:x} ctrl={4} shift={5} alt={6} focus={7} foreground={8} clipboardWindow={9} clipboardPid={10} length={11}",
            System.Diagnostics.Stopwatch.GetTimestamp(), phase, message, parameter.ToInt64(),
            GetKeyState(0x11) < 0, GetKeyState(0x10) < 0, GetKeyState(0x12) < 0,
            GetFocus(), MyGoInput.GetForegroundWindow(), clipboardWindow, clipboardPid, TextLength));
    }
    public override bool PreProcessMessage(ref System.Windows.Forms.Message message) {
        bool tracked = message.Msg == 0x100 || message.Msg == 0x101 || message.Msg == 0x104 || message.Msg == 0x105;
        if (tracked) Record("preprocess-before", message.Msg, message.WParam);
        bool handled = base.PreProcessMessage(ref message);
        if (tracked) Record(handled ? "preprocess-handled" : "preprocess-unhandled", message.Msg, message.WParam);
        return handled;
    }
    protected override void WndProc(ref System.Windows.Forms.Message message) {
        bool tracked = message.Msg == 0x7 || message.Msg == 0x8 || message.Msg == 0x100 || message.Msg == 0x101 || message.Msg == 0x302;
        if (tracked) Record("dispatch-before", message.Msg, message.WParam);
        base.WndProc(ref message);
        if (tracked) Record("dispatch-after", message.Msg, message.WParam);
    }
    public string InputDiagnostics() { return String.Join(Environment.NewLine, messages.ToArray()); }
}
public static class MyGoInput {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    public static void WinV() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }
    [DllImport("user32.dll", SetLastError=true)] static extern bool RegisterHotKey(IntPtr window, int id, uint modifiers, uint key);
    [DllImport("user32.dll", SetLastError=true)] static extern bool UnregisterHotKey(IntPtr window, int id);
    public static bool MainTestShortcutAvailable() {
        const int id = 0xD2C;
        if (!RegisterHotKey(IntPtr.Zero, id, 2 | 4, 0x78)) return false; // Ctrl+Shift+F9
        if (!UnregisterHotKey(IntPtr.Zero, id)) throw new System.ComponentModel.Win32Exception();
        return true;
    }
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr buffer, int size, out int needed);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static int IntegrityLevel(IntPtr process) {
        IntPtr token;
        if (!OpenProcessToken(process, 8, out token)) throw new System.ComponentModel.Win32Exception();
        try {
            int size;
            GetTokenInformation(token, 25, IntPtr.Zero, 0, out size);
            if (size == 0) throw new System.ComponentModel.Win32Exception();
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try {
                if (!GetTokenInformation(token, 25, buffer, size, out size)) throw new System.ComponentModel.Win32Exception();
                var sid = new System.Security.Principal.SecurityIdentifier(Marshal.ReadIntPtr(buffer));
                if (!sid.Value.StartsWith("S-1-16-", StringComparison.Ordinal)) throw new InvalidOperationException("Invalid integrity SID");
                return Int32.Parse(sid.Value.Substring(7));
            } finally { Marshal.FreeHGlobal(buffer); }
        } finally { CloseHandle(token); }
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int x, y; }
    public struct Rect { public int left, top, right, bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
    public static IntPtr WindowWithTitle(string title) { return FindWindow(null,title); }
    delegate bool EnumWindowCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowCallback callback, IntPtr parameter);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, System.Text.StringBuilder name, int size);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll")] static extern bool GetMenuItemRect(IntPtr window, IntPtr menu, uint item, out Rect rect);
    public static IntPtr ResidentTrayExit(int process, out Rect rect) {
        return ResidentTrayItem(process, 2, out rect);
    }
    public static IntPtr ResidentTrayItem(int process, uint item, out Rect rect) {
        rect = new Rect();
        if (!ResidentTrayMenuVisible(process)) return IntPtr.Zero;
        IntPtr popup = FindWindow("#32768", null);
        IntPtr menu = SendMessage(popup, 0x1E1, IntPtr.Zero, IntPtr.Zero);
        return GetMenuItemRect(IntPtr.Zero, menu, item, out rect) ? popup : IntPtr.Zero;
    }
    public static bool ResidentTrayMenuVisible(int process) {
        IntPtr popup = FindWindow("#32768", null);
        uint owner; GetWindowThreadProcessId(popup, out owner);
        if (popup == IntPtr.Zero || owner != process || !IsWindowVisible(popup)) return false;
        IntPtr menu = SendMessage(popup, 0x1E1, IntPtr.Zero, IntPtr.Zero); // MN_GETHMENU
        return menu != IntPtr.Zero && GetMenuItemCount(menu) == 3;
    }
    public static bool OpenResidentTrayMenu(int process) { return ResidentTrayEvent(process, 0x205); }
    public static bool ClickResidentTray(int process) { return ResidentTrayEvent(process, 0x202); }
    public struct IoCounters { public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes; }
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetProcessIoCounters(IntPtr process, out IoCounters counters);
    static bool ResidentTrayEvent(int process, int message) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr unused) {
            uint owner; GetWindowThreadProcessId(window, out owner);
            var name = new System.Text.StringBuilder(128); GetClassName(window, name, 128);
            if (owner == process && name.ToString() == "MyGoApp") { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        // The pinned MyGo backend's notification-area callbacks.
        return found != IntPtr.Zero && PostMessage(found, 0x8003, new IntPtr(1), new IntPtr(message));
    }
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, System.Text.StringBuilder title, int capacity);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateWindowEx(
        uint ex, string cls, string title, uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr data);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool UpdateWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    public static IntPtr PreviewBackdrop(IntPtr preview, Rect r, bool white) {
        int width=r.right-r.left, height=r.bottom-r.top;
        // A non-activating stock STATIC control paints SS_BLACKRECT or SS_WHITERECT below the flyout.
        IntPtr hwnd=CreateWindowEx(0x08000080,"STATIC","",0x80000000u | (white?6u:4u),
            r.left,r.top,width,height,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
        if (hwnd==IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
        if (!SetWindowPos(hwnd,preview,r.left,r.top,width,height,0x50)) {
            DestroyWindow(hwnd); throw new InvalidOperationException("Cannot place preview backdrop");
        }
        UpdateWindow(hwnd);
        return hwnd;
    }
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr window, uint attribute, out uint value, uint size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
    [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32.dll")] static extern bool CloseClipboard();
    [DllImport("user32.dll", SetLastError=true)] static extern IntPtr GetClipboardData(uint format);
    [DllImport("kernel32.dll", SetLastError=true)] static extern UIntPtr GlobalSize(IntPtr memory);
    [DllImport("user32.dll")] static extern IntPtr GetClipboardOwner();
    [DllImport("user32.dll")] static extern bool IsClipboardFormatAvailable(uint format);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr memory);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr memory);

    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    public static bool CheckClipboardText(string expected, uint sequence, IntPtr readerWindow) {
        uint readerPid;
        if (readerWindow == IntPtr.Zero || GetWindowThreadProcessId(readerWindow, out readerPid) != GetCurrentThreadId())
            throw new InvalidOperationException("Clipboard verification requires a window on the calling thread");
        if (!OpenClipboard(readerWindow)) return false;
        try {
            if (sequence != GetClipboardSequenceNumber()) throw new InvalidOperationException("Clipboard sequence changed before verification");
            IntPtr handle = GetClipboardData(13); // CF_UNICODETEXT
            int readError = Marshal.GetLastWin32Error();
            ulong size = handle == IntPtr.Zero ? 0 : GlobalSize(handle).ToUInt64();
            int sizeError = Marshal.GetLastWin32Error();
            if (handle == IntPtr.Zero || size < (ulong)(expected.Length + 1) * 2) {
                uint ownerPid;
                GetWindowThreadProcessId(GetClipboardOwner(), out ownerPid);
                throw new InvalidOperationException(String.Format(
                    "Clipboard does not contain the complete selected text: handle={0}, bytes={1}, expectedBytes={2}, readError={3}, sizeError={4}, available={5}, ownerPid={6}, sequence={7}",
                    handle.ToInt64(), size, (expected.Length + 1) * 2, readError, sizeError, IsClipboardFormatAvailable(13), ownerPid, sequence));
            }
            IntPtr data = GlobalLock(handle);
            if (data == IntPtr.Zero) throw new InvalidOperationException("Clipboard text handle is unreadable");
            try {
                if (Marshal.PtrToStringUni(data, expected.Length) != expected || Marshal.ReadInt16(data, expected.Length * 2) != 0)
                    throw new InvalidOperationException("Clipboard text differs from the selected entry");
            } finally { GlobalUnlock(handle); }
            return true;
        } finally { CloseClipboard(); }
    }
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

function Get-ControlPoint($root, [string]$Name, [Windows.Automation.ControlType]$Type = $null) {
    $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Name, [Windows.Automation.PropertyConditionFlags]::IgnoreCase)
    if ($Type) {
        $condition = [Windows.Automation.AndCondition]::new($condition,
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, $Type))
    }
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

function Get-SettingsCheckbox($root, [string]$Label = 'Disable WebView GPU') {
    $name = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Label)
    $type = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::CheckBox)
    $root.FindFirst([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.AndCondition]::new($type, $name))
}

function Test-EdgeDocking($Main, $Root) {
    if (!(Get-SettingsCheckbox $Root 'Edge Docking')) { Click-Control $Main (Get-ControlPoint $Root 'General Settings') }
    $box = Wait-Until { Get-SettingsCheckbox $Root 'Edge Docking' } 'Edge docking setting unavailable'
    $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
    $window = $Main.MainWindowHandle
    $screen = [Windows.Forms.Screen]::FromHandle($window).Bounds
    $before = [MyGoInput+Rect]::new()
    if (![MyGoInput]::GetWindowRect($window, [ref]$before)) { throw 'Cannot read initial window bounds' }
    $left = $screen.Right - ($before.right - $before.left)
    $top = $screen.Top + 60
    $insideY = $top + 80
    $null = [MyGoInput]::SetCursorPos($screen.Right - 25, $insideY)
    if (![MyGoInput]::SetWindowPos($window, [IntPtr]::Zero, $left, $top, 0, 0, 0x15)) { throw 'Cannot position the test window at the right edge' }
    $toggle.Toggle()
    $null = Wait-Until { $toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::On } 'Edge docking setting did not turn on'
    $null = [MyGoInput]::SetCursorPos($screen.Left + 20, $insideY)
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($window) } 'Windows edge docking did not retract the WebView'
    $null = [MyGoInput]::SetCursorPos($screen.Right - 1, $screen.Top + 10)
    Start-Sleep -Milliseconds 550
    if ([MyGoInput]::IsWindowVisible($window)) { throw 'Unrelated edge position revealed the WebView' }
    $null = [MyGoInput]::SetCursorPos($screen.Right - 1, $insideY)
    $null = Wait-Until { [MyGoInput]::IsWindowVisible($window) } 'Moving onto the original edge did not reveal the WebView'
    $revealed = [MyGoInput+Rect]::new()
    if (![MyGoInput]::GetWindowRect($window, [ref]$revealed) -or $revealed.right -gt $screen.Right -or $revealed.left -lt $screen.Left) { throw 'Revealed window is clipped' }
    $null = [MyGoInput]::SetCursorPos($screen.Left + 20, $insideY)
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($window) } 'Revealed window lost its docking anchor'
    $null = [MyGoInput]::SetCursorPos($screen.Right - 1, $insideY)
    $null = Wait-Until { [MyGoInput]::IsWindowVisible($window) } 'Second edge reveal failed'
    $box = Wait-Until { Get-SettingsCheckbox $Root 'Edge Docking' } 'Settings did not resume after the window returned'
    $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
    $toggle.Toggle()
    $null = Wait-Until { $toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::Off } 'Edge docking setting did not turn off'
    $null = [MyGoInput]::SetCursorPos($screen.Left + 20, $insideY)
    Start-Sleep -Milliseconds 1100
    if (![MyGoInput]::IsWindowVisible($window)) { throw 'Disabled edge docking still retracted the WebView' }
    $null = [MyGoInput]::SetWindowPos($window, [IntPtr]::Zero, $before.left, $before.top, 0, 0, 0x15)
    Write-Output 'Verified WebView2 edge docking from Settings, right-edge retraction, bounded reveal and disabling'
}

function Get-WebViewBrowser($main) {
    Wait-Until {
        Get-CimInstance Win32_Process -Filter ("ParentProcessId={0} AND Name='msedgewebview2.exe'" -f $main.Id) |
            Where-Object { $_.CommandLine -and $_.CommandLine -notmatch '(?:^|\s)--type=' } | Select-Object -First 1
    } 'No live WebView2 browser for the current MyGo main window'
}

function Click-Control($main, $point) {
    if ([MyGoInput]::GetForegroundWindow() -ne $main.MainWindowHandle) { throw 'MyGo main window lost focus before click' }
    Click-WindowPoint $main.MainWindowHandle $point
}

function Click-WindowPoint([IntPtr]$Window, $point) {
    if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not position pointer on the control' }
    $hit = New-Object MyGoInput+Point
    $hit.x, $hit.y = $point.X, $point.Y
    if ([MyGoInput]::GetAncestor([MyGoInput]::WindowFromPoint($hit), 2) -ne $Window) {
        throw 'Control coordinates do not belong to the expected MyGo window'
    }
    [MyGoInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 120
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

function Test-RegisteredCli($root, $cli) {
    # This script only runs in a disposable GitHub-hosted Windows session.
    $environment = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try {
        $before = $environment.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($null -ne $before) { $kind = $environment.GetValueKind('Path') }
        $scope = [Windows.Automation.TreeScope]::Descendants
        $add = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Add to PATH')
        $configured = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'dzc (configured for this user)')
        $button = Wait-Until { $root.FindFirst($scope, $add) } 'Missing Add to PATH for the isolated launch environment'
        $button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
        $null = Wait-Until { $root.FindFirst($scope, $configured) -and !$root.FindFirst($scope, $add) } 'Installed CLI was still reported missing'
        $info = & $cli desktop-tools info | ConvertFrom-Json
        if (!$info.cli_registered_for_user) { throw 'Fresh sibling CLI did not read the persisted user registration' }
        Write-Output 'Verified WebView2 reports the persisted Windows CLI registration after refreshing in a fresh child'
    } finally {
        try {
            if ($null -ne $before) { $environment.SetValue('Path', $before, $kind) }
            else { $environment.DeleteValue('Path', $false) }
        } finally { $environment.Dispose() }
    }
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

function Test-QuickPasteInput($Resident, $Editor, $Form, [string]$Marker) {
    foreach ($method in @('click', 'enter', 'click')) {
        $Editor.Text = ''
        $Form.Activate()
        $null = $Editor.Focus()
        $null = Wait-Until { $Editor.Focused -and [MyGoInput]::GetForegroundWindow() -eq $Form.Handle } 'Quick paste target did not gain focus'
        [Windows.Forms.SendKeys]::SendWait('^+v')
        $panel = Get-ResidentPanel $Resident ([regex]::Unescape('DezirClip \u5feb\u901f\u7c98\u8d34'))
        $root = [Windows.Automation.AutomationElement]::FromHandle($panel)
        $point = Get-ControlPoint $root $Marker
        if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not hover the native row' }
        Start-Sleep -Milliseconds 150
        if ($Editor.Text -ne '') { throw 'Hover submitted a paste' }
        $delivery = [Diagnostics.Stopwatch]::StartNew()
        if ($method -eq 'click') { Click-WindowPoint $panel $point }
        else { [Windows.Forms.SendKeys]::SendWait('{ENTER}') }
        $null = Wait-Until {
            $Editor.Text -ceq $Marker -and $Editor.Focused -and [MyGoInput]::GetForegroundWindow() -eq $Form.Handle -and ![MyGoInput]::IsWindowVisible($panel)
        } "Native quick-paste $method failed to deliver once and restore focus" 3
        Write-Output "Native quick-paste $method delivered once in $($delivery.ElapsedMilliseconds) ms"
    }
    # Add more than one protocol page to this disposable profile. The last row
    # must become reachable by the wheel without a pagination button.
    foreach ($index in 64..0) {
        & $cli add ('MyGo scroll fixture {0:D3}' -f $index) | Out-Null
        if ($LASTEXITCODE) { throw 'Cannot seed native scroll pagination' }
    }
    $Editor.Text = ''
    $Form.Activate()
    $null = $Editor.Focus()
    $null = Wait-Until { $Editor.Focused -and [MyGoInput]::GetForegroundWindow() -eq $Form.Handle } 'Scroll paste target did not gain focus'
    [Windows.Forms.SendKeys]::SendWait('^+v')
    $panel = Get-ResidentPanel $Resident ([regex]::Unescape('DezirClip \u5feb\u901f\u7c98\u8d34'))
    $root = [Windows.Automation.AutomationElement]::FromHandle($panel)
    $null = Get-ControlPoint $root 'MyGo scroll fixture 000'
    $button = $root.FindFirst([Windows.Automation.TreeScope]::Descendants,
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Load more'))
    if ($button) { throw 'Quick paste still exposes a Load more button' }
    $bounds = [MyGoInput+Rect]::new()
    if (![MyGoInput]::GetWindowRect($panel, [ref]$bounds)) { throw 'Cannot locate the quick-paste viewport' }
    $null = [MyGoInput]::SetCursorPos(($bounds.left+$bounds.right)/2, ($bounds.top+$bounds.bottom)/2)
    $target = 'MyGo scroll fixture 055'
    $row = Wait-Until {
        $item = $root.FindFirst([Windows.Automation.TreeScope]::Descendants,
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $target))
        if ($item -and !$item.Current.IsOffscreen) { return $item }
        [MyGoInput]::mouse_event(0x800, 0, 0, ([uint32]::MaxValue - 119), [UIntPtr]::Zero)
        return $null
    } 'Wheel scrolling did not automatically load the second page' 15
    $r = $row.Current.BoundingRectangle
    Click-WindowPoint $panel ([Drawing.Point]::new($r.Left+$r.Width/2, $r.Top+$r.Height/2))
    $null = Wait-Until { $Editor.Text -ceq $target -and $Editor.Focused -and ![MyGoInput]::IsWindowVisible($panel) } 'Appended row did not paste once into the original target' 3
    Write-Output 'Verified wheel-only pagination across 50 rows, no Load more button, and single-click paste of an appended row'
}

function Get-SavedMainHotkeys {
    $value = 'import os, json, sqlite3; print(json.dumps(sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]).execute("SELECT value FROM settings WHERE key=?", ("app.hotkey",)).fetchone()[0]))' | python -
    if ($LASTEXITCODE) { throw 'Cannot read saved main shortcut list' }
    return ($value | ConvertFrom-Json)
}

function Test-MainHotkeys($Main, $Root, $Resident, [string]$Executable, $Editor, $Form) {
    $original = Get-SavedMainHotkeys
    if (![MyGoInput]::MainTestShortcutAvailable()) { throw 'Test shortcut was occupied before editing' }
    Click-Control $Main (Get-ControlPoint $Root 'Clipboard')
    Click-Control $Main (Get-ControlPoint $Root 'Global Hotkey' ([Windows.Automation.ControlType]::Button))
    $name = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Global Hotkey')
    $type = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button)
    $condition = [Windows.Automation.AndCondition]::new($type, $name)
    $null = Wait-Until {
        $button = $Root.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
        if (!$button) { return $false }
        $pattern = $null
        $button.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$pattern) -and
            $pattern.Current.ToggleState -eq [Windows.Automation.ToggleState]::On -and $button.Current.HasKeyboardFocus
    } 'Main shortcut editor did not enter recording mode'
    [Windows.Forms.SendKeys]::SendWait('^+{F9}')
    $null = Get-ControlPoint $Root 'Delete Ctrl+Shift+F9'
    $expected = if ($original) { $original + "`nCtrl+Shift+F9" } else { 'Ctrl+Shift+F9' }
    if ((Get-SavedMainHotkeys) -cne $expected) { throw 'Main shortcut was shown without persisting the complete list' }
    if ([MyGoInput]::MainTestShortcutAvailable()) { throw 'Saved main shortcut has no native registration' }
    $Form.Activate()
    $null = $Editor.Focus()
    $null = Wait-Until { $Editor.Focused -and [MyGoInput]::GetForegroundWindow() -eq $Form.Handle } 'Editor did not regain foreground before shortcut activation'
    [Windows.Forms.SendKeys]::SendWait('^+{F9}')
    $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $Main.MainWindowHandle } 'New main shortcut did not activate the WebView'
    [Windows.Forms.SendKeys]::SendWait('^+{F9}')
    $handle = $Main.MainWindowHandle
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($handle) } 'New main shortcut did not hide the foreground WebView'
    if ($Main.HasExited) { throw 'Main shortcut destroyed the foreground WebView' }
    [Windows.Forms.SendKeys]::SendWait('^+{F9}')
    $reopened = Wait-Until {
        if ($Resident.HasExited) { throw 'Main shortcut closed the resident' }
        Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
            $_.Id -ne $Resident.Id -and $_.Path -eq $Executable -and $_.MainWindowTitle -eq 'DezirClip'
        } | Select-Object -First 1
    } 'Saved shortcut did not reveal the main WebView'
    if ($reopened.Id -ne $Main.Id) { throw 'Shortcut wake replaced the hidden WebView' }
    $newRoot = [Windows.Automation.AutomationElement]::FromHandle($reopened.MainWindowHandle)
    Click-Control $reopened (Get-ControlPoint $newRoot 'Clipboard')
    Click-Control $reopened (Get-ControlPoint $newRoot 'Delete Ctrl+Shift+F9')
    $null = Wait-Until { (Get-SavedMainHotkeys) -ceq $original } 'Removing the new shortcut did not preserve original bindings'
    if (![MyGoInput]::MainTestShortcutAvailable()) { throw 'Removed main shortcut was not released at the OS level' }
    Write-Host 'Verified WebView2 shortcut editing, persistence, native registration, same-process hide/wake and removal'
    return $reopened
}

function Test-WinVTakeover($Main, $Root) {
    $original = Get-SavedMainHotkeys
    function Answer-Restart([string]$Answer) {
        $dialog = Wait-Until {
            $name = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Restart Explorer Required')
            $process = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $Main.Id)
            [Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children,
                [Windows.Automation.AndCondition]::new($name, $process))
        } 'Win+V did not reach the Explorer confirmation'
        $button = $dialog.FindFirst([Windows.Automation.TreeScope]::Descendants,
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Answer))
        if (!$button) { throw 'Explorer confirmation button missing' }
        $button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    $box = Wait-Until { Get-SettingsCheckbox $Root 'Use Win+V Shortcut' } 'Win+V setting missing'
    $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
    if ($toggle.Current.ToggleState -ne [Windows.Automation.ToggleState]::Off) { throw 'Win+V fixture must start disabled' }
    $toggle.Toggle()
    Answer-Restart 'Cancel'
    $null = Wait-Until { $toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::Off } 'Cancelled Win+V takeover remained enabled'
    $null = Wait-Until { $box.Current.IsEnabled } 'Cancelled Win+V transaction did not finish'
    if ((Get-SavedMainHotkeys) -cne $original) { throw 'Cancelled Win+V takeover changed shortcuts' }
    $toggle.Toggle()
    Answer-Restart 'OK'
    $null = Wait-Until { (Get-SavedMainHotkeys) -match '(?im)^(Win|Super)\+V$' } 'Win+V was not saved after Explorer released it'
    $null = Wait-Until { $box.Current.IsEnabled } 'Win+V takeover did not finish'
    $window = $Main.MainWindowHandle
    $null = [MyGoInput]::SetForegroundWindow($window)
    $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $window } 'Win+V fixture lost foreground'
    [MyGoInput]::WinV()
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($window) } 'Win+V did not hide the window'
    if ($Main.HasExited) { throw 'Win+V destroyed the WebView' }
    [MyGoInput]::WinV()
    $null = Wait-Until { [MyGoInput]::IsWindowVisible($window) } 'Win+V did not restore the same window'
    $toggle.Toggle()
    Answer-Restart 'OK'
    $null = Wait-Until { (Get-SavedMainHotkeys) -ceq $original } 'Disabling Win+V did not restore shortcuts'
    $null = Wait-Until { $box.Current.IsEnabled } 'Win+V system restoration did not finish'
    Write-Output 'Verified cancelled and successful Win+V takeover, physical toggle, and system restoration'
}

function Open-HoverPreview($root, $main, [string]$marker) {
    $point = Get-ControlPoint $root $marker
    if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not hover the preview fixture' }
    $preview = Wait-Until {
        $handle = [MyGoInput]::WindowWithTitle([regex]::Unescape('DezirClip \u60ac\u6d6e\u9884\u89c8'))
        if ($handle -and [MyGoInput]::IsWindowVisible($handle)) { return $handle }
    } 'Native hover preview did not open'
    if ([MyGoInput]::GetForegroundWindow() -ne $main.MainWindowHandle) { throw 'Preview took keyboard focus' }
    return $preview
}

function Test-PreviewPixels([IntPtr]$preview, [switch]$HalfOpacity) {
    $bounds = New-Object MyGoInput+Rect
    if (![MyGoInput]::GetWindowRect($preview, [ref]$bounds)) { throw 'Cannot locate native preview' }
    $width = $bounds.right-$bounds.left
    $height = $bounds.bottom-$bounds.top
    $captureFrame = {
        $image = [Drawing.Bitmap]::new($width,$height)
        $graphics = [Drawing.Graphics]::FromImage($image)
        try { $graphics.CopyFromScreen($bounds.left,$bounds.top,0,0,$image.Size) } finally { $graphics.Dispose() }
        return $image
    }
    $firstPixel = $null
    $firstColor = $null
    foreach ($white in @($false,$true)) {
        $backdrop = [MyGoInput]::PreviewBackdrop($preview,$bounds,$white)
        try {
            # The stock rectangles use system theme colors. Render the backdrop alone to measure its actual fill.
            $reference = [Drawing.Bitmap]::new($width,$height)
            try {
                $graphics = [Drawing.Graphics]::FromImage($reference)
                try {
                    $dc = $graphics.GetHdc()
                    try {
                        if (![MyGoInput]::PrintWindow($backdrop,$dc,1)) { throw 'Cannot render the preview reference backdrop' }
                    } finally { $graphics.ReleaseHdc($dc) }
                } finally { $graphics.Dispose() }
                $color = $reference.GetPixel([int]($width/2),$height-22)
                $expected = [Drawing.Color]::FromArgb(255,$color.R,$color.G,$color.B).ToArgb()
            } finally { $reference.Dispose() }
            if ($white -and [Math]::Abs([int]$color.R - [int]$firstColor.R) -lt 64) { throw 'Preview reference backdrops lack contrast' }
            if (!$white) { $firstColor = $color }
            $sampleX = [int]($width/2)
            $sampleY = $height-22
            $last = @{}
            try {
                $null = Wait-Until {
                    [Windows.Forms.Application]::DoEvents()
                    $image = & $captureFrame
                    try {
                        $last.pixel = $image.GetPixel($sampleX,$sampleY).ToArgb()
                        if ($HalfOpacity) {
                            if (!$white -and $last.pixel -eq $expected) { return $false }
                            if ($white) {
                                $contrast = [Math]::Abs($image.GetPixel($sampleX,$sampleY).R - $firstPixel.R)
                                $referenceContrast = [Math]::Abs($color.R - $firstColor.R)
                                if ($contrast -le 3 -or $contrast -ge $referenceContrast - 5) { return $false }
                            }
                        } elseif ($last.pixel -ne $expected) { return $false }
                        $ink = 0
                        for ($y=8; $y -lt $height/2; $y+=2) {
                            for ($x=8; $x -lt $width-8; $x+=2) {
                                if ($image.GetPixel($x,$y).ToArgb() -ne $last.pixel) { $ink++ }
                            }
                        }
                        return ($white -or $ink -ge 8) -and [MyGoInput]::IsWindowVisible($preview)
                    } finally { $image.Dispose() }
                } 'Native preview must show both the controlled backdrop and visible content' 4
            } finally {
                Write-Output ("Preview bottom pixel={0:X8}, backdrop={1:X8}" -f $last.pixel,$expected)
            }
            if (!$white) { $firstPixel = [Drawing.Color]::FromArgb($last.pixel) }
        } finally { $null = [MyGoInput]::DestroyWindow($backdrop) }
    }
}

function Test-TransparentPreview($root, $main, [string]$marker) {
    $preview = Open-HoverPreview $root $main $marker
    Test-PreviewPixels $preview
    $away = Get-ControlPoint $root 'Settings'
    if (![MyGoInput]::SetCursorPos($away.X,$away.Y)) { throw 'Could not leave the selected row' }
    $null = Wait-Until { ![MyGoInput]::IsWindowVisible($preview) } 'Preview remained after leaving the row'

}

function Test-PinnedPreview($root, $main, $resident, [string]$marker) {
    $hover = Open-HoverPreview $root $main $marker
    $toolbar = [Windows.Automation.AutomationElement]::FromHandle($hover)
    Click-WindowPoint $hover (Get-ControlPoint $toolbar 'Pin this preview')
    $pin = Wait-Until {
        $handle = [MyGoInput]::WindowWithTitle([regex]::Unescape('DezirClip \u56fa\u5b9a\u9884\u89c8'))
        if ($handle -and [MyGoInput]::IsWindowVisible($handle)) { return $handle }
    } 'Native pinned preview did not open'
    [uint32]$ownerId = 0
    $null = [MyGoInput]::GetWindowThreadProcessId($pin,[ref]$ownerId)
    if ($ownerId -ne $resident.Id) { throw 'Pinned preview did not belong to the native resident' }
    if (!$main.HasExited -and !$main.CloseMainWindow()) { throw 'Could not close the main WebView' }
    $null = Wait-Until { $main.HasExited } 'Main WebView process did not exit'
    Test-PreviewPixels $pin
    $controls = [Windows.Automation.AutomationElement]::FromHandle($pin)
    Click-WindowPoint $pin (Get-ControlPoint $controls 'Background opacity')
    $null = Wait-Until {
        $value = 'import os, sqlite3; print(sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]).execute("SELECT value FROM settings WHERE key=?", ("app.preview_opacity",)).fetchone()[0])' | python -
        $LASTEXITCODE -eq 0 -and [int]$value -ge 45 -and [int]$value -le 55
    } 'Native slider did not persist its midpoint after the WebView exited'
    Test-PreviewPixels $pin -HalfOpacity
    Write-Output 'Verified resident pin updates its on-screen background after the WebView process exits'
}

function Test-ExternalEdit($root, $main, [string]$marker) {
    $point = Get-ControlPoint $root $marker
    if (![MyGoInput]::SetCursorPos($point.X, $point.Y)) { throw 'Could not hover the saved text' }
    $walker = [Windows.Automation.TreeWalker]::ControlViewWalker
    $name = [Windows.Automation.AutomationElement]::NameProperty
    $scope = [Windows.Automation.TreeScope]::Descendants
    $button = Wait-Until {
        $row = $root.FindFirst($scope, [Windows.Automation.PropertyCondition]::new($name, $marker))
        while ($row -and $row.Current.ControlType -ne [Windows.Automation.ControlType]::Document) {
            $open = $row.FindFirst($scope, [Windows.Automation.PropertyCondition]::new($name, 'Open'))
            if ($open) { return $open }
            $row = $walker.GetParent($row)
        }
    } 'Selected history row had no Open action'
    $bounds = $button.Current.BoundingRectangle
    Click-Control $main ([Drawing.Point]::new([int]($bounds.Left + $bounds.Width/2), [int]($bounds.Top + $bounds.Height/2)))
    $null = Get-ControlPoint $root ($marker + ' externally edited')
    @'
import os, sqlite3, sys
with sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]) as db:
    assert db.execute("SELECT use_count FROM clipboard_history WHERE content=?", (sys.argv[1]+" externally edited",)).fetchall() == [(1,)]
'@ | python - $marker
    if ($LASTEXITCODE) { throw 'The edited Windows row did not record exactly one external open' }
    Write-Output 'Verified a WebView2 Open click launched the selected editor and the Service refreshed the same history row'
}

function Assert-CustomBackgroundPresent($window) {
    $frame = [MyGoInput+Rect]::new()
    if (![MyGoInput]::GetWindowRect($window.MainWindowHandle, [ref]$frame)) { throw 'Cannot inspect the visible main window' }
    $bitmap = [Drawing.Bitmap]::new($frame.right-$frame.left, $frame.bottom-$frame.top)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($frame.left, $frame.top, 0, 0, $bitmap.Size)
        $hits = 0
        for ($y = 24; $y -lt $bitmap.Height-24; $y += 24) {
            for ($x = 24; $x -lt $bitmap.Width-24; $x += 24) {
                $pixel = $bitmap.GetPixel($x, $y)
                if ([Math]::Min([int]$pixel.R-$pixel.G, [int]$pixel.B-$pixel.G) -gt 60) { $hits++ }
            }
        }
        if ($hits -lt 20) { throw "Visible main window lacks its custom background: $hits colored samples" }
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Test-OcrAfterReopen($Main, $Resident, [string]$Executable, $Form, $Editor) {
    $Form.Activate()
    $null = $Editor.Focus()
    $null = Wait-Until { $Editor.Focused } 'OCR fixture editor did not regain focus'
    $fixture = Join-Path $PSScriptRoot '../../frontends/dzc-surface/cmd/dzc-desktop/testdata/ocr-clipboard.png'
    $image = [Drawing.Image]::FromFile((Resolve-Path -LiteralPath $fixture).Path)
    try { [Windows.Forms.Clipboard]::SetImage($image) } finally { $image.Dispose() }
    foreach ($reopen in @($false, $true)) {
        if ($reopen) {
            if (!$Main.CloseMainWindow()) { throw 'Could not close the OCR WebView' }
            $null = Wait-Until { $Main.HasExited } 'OCR WebView did not exit'
        }
        if (!$reopen -and [MyGoInput]::IsWindowVisible($Main.MainWindowHandle)) {
            if (![MyGoInput]::ClickResidentTray($Resident.Id)) { throw 'Cannot hide OCR window through tray' }
            $null = Wait-Until { $Main.Refresh(); $Main.MainWindowHandle -eq [IntPtr]::Zero } 'OCR tray did not hide the visible background window'
        }
        if (![MyGoInput]::OpenResidentTrayMenu($Resident.Id)) { throw 'Could not open the OCR resident tray menu' }
        $null = Wait-Until { [MyGoInput]::ResidentTrayMenuVisible($Resident.Id) } 'OCR resident tray menu did not appear'
        $showRect = [MyGoInput+Rect]::new()
        $popup = [MyGoInput]::ResidentTrayItem($Resident.Id, 0, [ref]$showRect)
        if ($popup -eq [IntPtr]::Zero) { throw 'Could not locate the resident Show action' }
        Click-WindowPoint $popup ([Drawing.Point]::new(($showRect.left + $showRect.right)/2, ($showRect.top + $showRect.bottom)/2))
        if ($reopen) {
            $Main = Wait-Until {
                if ($Resident.HasExited) { throw 'Resident exited during OCR recovery' }
                Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                    $_.Id -ne $Resident.Id -and $_.Path -eq $Executable -and $_.MainWindowTitle -eq 'DezirClip'
                } | Select-Object -First 1
            } 'Main window did not reopen after automatic OCR'
        }
        $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $Main.MainWindowHandle } 'OCR tray activation did not bring the main window to foreground'
        $tree = [Windows.Automation.AutomationElement]::FromHandle($Main.MainWindowHandle)
        $button = Wait-Until {
            $tree.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
                Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.Name.EndsWith('OCR Text') -and !$_.Current.IsOffscreen } |
                Select-Object -First 1
        } 'Persisted automatic OCR was missing from the image row' 40
        $bounds = $button.Current.BoundingRectangle
        Click-Control $Main ([Drawing.Point]::new([int]($bounds.Left+$bounds.Width/2), [int]($bounds.Top+$bounds.Height/2)))
        $null = Get-ControlPoint $tree 'DESKTOP TEST'
    }
    Write-Host 'Verified automatic OCR text survives a fresh WebView and the main window remains reopenable'
    return $Main
}

function Get-DesktopProcesses([int[]]$RootIds, [datetime]$Since = [datetime]::MinValue) {
    $processes = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate,Name,WorkingSetSize,PrivatePageCount |
        Where-Object { !$_.CreationDate -or $_.CreationDate -ge $Since })
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
$fixtureDatabase = $env:DEZIRCLIP_DB_PATH
$installedRedirect = $null
$redirectBackup = $null
if ($InstalledElevation) { $Elevation = $true }
$marker = 'MyGo desktop fixture ' + [guid]::NewGuid().ToString('N')
$form = New-Object Windows.Forms.Form
$form.Text = 'Independent MyGo paste target'
$edit = New-Object MyGoPasteTargetTextBox
$form.Controls.Add($edit)
$resident = $null
$startedService = $null
$startedServiceProcess = $null
$serviceCrashReceipt = $null
$quitVerified = $false
try {
    & $cli add $marker | Out-Null
    if ($LASTEXITCODE) { throw 'Could not seed isolated clipboard history' }
    @'
import os, sqlite3
with sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]) as db:
    db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                   [("app.hotkey", "Ctrl+Shift+F8"), ("app.silent_start", "false"), ("app.emoji_panel_enabled", "true"), ("app.language","en"), ("app.theme", "acrylic"), ("app.color_mode", "dark"), ("app.show_app_border", "false"), ("app.window_width","670"), ("app.window_height","608")])
'@ | python -
    if ($LASTEXITCODE) { throw 'Could not seed isolated window appearance' }
    if ($FileDrag) {
        'import os, sqlite3; db=sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]); db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)", [("app.window_pinned","true"),("app.capture_files","true")]); db.commit()' | python -
        if ($LASTEXITCODE) { throw 'Could not pin native drag fixture' }
    }
    if ($DisabledPanels) {
        @'
import os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',
                   [('app.quick_paste_enabled','false'), ('app.screenshot_enabled','false')])
'@ | python -
        if ($LASTEXITCODE) { throw 'Could not seed disabled native panels' }
    }
    if ($RichPaste) {
        @'
import os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',
                   [('app.rich_paste_hotkey','Ctrl+Shift+F10'), ('app.delete_after_paste','true')])
'@ | python -
        if ($LASTEXITCODE) { throw 'Could not seed rich paste preferences' }
    }
    if ($MiddleShortcut) {
        @'
import os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    db.execute("UPDATE settings SET value='MButton' WHERE key='app.hotkey'")
'@ | python -
        if ($LASTEXITCODE) { throw 'Could not seed middle-button shortcut' }
    }
    if ($Capture -or $PinPreview) {
        @'
import os, sqlite3
with sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]) as db:
    db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                   [('app.compact_mode','true'), ('app.preview_opacity','0')])
'@ | python -
        if ($LASTEXITCODE) { throw 'Could not seed the isolated preview preferences' }
    }
    if ($Capture -or $WakeBackground) {
        # Resources inside the private profile must use its managed image namespace.
        $wallpaperDirectory = New-Item -ItemType Directory -Path (Join-Path $dir.FullName 'emoji_favorites') -Force
        $wallpaper = Join-Path $wallpaperDirectory.FullName 'fav_ca01.png'
        $bitmap = [Drawing.Bitmap]::new(32, 32)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([Drawing.Color]::Magenta)
            $bitmap.Save($wallpaper, [Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
        @'
import os, sqlite3, sys
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    db.executemany('INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)',
                   [('app.custom_background',sys.argv[1]), ('app.custom_background_opacity','100'), ('app.surface_opacity','20')])
'@ | python - $wallpaper
        if ($LASTEXITCODE) { throw 'Could not seed the isolated custom background' }
        if ($WakeBackground) {
            @'
import random, struct, sys, zlib
rng = random.Random(17)
def chunk(kind, data):
    return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
rows = bytearray()
for y in range(1024):
    rows.append(0)
    for x in range(1024):
        rows.extend((rng.randrange(192,256),rng.randrange(0,48),rng.randrange(192,256)))
with open(sys.argv[1],'wb') as out:
    out.write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',1024,1024,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b''))
'@ | python - $wallpaper
            if ($LASTEXITCODE) { throw 'Could not generate the large synthetic background' }
            Write-Host "Wake fixture PNG bytes=$((Get-Item -LiteralPath $wallpaper).Length)"
        }
    }
    if ($Capture) {
        $fixtureEditor = Join-Path $dir.FullName 'Fixture Editor.exe'
        Add-Type -OutputAssembly $fixtureEditor -OutputType WindowsApplication -TypeDefinition @'
using System.IO;
using System.Text;
public class FixtureEditor {
    public static void Main(string[] args) {
        if (args.Length != 1) return;
        using (var file = new FileStream(args[0], FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite)) {
            file.Seek(0, SeekOrigin.End);
            var bytes = Encoding.UTF8.GetBytes(" externally edited");
            file.Write(bytes, 0, bytes.Length);
            file.Flush(true);
        }
    }
}
'@
        @'
import os, sqlite3, sys
with sqlite3.connect(os.environ["DEZIRCLIP_DB_PATH"]) as db:
    db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.text',?)", (sys.argv[1],))
    db.execute("UPDATE settings SET value='true' WHERE key='app.silent_start'")
    # Automatic OCR is scheduled for new persistent images. This fixture must
    # opt into persistence rather than relying on the RAM-only profile default.
    db.executemany("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)",
                   [('app.persistent','true'), ('app.ocr_enabled','true')])
'@ | python - $fixtureEditor
        if ($LASTEXITCODE) { throw 'Could not select the isolated external editor' }
    }
    if ($RecoverService) {
        @'
import os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    db.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.search_hotkey','Ctrl+Shift+F9')")
'@ | python -
        if ($LASTEXITCODE) { throw 'Could not seed the recovery search shortcut' }
    }
    if ($InstalledElevation) {
        $config = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'io.github.qiyuanhuakai.dezirclip'
        $null = New-Item -ItemType Directory -Force -Path $config
        $installedRedirect = Join-Path $config 'datapath.txt'
        if (Test-Path -LiteralPath $installedRedirect) { $redirectBackup = [IO.File]::ReadAllBytes($installedRedirect) }
        [IO.File]::WriteAllText($installedRedirect, $dir.FullName, [Text.UTF8Encoding]::new($false))
        Remove-Item Env:DEZIRCLIP_DB_PATH -ErrorAction SilentlyContinue
        Remove-Item Env:TIEZ_DB_PATH -ErrorAction SilentlyContinue
    }
    if ($Capture) { [Windows.Forms.Clipboard]::SetText('Before ' + $marker) }
    if ($Emoji -or $Elevation -or $Capture -or $FileDrag) {
        if ($Capture -or ($env:DZC_SERVICE_CRASH_ROOT -and $env:DZC_SERVICE_CRASH_LAUNCH -ne 'cli')) {
            if ($Capture) {
                $startedServiceProcess = Start-Process -FilePath (Join-Path (Split-Path $exe) 'dzc-service.exe') -ArgumentList ('--database "' + $fixtureDatabase + '"') -WindowStyle Hidden -RedirectStandardError (Join-Path $dir.FullName 'service.log') -PassThru
                $null = $startedServiceProcess.Handle
            } else {
                $recorded = Start-RecordedService (Join-Path (Split-Path $exe) 'dzc-service.exe') ('--database "' + $fixtureDatabase + '"') $env:DZC_SERVICE_CRASH_ROOT
                $startedServiceProcess = $recorded.Process
                $serviceCrashReceipt = $recorded.Receipt
            }
            $startedService = Wait-Until {
                if ($startedServiceProcess.HasExited) { throw 'Recorded Service exited before readiness' }
                $preference = $ErrorActionPreference
                try {
                    $ErrorActionPreference = 'Continue'
                    $json = & $cli service status --json 2>$null
                    $statusExit = $LASTEXITCODE
                } finally { $ErrorActionPreference = $preference }
                if ($statusExit -eq 0) {
                    $status = $json | ConvertFrom-Json
                    if ($status.capture_owner -and $status.state.service_instance_id) { return $status }
                }
            } 'Recorded Service did not become ready'
        } else {
            $startedService = & $cli service start --json | ConvertFrom-Json
        }
        if ($LASTEXITCODE -or $startedService.mode -ne 'service' -or !$startedService.capture_owner -or !$startedService.state.service_instance_id) {
            throw 'Could not start the independent capture Service before MyGo'
        }
        $serviceEndpoint = & $cli service desktop-endpoint | ConvertFrom-Json
        if ($LASTEXITCODE -or !$serviceEndpoint.pid -or $serviceEndpoint.instance -cne $startedService.state.service_instance_id) {
            throw 'Could not identify the original capture Service process'
        }
        if (!$startedServiceProcess) { $startedServiceProcess = Get-Process -Id $serviceEndpoint.pid }
        if ($startedServiceProcess.Id -ne $serviceEndpoint.pid -or $startedServiceProcess.Path -ne (Join-Path (Split-Path $exe) 'dzc-service.exe')) {
            throw 'Original capture Service used an unexpected executable'
        }
        # Retain the kernel handle before a possible exit; a later PID lookup
        # cannot distinguish a crashed Service from a reused process ID.
        $null = $startedServiceProcess.Handle
        if ($env:DZC_SERVICE_CRASH_ROOT -and !$serviceCrashReceipt) {
            # CLI launches retain their identity after readiness. Direct diagnostic
            # launches above separately cover exits before readiness is available.
            $serviceCrashReceipt = Join-Path $env:DZC_SERVICE_CRASH_ROOT ("service-{0}.json" -f $startedServiceProcess.Id)
            [pscustomobject]@{
                pid = $startedServiceProcess.Id
                executable = $startedServiceProcess.Path
                sha256 = (Get-FileHash -LiteralPath $startedServiceProcess.Path -Algorithm SHA256).Hash
                created_at = $startedServiceProcess.StartTime.ToUniversalTime().ToString('o')
                crashed = $false
                exit = $null
            } | ConvertTo-Json | Set-Content -LiteralPath $serviceCrashReceipt -Encoding UTF8
        }
    }
    if (!$Capture) { [Windows.Forms.Clipboard]::SetText('Before ' + $marker) }
    $form.Show()
    $form.Activate()
    $null = $edit.Focus()
    $null = Wait-Until { $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Editor did not acquire foreground'
    $workArea = [Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $cursorY = $workArea.Top + 16
    if (![MyGoInput]::SetCursorPos($workArea.Right - 14, $cursorY)) { throw 'Could not set the fixture pointer' }
    $launchPath = $env:Path
    $launchBrowserArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
    $launchTiming = $env:DZC_DESKTOP_TIMING
    try {
        if ($WakeTiming -or $Capture) { $env:DZC_DESKTOP_TIMING = '1' }
        if ($Capture) {
            $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "$launchBrowserArguments --dzc-ci-inherited-marker".Trim()
            $env:Path = (($launchPath -split ';') | Where-Object {
                $_ -and !(Test-Path -LiteralPath (Join-Path $_ 'dzc.exe') -PathType Leaf)
            }) -join ';'
        }
        $launchOptions = @{}
        if ($Capture) { $launchOptions.ArgumentList = '--show' }
        $resident = Start-Process -FilePath $exe @launchOptions -RedirectStandardError (Join-Path $dir.FullName 'desktop.log') -PassThru
    } finally {
        $env:Path = $launchPath
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $launchBrowserArguments
        $env:DZC_DESKTOP_TIMING = $launchTiming
    }
    $main = Wait-Until {
        if ($resident.HasExited) { throw 'Resident exited during startup' }
        Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
            $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
        } | Select-Object -First 1
    } 'Packaged MyGo main window did not appear'
    $null = Wait-Until {
        $frame = [MyGoInput+Rect]::new()
        if (![MyGoInput]::GetWindowRect($main.MainWindowHandle, [ref]$frame)) { return $false }
        $width = $frame.right - $frame.left
        $height = $frame.bottom - $frame.top
        $x = [Math]::Max($workArea.Left, $workArea.Right - $width - $ExpectedPointerMargin)
        $y = [Math]::Max($workArea.Top, [Math]::Min($cursorY + 12, $workArea.Bottom - $height - $ExpectedPointerMargin))
        [Math]::Abs($frame.left - $x) -le 5 -and [Math]::Abs($frame.top - $y) -le 5
    } 'MyGo main window did not follow the pointer inside the Windows work area'
    $null = Wait-Until {
        $clientRect = [MyGoInput+Rect]::new()
        [MyGoInput]::GetClientRect($main.MainWindowHandle, [ref]$clientRect) -and
            [Math]::Abs($clientRect.right - 670) -le 1 -and [Math]::Abs($clientRect.bottom - 608) -le 1
    } 'MyGo did not restore the existing physical client dimensions'
    $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
    if ($FileDrag) {
        . (Join-Path $PSScriptRoot 'mygo-file-drag-windows.ps1')
        return
    }
    if ($WakeTiming) {
        function Invoke-TrayToggle([bool]$Left) {
            if ($Left) {
                if (![MyGoInput]::ClickResidentTray($resident.Id)) { throw 'Cannot invoke tray left-click callback' }
            } else {
                if (![MyGoInput]::OpenResidentTrayMenu($resident.Id)) { throw 'Cannot open tray for wake timing' }
                $null = Wait-Until { [MyGoInput]::ResidentTrayMenuVisible($resident.Id) } 'Wake timing tray menu did not open'
                $showRect = [MyGoInput+Rect]::new()
                $popup = [MyGoInput]::ResidentTrayItem($resident.Id, 0, [ref]$showRect)
                if ($popup -eq [IntPtr]::Zero) { throw 'Wake timing toggle action missing' }
                Click-WindowPoint $popup ([Drawing.Point]::new(($showRect.left+$showRect.right)/2, ($showRect.top+$showRect.bottom)/2))
            }
        }
        function Get-BackgroundReadCount {
            @((Select-String -LiteralPath (Join-Path $dir.FullName 'desktop.log') -Pattern 'Desktop snapshot: desktop.appearance.background ')).Count
        }
        $null = Get-ControlPoint $root $marker
        if ($WakeBackground) { Assert-CustomBackgroundPresent $main }
        $iteration = 0
        foreach ($mode in @('hidden', 'hidden', 'recreated', 'hidden', 'recreated')) {
            $left = (++$iteration % 2) -eq 1
            $previousPID = $main.Id
            if ($mode -eq 'hidden') {
                # Visibility, rather than focus, determines whether a tray action hides.
                if ($iteration -eq 2) { $form.Activate() }
                Invoke-TrayToggle $left
                $null = Wait-Until { $main.Refresh(); !$main.HasExited -and $main.MainWindowHandle -eq [IntPtr]::Zero } 'Tray hide did not retain the live WebView'
            } else {
                if (!$main.CloseMainWindow()) { throw 'Cannot close the main WebView for reconstruction timing' }
                $null = Wait-Until { $main.HasExited } 'Main WebView did not exit before reconstruction'
            }
            $readsBefore = Get-BackgroundReadCount
            $beforeIo = [MyGoInput+IoCounters]::new()
            if ($mode -eq 'hidden' -and ![MyGoInput]::GetProcessIoCounters($main.Handle, [ref]$beforeIo)) { throw 'Cannot read process I/O counters' }
            $clock = [Diagnostics.Stopwatch]::StartNew()
            Invoke-TrayToggle $left
            $main = Wait-Until {
                Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                    $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
                } | Select-Object -First 1
            } 'Tray wake did not show a prepared main window'
            $visibleMs = $clock.ElapsedMilliseconds
            if (($mode -eq 'hidden') -ne ($main.Id -eq $previousPID)) { throw 'Wake timing did not exercise the requested lifecycle' }
            if ($WakeBackground) { Assert-CustomBackgroundPresent $main }
            $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
            $null = Get-ControlPoint $root $marker
            $contentMs = $clock.ElapsedMilliseconds
            $afterIo = [MyGoInput+IoCounters]::new()
            if (![MyGoInput]::GetProcessIoCounters($main.Handle, [ref]$afterIo)) { throw 'Cannot read process I/O counters after wake' }
            $newReads = (Get-BackgroundReadCount) - $readsBefore
            if ($WakeBackground -and (($mode -eq 'hidden' -and $newReads -ne 0) -or ($mode -eq 'recreated' -and $newReads -lt 1))) { throw "Unexpected background retransmission count: $mode $newReads" }
            Write-Host "Tray wake timing: $mode left=$left background=$WakeBackground visible=${visibleMs}ms content=${contentMs}ms backgroundReads=$newReads readBytes=$($afterIo.ReadBytes-$beforeIo.ReadBytes) writeBytes=$($afterIo.WriteBytes-$beforeIo.WriteBytes) otherBytes=$($afterIo.OtherBytes-$beforeIo.OtherBytes)"
        }
        return
    }
    if ($DisabledPanels) {
        $quickTitle = [regex]::Unescape('DezirClip \u5feb\u901f\u7c98\u8d34')
        $screenTitle = [regex]::Unescape('DezirClip \u622a\u56fe')
        foreach ($title in @($quickTitle, $screenTitle)) {
            if ([MyGoInput]::WindowWithTitle($title) -ne [IntPtr]::Zero) { throw 'Disabled panel retained a native window' }
        }
        Click-Control $main (Get-ControlPoint $root 'Settings')
        Click-Control $main (Get-ControlPoint $root 'Tools')
        foreach ($enabled in @($true, $false, $true)) {
            foreach ($panel in @(@('Quick Paste', $quickTitle), @('Screenshot', $screenTitle))) {
                $box = Wait-Until { Get-SettingsCheckbox $root $panel[0] } 'Native panel setting unavailable'
                $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
                $toggle.Toggle()
                $null = Wait-Until {
                    if ($resident.HasExited) { throw 'Disabling the last native panel exited the resident' }
                    $exists = [MyGoInput]::WindowWithTitle($panel[1]) -ne [IntPtr]::Zero
                    $exists -eq $enabled
                } ("Native panel window lifetime differs from its setting: {0}, enabled={1}" -f $panel[0], $enabled)
            }
        }
        Write-Output 'Verified predisabled startup, destroying both panels and recreating both panels'
        return
    }
    if ($RichPaste) {
        Click-Control $main (Get-ControlPoint $root 'Search clipboard...' ([Windows.Automation.ControlType]::ComboBox))
        $search = $root.FindFirst([Windows.Automation.TreeScope]::Descendants,
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Search clipboard...'))
        $value = $search.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
        $before = $value.Current.Value
        $richRow = @'
import json, os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    row = db.execute('SELECT id,content FROM clipboard_history ORDER BY timestamp DESC,id DESC LIMIT 1').fetchone()
    print(json.dumps({'id':row[0], 'content':row[1]}))
'@ | python - | ConvertFrom-Json
        if ($LASTEXITCODE -or !$richRow) { throw 'Could not read isolated rich paste row' }
        [Windows.Forms.SendKeys]::SendWait('^+{F10}')
        $null = Wait-Until {
            (Get-Content -LiteralPath (Join-Path $dir.FullName 'desktop.log') -Raw) -match 'Paste shortcut target unavailable'
        } 'Rich paste did not reject the resident own main-window target'
        if ($value.Current.Value -cne $before) { throw 'Rich paste changed its own search field' }
        $remaining = @'
import os, sqlite3, sys
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    print(db.execute('SELECT count(*) FROM clipboard_history WHERE id=?',(int(sys.argv[1]),)).fetchone()[0])
'@ | python - $richRow.id
        if ($LASTEXITCODE -or $remaining -ne '1') { throw 'Rejected self paste deleted history' }
        $form.Activate()
        $null = $edit.Focus()
        $null = Wait-Until { $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'External rich paste editor did not acquire focus'
        [Windows.Forms.SendKeys]::SendWait('^+{F10}')
        $null = Wait-Until { $edit.Text -ceq $richRow.content } 'Rich paste did not deliver to the external editor'
        $null = Wait-Until {
            $count = @'
import os, sqlite3, sys
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    print(db.execute('SELECT count(*) FROM clipboard_history WHERE id=?',(int(sys.argv[1]),)).fetchone()[0])
'@ | python - $richRow.id
            $LASTEXITCODE -eq 0 -and $count -eq '0'
        } 'Successful external rich paste did not apply delete-after-paste'
        Write-Output 'Verified rich paste rejects its own search without deleting history and pastes into an external editor before deleting history'
        return
    }
    if ($MiddleShortcut) {
        $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $main.MainWindowHandle } 'Main window did not acquire foreground'
        $previousMainId = $main.Id
        $null = $main.Handle
        [MyGoInput]::mouse_event(0x20, 0, 0, 0, [UIntPtr]::Zero)
        [MyGoInput]::mouse_event(0x40, 0, 0, 0, [UIntPtr]::Zero)
        $handle = $main.MainWindowHandle
        $null = Wait-Until { ![MyGoInput]::IsWindowVisible($handle) } 'Middle button did not hide the focused WebView'
        if ($main.HasExited) { throw 'Middle button destroyed the focused WebView' }
        $form.Activate()
        $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Editor did not reacquire foreground'
        [MyGoInput]::mouse_event(0x20, 0, 0, 0, [UIntPtr]::Zero)
        [MyGoInput]::mouse_event(0x40, 0, 0, 0, [UIntPtr]::Zero)
        $main = Wait-Until {
            if ($resident.HasExited) { throw 'Middle-button resident exited' }
            Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                $_.Id -eq $previousMainId -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
            } | Select-Object -First 1
        } 'Middle button did not reveal the same WebView'
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        $null = Wait-Until { Get-ControlPoint $root $marker } 'Recreated WebView lost its history'
        Write-Output 'Verified native middle-button toggle preserves the WebView process'
        return
    }
    if ($Elevation) {
        $helperLog = Join-Path $dir.FullName 'elevation.log'
        $profileJson = @{ dezirclip = $env:DEZIRCLIP_DB_PATH; tiez = $env:TIEZ_DB_PATH } | ConvertTo-Json -Compress
        $profile = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($profileJson)).TrimEnd('=').Replace('+','-').Replace('/','_')
        $helper = Start-Process -FilePath $exe -ArgumentList @('--elevate-helper', $startedService.state.service_instance_id, $resident.Id, ('"' + $fixtureDatabase + '"'), $profile) -WindowStyle Hidden -PassThru -RedirectStandardError $helperLog
        $null = $helper.Handle
        $null = Wait-Until { $helper.HasExited } 'Elevated handoff helper did not finish' 60
        $helper.WaitForExit()
        if ($helper.ExitCode -ne 0) {
            Get-Content -LiteralPath $helperLog | Write-Host
            throw "Elevated handoff helper failed: $($helper.ExitCode)"
        }
        $afterElevation = & $cli service status --json | ConvertFrom-Json
        if ($LASTEXITCODE -or !$afterElevation.capture_owner -or $afterElevation.state.service_instance_id -ceq $startedService.state.service_instance_id) {
            throw 'Elevation did not replace the capture Service'
        }
        $waiting = @(Get-CimInstance Win32_Process -Filter "Name='dzc-desktop.exe'" | Where-Object {
            $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--after-elevation'
        })
        if ($waiting.Count -ne 1 -or $resident.HasExited) { throw 'Elevated desktop did not wait for its original resident' }
        $replacement = Get-Process -Id $waiting[0].ProcessId
        if ($replacement.MainWindowHandle -ne 0) { throw 'Elevated desktop opened before the previous resident exited' }
        $resident.Kill(); $resident.WaitForExit()
        if (!$main.HasExited) { $main.Kill(); $main.WaitForExit() }
        $resident = $replacement
        $main = Wait-Until {
            if ($resident.HasExited) { throw 'Replacement desktop exited before showing its window' }
            Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
            } | Select-Object -First 1
        } 'Elevated main window did not appear'
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        $null = Wait-Until { Get-ControlPoint $root $marker } 'Elevated desktop did not retain the original history'
        Write-Output 'Verified elevated Service replacement, capture preservation, parent-exit ordering and reopened MyGo history'
        if ($InstalledElevation) {
            $destination = New-Item -ItemType Directory -Path ($dir.FullName + '-moved')
            $status = & $cli service status --json | ConvertFrom-Json
            if ($LASTEXITCODE) { throw 'Could not inspect elevated installed Host' }
            $moved = & $cli service move-data $destination.FullName --from-instance $status.state.service_instance_id --epoch $status.state.privacy_epoch --json | ConvertFrom-Json
            if ($LASTEXITCODE -or $moved.recovery) { throw 'Installed profile could not migrate after elevation' }
            if (!(Test-Path -LiteralPath (Join-Path $destination.FullName 'clipboard.db'))) { throw 'Migrated database missing' }
            $endpoint = & $cli service desktop-endpoint | ConvertFrom-Json
            if ($LASTEXITCODE -or $endpoint.database.Replace('\\?\','') -ne (Join-Path $destination.FullName 'clipboard.db')) { throw 'Elevated Host did not follow the installed redirect' }
            Write-Output 'Verified installed profile migration after elevation without database overrides'
        }
        return
    }
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
    if ($PinPreview) {
        Test-PinnedPreview $root $main $resident $marker
        return
    }
    if ($RecoverService) {
      foreach ($recoveryAction in @('show', 'search', 'settings', 'cancel')) {
        $oldResident = $resident
        $endpoint = & $cli service desktop-endpoint | ConvertFrom-Json
        if ($LASTEXITCODE -or !$endpoint.pid) { throw 'Cannot identify Service before the recovery fixture' }
        $serviceProcess = Get-Process -Id $endpoint.pid
        $expectedService = Join-Path (Split-Path $exe) 'dzc-service.exe'
        if ($serviceProcess.Path -ne $expectedService) { throw 'Recovery fixture Service executable mismatch' }
        $null = $serviceProcess.Handle
        if (!$main.CloseMainWindow()) { throw 'Cannot close the idle main WebView' }
        $null = Wait-Until { $main.HasExited } 'Idle main WebView did not exit'
        # This process belongs to the isolated CI profile. End it without a
        # graceful protocol stop, then exercise the existing resident's tray.
        $serviceProcess.Kill()
        $null = Wait-Until { $serviceProcess.HasExited } 'Fixture Service did not exit'
        $exitLog = Join-Path ([IO.Path]::GetDirectoryName($env:DEZIRCLIP_DB_PATH)) 'desktop-service-exit.log'
        $null = Wait-Until {
            (Test-Path $exitLog) -and ((Get-Content $exitLog -Raw) -match ("pid={0} exit_code=0x[0-9a-f]{{8}}" -f $serviceProcess.Id))
        } 'Resident did not retain the unexpectedly terminated Service exit status'
        if ($recoveryAction -eq 'cancel') {
            if (![MyGoInput]::ClickResidentTray($resident.Id) -or ![MyGoInput]::ClickResidentTray($resident.Id)) {
                throw 'Cannot toggle twice during pending Service recovery'
            }
        } elseif ($recoveryAction -eq 'search') {
            if (![MyGoInput]::ClickResidentTray($resident.Id)) { throw 'Cannot begin tray recovery before superseding search' }
            $form.Activate()
            $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Recovery shortcut target did not acquire foreground'
            [Windows.Forms.SendKeys]::SendWait('^+{F9}')
        } else {
            if (![MyGoInput]::OpenResidentTrayMenu($resident.Id)) { throw 'Cannot open tray after Service exit' }
            $null = Wait-Until { [MyGoInput]::ResidentTrayMenuVisible($resident.Id) } 'Recovery tray menu did not open'
            $showRect = [MyGoInput+Rect]::new()
            $item = if ($recoveryAction -eq 'settings') { 1 } else { 0 }
            $popup = [MyGoInput]::ResidentTrayItem($resident.Id, $item, [ref]$showRect)
            if ($popup -eq [IntPtr]::Zero) { throw 'Recovery tray action missing' }
            Click-WindowPoint $popup ([Drawing.Point]::new(($showRect.left+$showRect.right)/2, ($showRect.top+$showRect.bottom)/2))
        }
        $resident = Wait-Until {
            Get-CimInstance Win32_Process -Filter "Name='dzc-desktop.exe'" | Where-Object {
                $_.ParentProcessId -eq $oldResident.Id -and $_.ExecutablePath -eq $exe
            } | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue } | Select-Object -First 1
        } 'Tray activation did not replace the stale native owner' 40
        $null = Wait-Until { $oldResident.HasExited } 'Stale resident did not exit during Host recovery'
        if ($recoveryAction -eq 'cancel') {
            $quiet = [Diagnostics.Stopwatch]::StartNew()
            while ($quiet.ElapsedMilliseconds -lt 1500) {
                $visible = Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                    $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
                }
                if ($visible) { throw 'Cancelled recovery resurrected the main window' }
                [Windows.Forms.Application]::DoEvents()
                Start-Sleep -Milliseconds 40
            }
            if (![MyGoInput]::ClickResidentTray($resident.Id)) { throw 'Cannot show the recovered silent resident' }
        }
        $main = Wait-Until {
            if ($resident.HasExited) { throw 'Recovered resident exited' }
            Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
            } | Select-Object -First 1
        } 'Tray activation did not recreate the main WebView' 40
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        if ($recoveryAction -eq 'settings') {
            $null = Get-ControlPoint $root 'General Settings'
        } else {
            $null = Get-ControlPoint $root $marker
            if ($recoveryAction -eq 'search') {
                $null = Wait-Until {
                    $root.FindFirst([Windows.Automation.TreeScope]::Descendants,
                        [Windows.Automation.AndCondition]::new(
                            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Search clipboard...'),
                            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::HasKeyboardFocusProperty, $true)))
                } 'Service recovery did not focus the requested search input'
            }
        }
        $replacement = & $cli service desktop-endpoint | ConvertFrom-Json
        if ($LASTEXITCODE -or $replacement.instance -eq $endpoint.instance -or $replacement.database -cne $endpoint.database) {
            throw 'Recovery did not authenticate a new Service for the same profile'
        }
        Write-Output "Verified $recoveryAction recovered after Service exit with a fresh native owner, Host identity and requested WebView destination"
        $serviceProcess.Dispose()
      }
        return
    }
    if ($Quit) {
        $null = Wait-Until { Get-ControlPoint $root $marker } 'History was not ready before tray Exit'
        $null = $resident.Handle
        $null = $main.Handle
        $endpoint = & $cli service desktop-endpoint | ConvertFrom-Json
        if ($LASTEXITCODE -or !$endpoint.pid) { throw 'Cannot identify Service before exit' }
        $serviceProcess = Get-Process -Id $endpoint.pid
        $null = $serviceProcess.Handle
        # This root is still alive and its process handle is retained. An older
        # process cannot be its child even if Windows has reused its parent's PID.
        # Idle checks below keep tracking recorded roots after their parents exit.
        $children = @(Get-DesktopProcesses @($resident.Id) -Since $resident.StartTime | ForEach-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            if ($process) { $null = $process.Handle; $process }
        })
        try {
            if (![MyGoInput]::OpenResidentTrayMenu($resident.Id)) { throw 'Cannot open the resident tray menu' }
            $null = Wait-Until { [MyGoInput]::ResidentTrayMenuVisible($resident.Id) } 'Resident tray menu did not open'
            # Click the real owner-drawn Exit item without relying on keyboard
            # foreground permission for the synthetic notification callback.
            $exitRect = [MyGoInput+Rect]::new()
            $popup = [MyGoInput]::ResidentTrayExit($resident.Id, [ref]$exitRect)
            if ($popup -eq [IntPtr]::Zero) { throw 'Cannot locate the actual tray Exit item' }
            Click-WindowPoint $popup ([Drawing.Point]::new(($exitRect.left + $exitRect.right) / 2, ($exitRect.top + $exitRect.bottom) / 2))
            $null = Wait-Until { $resident.HasExited -and $serviceProcess.HasExited -and
                @($children | Where-Object { !$_.HasExited }).Count -eq 0 } 'Tray Exit left Desktop, WebView or Service processes running'
            Write-Output "Exit codes: resident=$($resident.ExitCode), main=$($main.ExitCode), service=$($serviceProcess.ExitCode)"
            if ($resident.ExitCode -ne 0 -or $main.ExitCode -ne 0 -or $serviceProcess.ExitCode -ne 0) { throw 'Application exit was not clean' }
            $quitVerified = $true
            Write-Output 'Verified tray Exit stopped the resident, main WebView process tree and authenticated Service without fixture cleanup'
        } finally {
            Write-Output "Exit observation: resident=$($resident.HasExited), main=$($main.HasExited), service=$($serviceProcess.HasExited)"
            foreach ($process in $children) { Write-Output "Child $($process.Id) $($process.ProcessName): exited=$($process.HasExited)" }
            foreach ($process in $children) { $process.Dispose() }
            $serviceProcess.Dispose()
        }
        return
    }
    if ($Lifecycle) {
        Click-Control $main (Get-ControlPoint $root 'Settings')
        Test-EdgeDocking $main $root
        $main = Test-MainHotkeys $main $root $resident $exe $edit $form
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        Test-WinVTakeover $main $root
        return
    }
    if ($Capture) {
        Assert-CustomBackgroundPresent $main
        Test-TransparentPreview $root $main $marker
        Test-ExternalEdit $root $main $marker
        $main = Test-OcrAfterReopen $main $resident $exe $form $edit
        Assert-CustomBackgroundPresent $main
        Write-Output 'Verified custom background is present at the initial visible-window check and after WebView recreation'
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        Click-Control $main (Get-ControlPoint $root 'Settings')
        $softwareFlag = '(?:^|\s)--disable-gpu(?:\s|$)'
        $inheritedGpu = (Get-WebViewBrowser $main).CommandLine -match $softwareFlag
        $originalHost = (& $cli service status --json | ConvertFrom-Json).state.service_instance_id
        if ($LASTEXITCODE -or !$originalHost) { throw 'Cannot identify Service before GPU change' }
        foreach ($disabled in @($true, $false)) {
            if (!(Get-SettingsCheckbox $root)) { Click-Control $main (Get-ControlPoint $root 'General Settings') }
            $box = Wait-Until { Get-SettingsCheckbox $root } 'GPU setting not available'
            $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
            if (($toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::On) -eq $disabled) {
                throw 'Unexpected GPU setting before changing it'
            }
            $toggle.Toggle()
            $null = Wait-Until { $main.HasExited } 'GPU change did not close the previous WebView'
            $main = Wait-Until {
                if ($resident.HasExited) { throw 'GPU change terminated the native resident' }
                Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                    $_.Id -ne $resident.Id -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
                } | Select-Object -First 1
            } 'GPU change did not create a replacement main window'
            $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
            $browser = Get-WebViewBrowser $main
            $actual = $browser.CommandLine -match $softwareFlag
            $integrity = [MyGoInput]::IntegrityLevel($main.Handle)
            $markerSeen = $browser.CommandLine -match '(?:^|\s)--dzc-ci-inherited-marker(?:\s|$)'
            Write-Output "GPU setting=$disabled actual=$actual integrity=$integrity externalMarker=$markerSeen main=$($main.Id) browser=$($browser.ProcessId)"
            if ($integrity -ge 12288 -and $markerSeen) { throw 'Elevated WebView2 accepted the inherited override' }
            if ($actual -ne ($disabled -or $inheritedGpu)) {
                throw "WebView2 GPU argument mismatch: setting=$disabled inherited=$inheritedGpu actual=$actual main=$($main.Id)"
            }
            $currentHost = (& $cli service status --json | ConvertFrom-Json).state.service_instance_id
            if ($LASTEXITCODE -or $currentHost -cne $originalHost) { throw 'GPU change replaced the Rust Service' }
        }
        Write-Output 'Verified both GPU changes on replacement WebView2 browsers with the same native resident and Rust Service'
        Test-EdgeDocking $main $root
        $main = Test-MainHotkeys $main $root $resident $exe $edit $form
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        Click-Control $main (Get-ControlPoint $root 'Tools')
        Test-RegisteredCli $root $cli
        $point = Get-ControlPoint $root 'Full Screen Capture'
        $bounds = [Windows.Forms.Screen]::FromPoint($point).Bounds
        Click-Control $main $point
        $acknowledged = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Copied to clipboard')
        $null = Wait-Until { $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $acknowledged) } 'Full Screen Capture was not acknowledged'
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
    $clipboardSequence = [MyGoInput]::GetClipboardSequenceNumber()
    $null = Wait-Until { [MyGoInput]::CheckClipboardText($marker, $clipboardSequence, $form.Handle) } 'Clipboard remained locked after delivery' 3
    if ($Emoji) {
        $afterPaste = & $cli service status --json | ConvertFrom-Json
        if ($LASTEXITCODE -or $afterPaste.state.service_instance_id -cne $startedService.state.service_instance_id -or !$afterPaste.capture_owner) {
            throw 'Emoji paste did not preserve the original capture Service'
        }
        Write-Output 'Verified physical WebView2 emoji click pasted into the original editor using an already-running Service'
        return
    }
    $null = Wait-Until { $main.HasExited } 'Unpinned WebView2 main window did not exit'
    if ($resident.HasExited) { throw 'Native resident exited with the main window' }
    Write-Output 'Verified shared WebView2 row click, exact clipboard and editor text, restored focus, and independent native resident'
    # The file must also parse as Windows PowerShell 5.1 without a UTF-8 BOM.
    Test-ResidentPanel $resident '^+v' ([regex]::Unescape('DezirClip \u5feb\u901f\u7c98\u8d34')) $edit $form
    Test-ResidentPanel $resident '^+a' ([regex]::Unescape('DezirClip \u622a\u56fe')) $edit $form
    if ($edit.Text -cne $marker) { throw 'Cancelled native panel changed the editor' }
    $null = Wait-Until { [MyGoInput]::CheckClipboardText($marker, $clipboardSequence, $form.Handle) } 'Clipboard remained locked after panel cancellation' 3
    if ($QuickPasteInput) { Test-QuickPasteInput $resident $edit $form $marker }
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
    # A readiness failure leaves startedService null. Record the retained child's
    # natural exit independently, before any diagnostic query or active cleanup.
    try { Update-RecordedServiceExit $startedServiceProcess $serviceCrashReceipt }
    catch { Write-Host ("Service exit receipt failed: " + $_.Exception.Message) }
    Write-Host ("Editor length={0}, exact={1}, focused={2}, foreground={3}, editor window={4}" -f
        $edit.Text.Length, ($edit.Text -ceq $marker), $edit.Focused, [MyGoInput]::GetForegroundWindow(), $form.Handle)
    Write-Host ("Synthetic editor input diagnostics:`n" + $edit.InputDiagnostics())
    try {
      if ($main -and !$main.HasExited -and $root) {
        $names = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
            ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Last 40
        Write-Host ("Synthetic main-window accessibility tree: " + ($names -join ' | '))
        if ($RecoverService) {
          $focused = $root.FindAll([Windows.Automation.TreeScope]::Descendants,
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::HasKeyboardFocusProperty, $true)) |
            ForEach-Object { "$($_.Current.ControlType.ProgrammaticName): $($_.Current.Name)" }
          Write-Host ("Recovery foreground={0}, main={1}, focused controls={2}" -f [MyGoInput]::GetForegroundWindow(), $main.MainWindowHandle, ($focused -join ' | '))
        }
      }
      Write-Host ("Clipboard matches selected entry: " + ([Windows.Forms.Clipboard]::GetText() -ceq $marker))
    } catch { Write-Host ("Diagnostic query failed: " + $_.Exception.Message) }
    if ($Capture) {
        @'
import json, os, sqlite3
with sqlite3.connect(os.environ['DEZIRCLIP_DB_PATH']) as db:
    settings=dict(db.execute("SELECT key,value FROM settings WHERE key IN ('app.persistent','app.ocr_enabled')"))
    rows=db.execute("SELECT ocr_status,count(*),sum(length(COALESCE(ocr_text,''))>0) FROM clipboard_history WHERE content_type='image' GROUP BY ocr_status").fetchall()
    print('Synthetic OCR diagnostics:', json.dumps({'settings':settings,'status_counts':rows}))
'@ | python -
    }
    $log = Join-Path $dir.FullName 'desktop.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 12 | Write-Host }
    $serviceLog = Join-Path $dir.FullName 'service.log'
    if (Test-Path -LiteralPath $serviceLog) { Get-Content -LiteralPath $serviceLog -Tail 12 | Write-Host }
    if (($Emoji -or $Elevation) -and ($startedService -or $startedServiceProcess)) {
        if ($startedServiceProcess) {
            $startedServiceProcess.Refresh()
            if ($startedServiceProcess.HasExited) {
                Write-Host ("Original Service exited: pid={0}, exit={1}" -f $startedServiceProcess.Id, $startedServiceProcess.ExitCode)
                try {
                    $reported = $false
                    foreach ($event in @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000; StartTime = (Get-Date).AddMinutes(-5) } -MaxEvents 20 -ErrorAction Stop)) {
                        $fields = @{}
                        foreach ($field in ([xml]$event.ToXml()).Event.EventData.Data) { $fields[$field.Name] = $field.'#text' }
                        if ($fields['AppName'] -ne 'dzc-service.exe') { continue }
                        # Restrict diagnostics to module identity and exception location;
                        # do not publish process memory or captured clipboard contents.
                        [pscustomobject]@{
                            time = $event.TimeCreated.ToUniversalTime().ToString('o')
                            application = $fields['AppName']
                            module = $fields['ModuleName']
                            module_version = $fields['ModuleVersion']
                            exception = $fields['ExceptionCode']
                            offset = $fields['FaultingOffset']
                            process = $fields['ProcessId']
                        } | ConvertTo-Json -Compress | Write-Host
                        $reported = $true
                    }
                    if (!$reported) { Write-Host 'No matching Service fault module event was available' }
                } catch { Write-Host ("Service fault event unavailable: " + $_.Exception.Message) }
            } else {
                Write-Host ("Original Service is still running: pid={0}" -f $startedServiceProcess.Id)
            }
        }
        if ($startedService) { try {
            $observedService = & $cli service status --json | ConvertFrom-Json
            Write-Host ("Before cleanup: status exit={0}, original Service={1}, capture owner={2}" -f
                $LASTEXITCODE, ($startedService.state.service_instance_id -ceq $observedService.state.service_instance_id), $observedService.capture_owner)
        } catch { Write-Host 'Could not inspect the original Service before cleanup' } }
    }
    throw $failure
} finally {
    if ($WakeTiming -or $Capture) {
        $timingLog = Join-Path $dir.FullName 'desktop.log'
        if (Test-Path -LiteralPath $timingLog) { Get-Content -LiteralPath $timingLog | Write-Host }
    }
    try { Update-RecordedServiceExit $startedServiceProcess $serviceCrashReceipt }
    catch { Write-Host ("Service exit receipt failed before cleanup: " + $_.Exception.Message) }
    if ($resident -and !$resident.HasExited) { $resident.Kill(); $resident.WaitForExit() }
    if ($main -and !$main.HasExited) { $main.Kill(); $main.WaitForExit() }
    if (!$quitVerified) { & $cli service stop | Out-Null }
    if ($installedRedirect) {
        if ($null -ne $redirectBackup) { [IO.File]::WriteAllBytes($installedRedirect, $redirectBackup) }
        else { Remove-Item -LiteralPath $installedRedirect -ErrorAction SilentlyContinue }
    }
    if ($startedServiceProcess) {
        if ($serviceCrashReceipt -and !$startedServiceProcess.HasExited) {
            $startedServiceProcess.Kill(); $startedServiceProcess.WaitForExit()
        }
        $startedServiceProcess.Dispose()
    }
    $form.Dispose()
}
