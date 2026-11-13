if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    [Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
    throw 'Native file drag requires a disposable GitHub-hosted Windows STA session'
}
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing @'
using System;
using System.Drawing;
using System.Threading;
using System.Windows.Forms;
public sealed class FileTransferFixture : Panel {
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    public string[] Source;
    public string[] Received;
    public FileTransferFixture() {
        AllowDrop = true;
        BackColor = Color.LightSteelBlue;
        Dock = DockStyle.Fill;
        DragEnter += (s, e) => { e.Effect = e.Data.GetDataPresent(DataFormats.FileDrop) ? DragDropEffects.Copy : DragDropEffects.None; };
        DragDrop += (s, e) => { Received = (string[])e.Data.GetData(DataFormats.FileDrop); };
    }
    protected override void OnMouseDown(MouseEventArgs e) {
        base.OnMouseDown(e);
        if (Source != null && e.Button == MouseButtons.Left) {
            var data = new DataObject(DataFormats.FileDrop, Source);
            DoDragDrop(data, DragDropEffects.Copy);
        }
    }
    public static void Gesture(Point from, Point to, bool cancel) {
        var thread = new Thread(() => {
            SetCursorPos(from.X, from.Y);
            Thread.Sleep(100);
            mouse_event(2, 0, 0, 0, UIntPtr.Zero);
            for (int i = 1; i <= 30; i++) {
                SetCursorPos(from.X + (to.X-from.X)*i/30, from.Y + (to.Y-from.Y)*i/30);
                Thread.Sleep(40);
            }
            Thread.Sleep(500);
            if (cancel) { keybd_event(0x1b, 0, 0, UIntPtr.Zero); keybd_event(0x1b, 0, 2, UIntPtr.Zero); }
            mouse_event(4, 0, 0, 0, UIntPtr.Zero);
        });
        thread.IsBackground = true;
        thread.Start();
    }
}
'@
$panel = [FileTransferFixture]::new()
$form.Controls.Clear()
$form.Controls.Add($panel)
$form.SetBounds(10, 60, 300, 300)
$form.Show()
$files = @((Join-Path $dir.FullName 'drag 文件 with spaces.txt'), (Join-Path $dir.FullName 'drag-image.png'))
[IO.File]::WriteAllText($files[0], 'Native copy fixture')
$bitmap = [Drawing.Bitmap]::new(24, 24)
try { $bitmap.Save($files[1], [Drawing.Imaging.ImageFormat]::Png) } finally { $bitmap.Dispose() }
$panel.Source = $files
$target = $panel.PointToScreen([Drawing.Point]::new(100,100))
$drop = Get-ControlPoint $root $marker
[FileTransferFixture]::Gesture($target, $drop, $false)
$null = Wait-Until { [Windows.Forms.Clipboard]::ContainsFileDropList() -and
    ([Windows.Forms.Clipboard]::GetFileDropList() -join '|') -eq ($files -join '|') } 'Native drop did not copy both original files'
$panel.Source = $null
$row = Get-ControlPoint $root 'drag-image.png'
[FileTransferFixture]::Gesture($row, $target, $false)
$null = Wait-Until { $panel.Received -and ($panel.Received -join '|') -eq ($files -join '|') } 'Native card drag did not attach both files'
if ([IO.File]::ReadAllText($files[0]) -ne 'Native copy fixture' -or !(Test-Path -LiteralPath $files[1])) { throw 'Native drag modified a source file' }
$panel.Received = $null
[FileTransferFixture]::Gesture($row, $target, $true)
$watch = [Diagnostics.Stopwatch]::StartNew()
while ($watch.Elapsed.TotalSeconds -lt 3) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 50 }
if ($panel.Received) { throw 'Cancelled drag unexpectedly delivered files' }
if (![MyGoInput]::IsWindowVisible($main.MainWindowHandle)) { throw 'Cancelling the native drag hid the main window' }
if (([Windows.Forms.Clipboard]::GetFileDropList() -join '|') -ne ($files -join '|')) { throw 'Dragging unexpectedly changed the clipboard' }
$bitmap = [Drawing.Bitmap]::new(240, 160)
try {
    $paint = [Drawing.Graphics]::FromImage($bitmap)
    try { $paint.Clear([Drawing.Color]::CornflowerBlue) } finally { $paint.Dispose() }
    [Windows.Forms.Clipboard]::SetImage($bitmap)
} finally { $bitmap.Dispose() }
$image = Get-ControlPoint $root 'Clipboard Image'
[FileTransferFixture]::Gesture($image, $target, $false)
$null = Wait-Until { $panel.Received -and $panel.Received.Length -eq 1 } 'Image card did not export a native file'
$bytes = [IO.File]::ReadAllBytes($panel.Received[0])
if ($bytes.Length -lt 8 -or $bytes[0] -ne 137 -or $bytes[1] -ne 80 -or $bytes[2] -ne 78 -or $bytes[3] -ne 71) { throw 'Image export was not PNG' }
if (![Windows.Forms.Clipboard]::ContainsImage()) { throw 'Dragging the image unexpectedly changed the clipboard' }
Write-Output 'Native Windows multi-file copy, Unicode paths, image attachment and cancellation passed'
