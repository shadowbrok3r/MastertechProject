# Builds the firmware with Wi-Fi settings and flashes it, then opens the serial monitor.
param(
    [string]$Port = "COM6",
    [string]$Ssid = $Env:HID_WIFI_SSID,
    [string]$DeviceId = $(if ($Env:HID_DEVICE_ID) { $Env:HID_DEVICE_ID } else { "HID-DEV" }),
    [string]$RelayUrl = $Env:HID_RELAY_URL
)

. "$HOME\export-esp.ps1"
$Env:CARGO_TARGET_DIR = "C:\hidt"
$Env:CARGO_WORKSPACE_DIR = $PSScriptRoot
$Env:ESP_IDF_TOOLS_INSTALL_DIR = "global"

if (-not $Ssid) { $Ssid = Read-Host "Wi-Fi SSID" }
$Env:HID_WIFI_SSID = $Ssid
if (-not $Env:HID_WIFI_PASS) {
    $secure = Read-Host "Wi-Fi password for $Ssid (blank for open)" -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try { $Env:HID_WIFI_PASS = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
}
$Env:HID_DEVICE_ID = $DeviceId
if ($RelayUrl) { $Env:HID_RELAY_URL = $RelayUrl }

Push-Location $PSScriptRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    espflash flash --port $Port --chip esp32s3 --monitor "C:\hidt\xtensa-esp32s3-espidf\release\hid-injector"
}
finally {
    Pop-Location
    Remove-Item Env:HID_WIFI_PASS -ErrorAction SilentlyContinue
}
