# Builds the native egui client for Windows 7 (nightly win7 target + static CRT).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $target = "x86_64-win7-windows-msvc"
    cargo +nightly build -Z build-std=std,panic_abort --target $target --release --manifest-path client-native/Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw "Win7 native client build failed" }
    $exe = Join-Path $root "client-native/target/$target/release/soundguard-client-native.exe"
    python scripts/patch-win7-imports.py $exe
    if ($LASTEXITCODE -ne 0) { throw "Win7 import patch failed" }
    New-Item -ItemType Directory -Force -Path (Join-Path $root "dist") | Out-Null
    Copy-Item $exe (Join-Path $root "dist/soundguard-client-win7.exe") -Force
    Write-Host "Built: dist\soundguard-client-win7.exe"
} finally {
    Pop-Location
}
