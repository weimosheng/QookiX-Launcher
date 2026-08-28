# ============================================================================
#  sign-windows.ps1 — 为 Windows 构建产物做代码签名
#
#  代码签名是解决"杀毒软件误报 / SmartScreen 未知发布者"最有效的办法。
#  本脚本在未配置证书时是安全的 no-op（直接退出 0），不影响日常构建。
#
#  环境变量（均可选，未配置则跳过签名）：
#    QOOKIX_CERT_PFX        代码签名证书 (.pfx) 的绝对路径
#    QOOKIX_CERT_PASSWORD   证书密码（可选；无密码则证书必须装在个人存储中）
#    QOOKIX_SIGNTOOL        signtool.exe 路径（可选；默认自动查找 Windows SDK）
#    QOOKIX_TIMESTAMP_URL   RFC3161 时间戳服务器（可选；默认 DigiCert）
#
#  用法：
#    powershell -File scripts/sign-windows.ps1 -Files "a.exe" "b.dll"
# ============================================================================
param(
  [Parameter(Mandatory = $false)]
  [string[]]$Files
)

$ErrorActionPreference = 'Stop'

$cert = $env:QOOKIX_CERT_PFX
if (-not $cert -or -not (Test-Path $cert)) {
  Write-Host "sign-windows: QOOKIX_CERT_PFX 未设置或文件不存在，跳过签名"
  exit 0
}
if (-not $Files -or $Files.Count -eq 0) {
  Write-Host "sign-windows: 没有需要签名的文件，跳过"
  exit 0
}

# 定位 signtool.exe
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
  throw "sign-windows: 找不到 signtool.exe。请安装 Windows SDK 或设置 QOOKIX_SIGNTOOL。"
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
  Write-Host "sign-windows: 签名 $f"
  & $signtool @argsList $f
  if ($LASTEXITCODE -ne 0) {
    Write-Host "sign-windows: 签名失败 $f (exit $LASTEXITCODE)"
    $failed = $true
  }
}
if ($failed) { exit 1 }
Write-Host "sign-windows: 签名完成"
exit 0
