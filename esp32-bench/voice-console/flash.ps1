# Builds and flashes the P4 voice-console, then opens the serial monitor.
# Wi-Fi creds come from VOICE_WIFI_SSID/VOICE_WIFI_PASS (set as User env vars);
# pass -Ssid/-Pass to override.
param(
    [string]$Port = "COM5",
    [string]$Ssid = $Env:VOICE_WIFI_SSID,
    [string]$Pass = $Env:VOICE_WIFI_PASS,
    [string]$DeviceId = $(if ($Env:VOICE_DEVICE_ID) { $Env:VOICE_DEVICE_ID } else { "VOICE-DEV" }),
    [string]$RelayUrl = $Env:VOICE_RELAY_URL
)

. "$HOME\export-esp.ps1"
$Env:CARGO_TARGET_DIR = "C:\vct"
$Env:CARGO_WORKSPACE_DIR = $PSScriptRoot
$Env:ESP_IDF_TOOLS_INSTALL_DIR = "global"

if (-not $Ssid) { $Ssid = Read-Host "Wi-Fi SSID" }
$Env:VOICE_WIFI_SSID = $Ssid
if (-not $Pass) {
    $secure = Read-Host "Wi-Fi password for $Ssid (blank for open)" -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try { $Pass = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
}
$Env:VOICE_WIFI_PASS = $Pass
$Env:VOICE_DEVICE_ID = $DeviceId
if ($RelayUrl) { $Env:VOICE_RELAY_URL = $RelayUrl }

Push-Location $PSScriptRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    espflash flash --port $Port --chip esp32p4 --monitor "C:\vct\riscv32imafc-esp-espidf\release\voice-console"
}
finally {
    Pop-Location
}
