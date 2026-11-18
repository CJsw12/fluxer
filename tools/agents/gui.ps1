# GUI probe for agents: window rect, screenshot, mouse, keys, clipboard (Windows only).
# Moves the real mouse/keyboard. Coordinates are physical pixels relative to the window's top-left.
# Usage (pwsh 7):
#   gui.ps1 rect       -Process <name>
#   gui.ps1 shot       -Process <name> -Out <png>
#   gui.ps1 move       -Process <name> -X 10 -Y 20   (hover; moves in small steps so mouse-move events fire)
#   gui.ps1 click      -Process <name> -X 10 -Y 20 [-Double]
#   gui.ps1 drag       -Process <name> -X 10 -Y 20 -X2 300 -Y2 20
#   gui.ps1 scroll     -Process <name> -X 10 -Y 20 -Delta -120
#   gui.ps1 keys       -Process <name> -Keys '^c'      (SendKeys syntax)
#   gui.ps1 text       -Process <name> -Text '안녕'     (pastes via clipboard + Ctrl+V)
#   gui.ps1 clipboard
param(
    [Parameter(Mandatory, Position = 0)][ValidateSet('rect', 'shot', 'move', 'click', 'drag', 'scroll', 'keys', 'text', 'clipboard')][string]$Action,
    [string]$Process,
    [int]$X, [int]$Y, [int]$X2, [int]$Y2,
    [int]$Delta = -120,
    [switch]$Double,
    [string]$Keys,
    [string]$Text,
    [string]$Out,
    [string]$PathLike  # match the exe path with wildcards, e.g. '*fluxer-ref*'
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class GuiNative {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    // Largest visible top-level window of any of the given processes (Electron/Chromium has several).
    public static IntPtr LargestWindow(uint[] pids) {
        IntPtr best = IntPtr.Zero; long bestArea = 0;
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (Array.IndexOf(pids, pid) < 0 || !IsWindowVisible(h)) return true;
            RECT r; GetWindowRect(h, out r);
            long a = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
            if (a > bestArea) { bestArea = a; best = h; }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
'@
[void][GuiNative]::SetProcessDPIAware()

function Get-Window {
    if (-not $Process) { throw '-Process is required' }
    $procs = @(Get-Process -Name $Process -ErrorAction Stop | Where-Object { -not $PathLike -or $_.Path -like $PathLike })
    $h = [GuiNative]::LargestWindow([uint32[]]($procs | ForEach-Object { $_.Id }))
    if ($h -eq [IntPtr]::Zero) { throw "no window for process '$Process'" }
    $r = New-Object GuiNative+RECT
    [void][GuiNative]::GetWindowRect($h, [ref]$r)
    [pscustomobject]@{ Handle = $h; Left = $r.Left; Top = $r.Top; Width = $r.Right - $r.Left; Height = $r.Bottom - $r.Top }
}
function Focus-Window($w) {
    # Restore, then force foreground (an Alt tap lifts the foreground lock); fail loudly if another window keeps focus.
    [void][GuiNative]::ShowWindow($w.Handle, 9)
    for ($i = 0; $i -lt 5; $i++) {
        [GuiNative]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [GuiNative]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
        [void][GuiNative]::SetForegroundWindow($w.Handle); Start-Sleep -Milliseconds 150
        if ([GuiNative]::GetForegroundWindow() -eq $w.Handle) { return }
    }
    throw "could not bring '$Process' to the foreground"
}
# Refuse to inject input when another window covers the target point (input would go there).
function Assert-Unobstructed($w, $x, $y) {
    $pt = New-Object GuiNative+POINT; $pt.X = $w.Left + $x; $pt.Y = $w.Top + $y
    $top = [GuiNative]::GetAncestor([GuiNative]::WindowFromPoint($pt), 2)
    if ($top -ne $w.Handle) { throw "point ($x,$y) of '$Process' is covered by another window; input would go there" }
}
function Move-To($w, $x, $y) { [void][GuiNative]::SetCursorPos($w.Left + $x, $w.Top + $y); Start-Sleep -Milliseconds 30 }
$LDOWN = 0x2; $LUP = 0x4; $WHEEL = 0x800

switch ($Action) {
    'rect' { Get-Window | Select-Object Left, Top, Width, Height | ConvertTo-Json -Compress }
    'shot' {
        # PrintWindow renders the window itself, so overlapping windows don't leak into the shot.
        $w = Get-Window
        if (-not $Out) { throw '-Out is required' }
        $bmp = New-Object System.Drawing.Bitmap $w.Width, $w.Height
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc(); $ok = [GuiNative]::PrintWindow($w.Handle, $hdc, 2); $g.ReleaseHdc($hdc)
        if (-not $ok) { $g.CopyFromScreen($w.Left, $w.Top, 0, 0, $bmp.Size) }
        $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
        "saved $Out ($($w.Width)x$($w.Height))"
    }
    'move' {
        $w = Get-Window; Focus-Window $w; Assert-Unobstructed $w $X $Y
        Move-To $w ($X - 10) $Y
        # Relative MOUSEEVENTF_MOVE steps generate real WM_MOUSEMOVE input, unlike SetCursorPos alone.
        for ($i = 0; $i -lt 5; $i++) { [GuiNative]::mouse_event(0x1, 2, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
    }
    'click' {
        $w = Get-Window; Focus-Window $w; Assert-Unobstructed $w $X $Y; Move-To $w $X $Y
        $n = if ($Double) { 2 } else { 1 }
        1..$n | ForEach-Object { [GuiNative]::mouse_event($LDOWN, 0, 0, 0, [UIntPtr]::Zero); [GuiNative]::mouse_event($LUP, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60 }
    }
    'drag' {
        $w = Get-Window; Focus-Window $w; Assert-Unobstructed $w $X $Y; Move-To $w $X $Y
        [GuiNative]::mouse_event($LDOWN, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80
        $steps = 15
        for ($i = 1; $i -le $steps; $i++) {
            Move-To $w ([int]($X + ($X2 - $X) * $i / $steps)) ([int]($Y + ($Y2 - $Y) * $i / $steps))
        }
        Start-Sleep -Milliseconds 80
        [GuiNative]::mouse_event($LUP, 0, 0, 0, [UIntPtr]::Zero)
    }
    'scroll' { $w = Get-Window; Focus-Window $w; Assert-Unobstructed $w $X $Y; Move-To $w $X $Y; [GuiNative]::mouse_event($WHEEL, 0, 0, $Delta, [UIntPtr]::Zero) }
    'keys' { $w = Get-Window; Focus-Window $w; [System.Windows.Forms.SendKeys]::SendWait($Keys) }
    'text' { $w = Get-Window; Focus-Window $w; Set-Clipboard -Value $Text; [System.Windows.Forms.SendKeys]::SendWait('^v') }
    'clipboard' { Get-Clipboard -Raw }
}
