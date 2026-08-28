# ============================================================================
#  sign-windows.ps1 - Code-sign Windows build artifacts.
#
#  Code signing is the most effective way to avoid antivirus false positives
#  and the "unknown publisher" SmartScreen warning. This script is a safe
#  no-op (exits 0) when no certificate is configured.
#
#  NOTE: keep this file ASCII-only. It is executed by Windows PowerShell 5.1,
#  which reads .ps1 files without a BOM using the system ANSI code page, so
#  non-ASCII characters corrupt the parser and break the build.
#
#  Environment variables (all optional; signing is skipped if unset):
#    QOOKIX_CERT_PFX        absolute path to the code-signing cert (.pfx)
#    QOOKIX_CERT_PASSWORD   cert password (optional)
#    QOOKIX_SIGNTOOL        signtool.exe path (optional; auto-discovered)
#    QOOKIX_TIMESTAMP_URL   RFC3161 timestamp server (optional; default DigiCert)
#
#  Usage:
#    powershell -File scripts/sign-windows.ps1 -Files "a.exe" "b.dll"
# ============================================================================
param(
  [Parameter(Mandatory = $false)]
  [string[]]$Files
)

$ErrorActionPreference = 'Stop'

# Also accept the file list from stdin (one path per line). The CI pipeline
# passes paths this way to avoid Windows PowerShell 5.1's `-File` argument
# parser, which mangles quoted arguments that contain spaces (e.g.
# "QookiX Launcher_0.2.0_x64-setup.exe").
$stdinFiles = @()
if ([Console]::IsInputRedirected) {
  $stdinFiles = @(
    [Console]::In.ReadToEnd() -split "`r?`n" |
      ForEach-Object { $_.Trim() } |
      Where-Object { $_ -ne '' }
  )
}
$Files = @($Files) + $stdinFiles

$cert = $env:QOOKIX_CERT_PFX
if (-not $cert -or -not (Test-Path $cert)) {
  Write-Host "sign-windows: QOOKIX_CERT_PFX not set or file missing, skipping signing"
  exit 0
}
if ($Files.Count -eq 0) {
  Write-Host "sign-windows: no files given, skipping"
  exit 0
}

# Locate signtool.exe
$signtool = $env:QOOKIX_SIGNTOOL
if (-not $signtool) {
  $kitsRoot = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
  if (Test-Path $kitsRoot) {
    $found = Get-ChildItem -Path $kitsRoot -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
      Where-Object { $_.FullName -match '\\x64\\' } |
      Sort-Object FullName -Descending |
      Select-Object -First 1
    if ($found) { $signtool = $found.FullName }
  }
}
if (-not $signtool -or -not (Test-Path $signtool)) {
  throw "sign-windows: signtool.exe not found. Install the Windows SDK or set QOOKIX_SIGNTOOL."
}

$ts = $env:QOOKIX_TIMESTAMP_URL
if (-not $ts) { $ts = 'http://timestamp.digicert.com' }

$argsList = @('sign', '/fd', 'SHA256')
if ($env:QOOKIX_CERT_PASSWORD) {
  $argsList += @('/f', $cert, '/p', $env:QOOKIX_CERT_PASSWORD)
} else {
  $argsList += @('/f', $cert)
}
$argsList += @('/tr', $ts, '/td', 'SHA256')

$failed = $false
foreach ($f in $Files) {
  if (-not (Test-Path $f)) { continue }
  Write-Host "sign-windows: signing $f"
  & $signtool @argsList $f
  if ($LASTEXITCODE -ne 0) {
    Write-Host "sign-windows: signing failed $f (exit $LASTEXITCODE)"
    $failed = $true
  }
}
if ($failed) { exit 1 }
Write-Host "sign-windows: done"
exit 0
