# Builds the per-user client installer and the Windows server installer.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$candidates = @(
    "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe",
    "C:\Program Files (x86)\NSIS\makensis.exe",
    "C:\Program Files\NSIS\makensis.exe"
)
$makensis = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $makensis) {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue
    if ($cmd) { $makensis = $cmd.Source }
}
if (-not $makensis) {
    throw "未找到 makensis.exe。请安装 NSIS 3，或使用 Tauri 自带的 NSIS。"
}
Push-Location $root
try {
    & $makensis "deploy\windows\client.nsi"
    if ($LASTEXITCODE -ne 0) { throw "客户端安装包构建失败" }
    & $makensis "deploy\windows\server.nsi"
    if ($LASTEXITCODE -ne 0) { throw "服务端安装包构建失败" }
    Write-Host "已生成 dist\soundguard-client-setup-0.2.0.exe 与 dist\soundguard-server-setup-0.2.0.exe"
} finally {
    Pop-Location
}
