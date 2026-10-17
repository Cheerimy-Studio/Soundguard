$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Push-Location $root
try {
    cargo +1.77.2 build --release --manifest-path src-tauri\Cargo.toml
    python scripts\patch-imports.py target\release\soundguard-client.exe
    Write-Host ""
    Write-Host "Built: $root\target\release\soundguard-client.exe"
} finally {
    Pop-Location
}
