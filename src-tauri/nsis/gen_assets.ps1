# Generates the dark NSIS installer assets (sidebar + header bitmaps).
# Run from the nsis/ directory. Requires .NET (System.Drawing), available on Windows.
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

function New-Sidebar {
  $w = 164; $h = 314
  $bmp = New-Object System.Drawing.Bitmap($w, $h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::FromArgb(0x16, 0x16, 0x1E))
  # subtle vertical gradient
  $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    [System.Drawing.Rectangle]::FromLTRB(0, 0, $w, $h),
    [System.Drawing.Color]::FromArgb(0x20, 0x20, 0x2C),
    [System.Drawing.Color]::FromArgb(0x12, 0x12, 0x1A),
    [System.Drawing.Drawing2D.LinearGradientMode]::Vertical)
  $g.FillRectangle($brush, 0, 0, $w, $h)
  # brand accent bar
  $brand = [System.Drawing.Color]::FromArgb(0x2D, 0xD4, 0xBF)
  $g.FillRectangle((New-Object System.Drawing.SolidBrush($brand)), 0, $h - 6, $w, 6)
  # wordmark
  $sf = New-Object System.Drawing.StringFormat
  $sf.Alignment = [System.Drawing.StringAlignment]::Center
  $titleFont = New-Object System.Drawing.Font('Segoe UI', 16, [System.Drawing.FontStyle]::Bold)
  $subFont = New-Object System.Drawing.Font('Segoe UI', 9, [System.Drawing.FontStyle]::Regular)
  $g.DrawString('QookiX', $titleFont, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF(0, 132, $w, 40)), $sf)
  $g.DrawString('Launcher', $subFont, [System.Drawing.Color]::FromArgb(160, 160, 170), (New-Object System.Drawing.RectangleF(0, 172, $w, 24)), $sf)
  $bmp.Save('sidebar.bmp', [System.Drawing.Imaging.ImageFormat]::Bmp)
  $bmp.Dispose(); $g.Dispose()
  Write-Host 'wrote sidebar.bmp'
}

function New-Header {
  $w = 150; $h = 57
  $bmp = New-Object System.Drawing.Bitmap($w, $h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::FromArgb(0x20, 0x20, 0x2C))
  $brand = [System.Drawing.Color]::FromArgb(0x2D, 0xD4, 0xBF)
  $g.FillRectangle((New-Object System.Drawing.SolidBrush($brand)), 0, 0, 4, $h)
  $f = New-Object System.Drawing.Font('Segoe UI', 12, [System.Drawing.FontStyle]::Bold)
  $g.DrawString('QookiX', $f, [System.Drawing.Brushes]::White, 10, 18)
  $bmp.Save('header.bmp', [System.Drawing.Imaging.ImageFormat]::Bmp)
  $bmp.Dispose(); $g.Dispose()
  Write-Host 'wrote header.bmp'
}

New-Sidebar
New-Header
