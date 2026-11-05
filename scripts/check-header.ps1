param([switch]$DebugBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$configuration = if ($DebugBuild) { 'debug' } else { 'release' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class HeaderCheck {
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref Point p);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int width, int height, uint flags);
}
'@
[HeaderCheck]::SetProcessDPIAware() | Out-Null
$app = Start-Process -FilePath (Join-Path $projectRoot "target\$configuration\mado-mail.exe") -ArgumentList '--demo' -PassThru
try {
    for ($i = 0; $i -lt 100; $i++) {
        $app.Refresh()
        if ($app.HasExited) { throw 'Demo exited before opening its window.' }
        if ($app.MainWindowHandle -ne 0) { break }
        Start-Sleep -Milliseconds 100
    }
    $handle = $app.MainWindowHandle
    if ($handle -eq 0) { throw 'Demo window did not open.' }
    Start-Sleep -Milliseconds 800
    $scale = [HeaderCheck]::GetDpiForWindow($handle) / 96.0
    function PackedPoint([int]$x, [int]$y) { return [IntPtr](($y -shl 16) -bor ($x -band 0xffff)) }
    function Check-Hit([int]$x, [int]$y, [int]$expected, [switch]$Click) {
        $client = PackedPoint $x $y
        [HeaderCheck]::SendMessageW($handle, 0x200, [IntPtr]::Zero, $client) | Out-Null
        Start-Sleep -Milliseconds 100
        $point = New-Object HeaderCheck+Point
        $point.X = $x; $point.Y = $y
        [HeaderCheck]::ClientToScreen($handle, [ref]$point) | Out-Null
        $screen = PackedPoint $point.X $point.Y
        $hit = [HeaderCheck]::SendMessageW($handle, 0x84, [IntPtr]::Zero, $screen).ToInt32()
        if ($hit -ne $expected) { throw "Hit test at $x,$y returned $hit, expected $expected." }
        if ($Click) {
            [HeaderCheck]::SendMessageW($handle, 0xa1, [IntPtr]$hit, $screen) | Out-Null
            [HeaderCheck]::SendMessageW($handle, 0xa2, [IntPtr]$hit, $screen) | Out-Null
            Start-Sleep -Milliseconds 300
        }
    }
    function Check-Layout {
        $rect = New-Object HeaderCheck+Rect
        [HeaderCheck]::GetClientRect($handle, [ref]$rect) | Out-Null
        $script:width = $rect.Right
        $script:captionY = [int](26 * $scale)
        Check-Hit ([int](45 * $scale)) $captionY 2
        Check-Hit ([int](150 * $scale)) $captionY 1
        Check-Hit ([int]($width - 115 * $scale)) $captionY 8
        Check-Hit ([int]($width - 69 * $scale)) $captionY 9
        Check-Hit ([int]($width - 23 * $scale)) $captionY 20
    }
    Check-Layout
    & (Join-Path $PSScriptRoot 'check-window.ps1') -Demo -ProcessId $app.Id
    Copy-Item (Join-Path $projectRoot 'target\triage-window.png') (Join-Path $projectRoot 'target\header-window.png') -Force
    Check-Hit ([int]($width - 69 * $scale)) $captionY 9 -Click
    if (-not [HeaderCheck]::IsZoomed($handle)) { throw 'Maximize failed.' }
    Check-Layout
    Check-Hit ([int]($width - 69 * $scale)) $captionY 9 -Click
    if ([HeaderCheck]::IsZoomed($handle)) { throw 'Restore failed.' }
    [HeaderCheck]::SetWindowPos($handle, [IntPtr]::Zero, 0, 0, [int](1100 * $scale), [int](600 * $scale), 6) | Out-Null
    Start-Sleep -Milliseconds 300
    Check-Layout
    Check-Hit ([int]($width - 115 * $scale)) $captionY 8 -Click
    if (-not [HeaderCheck]::IsIconic($handle)) { throw 'Minimize failed.' }
    [HeaderCheck]::ShowWindow($handle, 9) | Out-Null
    Start-Sleep -Milliseconds 300
    Check-Layout
    # Target only this synthetic HWND, without moving the pointer or sending global keys.
    Check-Hit ([int](150 * $scale)) $captionY 1
    $filterPoint = PackedPoint ([int](150 * $scale)) $captionY
    [HeaderCheck]::SendMessageW($handle, 0x201, [IntPtr]1, $filterPoint) | Out-Null
    [HeaderCheck]::SendMessageW($handle, 0x202, [IntPtr]::Zero, $filterPoint) | Out-Null
    function Send-Key([int]$key) {
        [HeaderCheck]::PostMessageW($handle, 0x100, [IntPtr]$key, [IntPtr]1) | Out-Null
        [HeaderCheck]::PostMessageW($handle, 0x101, [IntPtr]$key, [IntPtr]1) | Out-Null
        Start-Sleep -Milliseconds 100
    }
    # Rules, Refresh, reader visibility, reader placement, Auto-apply, minimize, maximize.
    for ($i = 0; $i -lt 7; $i++) { Send-Key 9 }
    Send-Key 13
    if (-not [HeaderCheck]::IsZoomed($handle)) {
        & (Join-Path $PSScriptRoot 'check-window.ps1') -Demo -ProcessId $app.Id
        throw 'Keyboard maximize failed.'
    }
    Send-Key 32
    if ([HeaderCheck]::IsZoomed($handle)) { throw 'Keyboard restore failed.' }
    Check-Layout
    Check-Hit ([int](45 * $scale)) $captionY 2
    & (Join-Path $PSScriptRoot 'check-window.ps1') -Demo -ProcessId $app.Id
    Copy-Item (Join-Path $projectRoot 'target\triage-window.png') (Join-Path $projectRoot 'target\header-narrow-window.png') -Force
    Check-Hit ([int]($width - 23 * $scale)) $captionY 20 -Click
    if (-not $app.WaitForExit(5000)) { throw 'Close failed.' }
    'Header checks passed: drag region, interactive filter, caption hit tests, maximize, restore, minimum width, minimize, keyboard maximize/restore, close.'
} finally {
    $app.Refresh()
    if (-not $app.HasExited) { Stop-Process -Id $app.Id }
}
