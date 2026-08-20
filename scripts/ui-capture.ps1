Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$exe = "H:\tauri_Project\QookiX-Launcher\src-tauri\target\release\qookix-launcher.exe"
$p = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 8
$proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
if (-not $proc -or $proc.MainWindowHandle -eq 0) {
    Start-Sleep -Seconds 4
    $proc = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
}
"MainWindowHandle: $($proc.MainWindowHandle)"
$hwnd = $proc.MainWindowHandle
if ($hwnd -ne 0) {
    $rect = New-Object Win32+RECT
    [Win32]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    "Window: ${w}x${h} at ($($rect.Left),$($rect.Top))"
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [Win32]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    $bmp.Save("H:\tauri_Project\QookiX-Launcher\scripts\ui-shot.png", [System.Drawing.Imaging.ImageFormat]::Png)
    # sample pixels: corners + center strip
    $samples = @(
        @(5, 5), @($w/2, 5), @($w - 5, 5),          # titlebar row
        @(30, 120), @(100, 200), @($w/2, $h/2),      # body
        @($w - 40, 200), @($w/2, $h - 20)            # right body / bottom
    )
    foreach ($s in $samples) {
        $x = [int]$s[0]; $y = [int]$s[1]
        $c = $bmp.GetPixel($x, $y)
        "pixel($x,$y) = R$($c.R) G$($c.G) B$($c.B) A$($c.A)"
    }
    $bmp.Dispose()
} else {
    "NO WINDOW HANDLE - app may have failed"
}
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; "stopped" }
