#requires -Version 5.1
<#
.SYNOPSIS
Stops and optionally removes the local SoundGuard client/server data.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts\uninstall-local.ps1
#>
param(
    [switch]$PurgeData,
    [switch]$RemoveDesktopCopy
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Get-Process soundguard-server, soundguard-client -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

$configs = @(
    (Join-Path $root "dist\client-config.json"),
    (Join-Path $root "client\src-tauri\target\debug\client-config.json"),
    (Join-Path $env:APPDATA "com.soundguard.client\client-config.json")
)
if ($RemoveDesktopCopy) {
    $configs += (Join-Path ([Environment]::GetFolderPath("Desktop")) "Soundguard\client-config.json")
}

foreach ($path in $configs) {
    if (Test-Path $path) {
        Remove-Item $path -Force
        Write-Host "已删除配置：$path"
    }
}

if (RemoveDesktopCopy) {
    $desktopCopy = Join-Path ([Environment]::GetFolderPath("Desktop")) "Soundguard"
    if (Test-Path $desktopCopy) {
        Remove-Item $desktopCopy -Recurse -Force
        Write-Host "已删除桌面客户端副本：$desktopCopy"
    }
}

if ($PurgeData) {
    $data = Join-Path $root "data"
    if (Test-Path $data) {
        Remove-Item $data -Recurse -Force
        Write-Host "已删除运行数据：$data"
    }
} else {
    Write-Host "运行数据保留在：$(Join-Path $root 'data')"
}

Write-Host "本地 SoundGuard 进程已停止。"
