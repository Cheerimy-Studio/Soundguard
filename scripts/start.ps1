# Starts the release server and Tauri client built with `npm run build`.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$server = Join-Path $root "dist\soundguard-server.exe"
$client = Join-Path $root "dist\soundguard-client.exe"

if (-not (Test-Path $server) -or -not (Test-Path $client)) {
    Write-Error "Release binaries are missing. Run 'npm run build' from the repository root first."
}

if (-not (Test-Path (Join-Path $root ".env"))) {
    Write-Error "Missing .env. Copy .env.example to .env and change ADMIN_PASSWORD, DEVICE_TOKEN, STOP_PIN and JWT_SECRET."
}

Start-Process -FilePath $server -WorkingDirectory $root
Start-Sleep -Seconds 1
Start-Process -FilePath $client -WorkingDirectory $root
Write-Host "SoundGuard server and client started."
Write-Host "Dashboard: http://127.0.0.1:8080"
