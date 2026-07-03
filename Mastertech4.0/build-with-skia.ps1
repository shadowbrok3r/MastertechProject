# Builds MasterTech with the skia-render feature (egui_skia software renderer).
# Plain `cargo run`/`cargo build -p MasterTech` do NOT need this script or its
# prerequisites -- skia is opt-in. See BUILD.md.
param(
    [string]$Profile = "release-fast",
    [string]$NinjaPath = "C:\Users\Owner\tools\ninja\ninja.exe",
    [string]$TargetDir = "C:\ct"
)

if (-not (Test-Path $NinjaPath)) {
    Write-Error "ninja not found at $NinjaPath. Download ninja-win.zip from https://github.com/ninja-build/ninja/releases, extract ninja.exe, and pass -NinjaPath if it's elsewhere."
    exit 1
}

$env:SKIA_NINJA_COMMAND = $NinjaPath
$env:CARGO_TARGET_DIR = $TargetDir

Push-Location (Join-Path $PSScriptRoot "..")
try {
    cargo build -p MasterTech --profile $Profile --features skia-render
} finally {
    Pop-Location
}
