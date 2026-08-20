Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Bitmap]::FromFile("H:\tauri_Project\QookiX-Launcher\app-icon.png")
$counts = @{}
for ($y = 0; $y -lt $img.Height; $y += 12) {
  for ($x = 0; $x -lt $img.Width; $x += 12) {
    $c = $img.GetPixel($x, $y)
    if ($c.A -gt 200) {
      $r = [math]::Round($c.R / 16) * 16
      $g = [math]::Round($c.G / 16) * 16
      $b = [math]::Round($c.B / 16) * 16
      $key = "$r,$g,$b"
      if ($counts.ContainsKey($key)) { $counts[$key]++ } else { $counts[$key] = 1 }
    }
  }
}
$img.Dispose()
$counts.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 8 | ForEach-Object { "RGB($($_.Key)) x$($_.Value)" }
