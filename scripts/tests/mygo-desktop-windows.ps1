param([Parameter(Mandatory)][string]$Desktop, [switch]$Emoji, [switch]$Capture, [switch]$PinPreview, [switch]$Elevation, [switch]$InstalledElevation, [switch]$MiddleShortcut, [switch]$RichPaste, [switch]$DisabledPanels, [switch]$Quit, [ValidateSet(5, 16)][int]$ExpectedPointerMargin = 16)

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
    public static bool OpenResidentTrayMenu(int process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr unused) {
            uint owner; GetWindowThreadProcessId(window, out owner);
            var name = new System.Text.StringBuilder(128); GetClassName(window, name, 128);
            if (owner == process && name.ToString() == "MyGoApp") { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        // The pinned MyGo backend's notification-area right-click callback.
        return found != IntPtr.Zero && PostMessage(found, 0x8003, new IntPtr(1), new IntPtr(0x205));
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
    $null = Wait-Until { $Main.HasExited } 'New main shortcut did not close the foreground WebView'
    [Windows.Forms.SendKeys]::SendWait('^+{F9}')
    $reopened = Wait-Until {
        if ($Resident.HasExited) { throw 'Main shortcut closed the resident' }
        Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
            $_.Id -ne $Resident.Id -and $_.Path -eq $Executable -and $_.MainWindowTitle -eq 'DezirClip'
        } | Select-Object -First 1
    } 'Saved shortcut did not recreate the main WebView'
    $newRoot = [Windows.Automation.AutomationElement]::FromHandle($reopened.MainWindowHandle)
    Click-Control $reopened (Get-ControlPoint $newRoot 'Settings')
    Click-Control $reopened (Get-ControlPoint $newRoot 'Clipboard')
    Click-Control $reopened (Get-ControlPoint $newRoot 'Delete Ctrl+Shift+F9')
    $null = Wait-Until { (Get-SavedMainHotkeys) -ceq $original } 'Removing the new shortcut did not preserve original bindings'
    if (![MyGoInput]::MainTestShortcutAvailable()) { throw 'Removed main shortcut was not released at the OS level' }
    Write-Host 'Verified WebView2 shortcut editing, persistence, native registration, activation, WebView recreation and removal'
    return $reopened
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
'@ | python - $fixtureEditor
        if ($LASTEXITCODE) { throw 'Could not select the isolated external editor' }
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
    if ($Emoji -or $Elevation) {
        if ($env:DZC_SERVICE_CRASH_ROOT -and $env:DZC_SERVICE_CRASH_LAUNCH -ne 'cli') {
            $recorded = Start-RecordedService (Join-Path (Split-Path $exe) 'dzc-service.exe') ('--database "' + $fixtureDatabase + '"') $env:DZC_SERVICE_CRASH_ROOT
            $startedServiceProcess = $recorded.Process
            $serviceCrashReceipt = $recorded.Receipt
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
    [Windows.Forms.Clipboard]::SetText('Before ' + $marker)
    $form.Show()
    $form.Activate()
    $null = $edit.Focus()
    $null = Wait-Until { $edit.Focused -and [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Editor did not acquire foreground'
    $workArea = [Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $cursorY = $workArea.Top + 16
    if (![MyGoInput]::SetCursorPos($workArea.Right - 14, $cursorY)) { throw 'Could not set the fixture pointer' }
    $launchPath = $env:Path
    $launchBrowserArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
    try {
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
    if ($DisabledPanels) {
        foreach ($title in @('DezirClip 快速粘贴', 'DezirClip 截图')) {
            if ([MyGoInput]::WindowWithTitle($title) -ne [IntPtr]::Zero) { throw 'Disabled panel retained a native window' }
        }
        Click-Control $main (Get-ControlPoint $root 'Settings')
        Click-Control $main (Get-ControlPoint $root 'Tools')
        foreach ($enabled in @($true, $false, $true)) {
            foreach ($panel in @(@('Quick Paste', 'DezirClip 快速粘贴'), @('Screenshot', 'DezirClip 截图'))) {
                $box = Wait-Until { Get-SettingsCheckbox $root $panel[0] } 'Native panel setting unavailable'
                $toggle = $box.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern)
                $toggle.Toggle()
                $null = Wait-Until {
                    if ($resident.HasExited) { throw 'Disabling the last native panel exited the resident' }
                    $exists = [MyGoInput]::WindowWithTitle($panel[1]) -ne [IntPtr]::Zero
                    $exists -eq $enabled
                } 'Native panel window lifetime differs from its setting'
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
        $null = Wait-Until { $main.HasExited } 'Middle button did not close the focused WebView'
        $form.Activate()
        $null = Wait-Until { [MyGoInput]::GetForegroundWindow() -eq $form.Handle } 'Editor did not reacquire foreground'
        [MyGoInput]::mouse_event(0x20, 0, 0, 0, [UIntPtr]::Zero)
        [MyGoInput]::mouse_event(0x40, 0, 0, 0, [UIntPtr]::Zero)
        $main = Wait-Until {
            if ($resident.HasExited) { throw 'Middle-button resident exited' }
            Get-Process -Name dzc-desktop -ErrorAction SilentlyContinue | Where-Object {
                $_.Id -ne $resident.Id -and $_.Id -ne $previousMainId -and $_.Path -eq $exe -and $_.MainWindowTitle -eq 'DezirClip'
            } | Select-Object -First 1
        } 'Middle button did not recreate the WebView'
        $root = [Windows.Automation.AutomationElement]::FromHandle($main.MainWindowHandle)
        $null = Wait-Until { Get-ControlPoint $root $marker } 'Recreated WebView lost its history'
        Write-Output 'Verified native middle-button toggle and WebView recreation with the same resident'
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
    if ($Quit) {
        $endpoint = & $cli service desktop-endpoint | ConvertFrom-Json
        if ($LASTEXITCODE -or !$endpoint.pid) { throw 'Cannot identify Service before exit' }
        $serviceProcess = Get-Process -Id $endpoint.pid
        $null = $serviceProcess.Handle
        $children = @(Get-DesktopProcesses @($resident.Id) | ForEach-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            if ($process) { $null = $process.Handle; $process }
        })
        try {
            if (![MyGoInput]::OpenResidentTrayMenu($resident.Id)) { throw 'Cannot open the resident tray menu' }
            $exitLabel = [regex]::Unescape('\u9000\u51fa')
            $exitItem = Wait-Until {
                [Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Descendants,
                    [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $exitLabel))
            } 'Tray Exit item was not exposed'
            $exitItem.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
            $null = Wait-Until { $resident.HasExited -and $serviceProcess.HasExited -and
                @($children | Where-Object { !$_.HasExited }).Count -eq 0 } 'Tray Exit left Desktop, WebView or Service processes running'
            if ($resident.ExitCode -ne 0 -or $serviceProcess.ExitCode -ne 0) { throw 'Application exit was not clean' }
            Write-Output 'Verified tray Exit stopped the resident, main WebView process tree and authenticated Service without fixture cleanup'
        } finally {
            foreach ($process in $children) { $process.Dispose() }
            $serviceProcess.Dispose()
        }
        return
    }
    if ($Capture) {
        Test-TransparentPreview $root $main $marker
        Test-ExternalEdit $root $main $marker
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
        $null = Wait-Until {
            try { $image = [Windows.Forms.Clipboard]::GetImage() }
            catch [Runtime.InteropServices.ExternalException] { return $false }
            if ($image) {
                try { return $image.Width -eq $bounds.Width -and $image.Height -eq $bounds.Height }
                finally { $image.Dispose() }
            }
        } 'Full Screen Capture did not copy the selected display'
        $acknowledged = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Copied to clipboard')
        $null = Wait-Until { $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $acknowledged) } 'Full Screen Capture was not acknowledged'
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
      }
      Write-Host ("Clipboard matches selected entry: " + ([Windows.Forms.Clipboard]::GetText() -ceq $marker))
    } catch { Write-Host ("Diagnostic query failed: " + $_.Exception.Message) }
    $log = Join-Path $dir.FullName 'desktop.log'
    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 12 | Write-Host }
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
    try { Update-RecordedServiceExit $startedServiceProcess $serviceCrashReceipt }
    catch { Write-Host ("Service exit receipt failed before cleanup: " + $_.Exception.Message) }
    if ($resident -and !$resident.HasExited) { $resident.Kill(); $resident.WaitForExit() }
    if ($main -and !$main.HasExited) { $main.Kill(); $main.WaitForExit() }
    & $cli service stop | Out-Null
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
