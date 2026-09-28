# voice-console — ESP32-P4 bench voice + touch console

Rust / `esp-idf-svc` firmware for the Waveshare ESP32-P4 WIFI6 Touch LCD (720×720). See `../../docs/ESP32_BENCH_HARDWARE_PLAN.md`.

Status: **milestone 1 scaffold** — Wi-Fi (via the onboard ESP32-C6) + relay round-trip, reusing the same relay transport as `hid-injector`. Audio (ES8311 I2S mic/speaker) and the touch UI come after Wi-Fi is proven. Board: esp32p4 rev v1.3, 32 MB flash, CH343 UART on COM5.

## Blocking: C6-hosted Wi-Fi config

The P4 has no radio; Wi-Fi runs through the onboard ESP32-C6 over `esp_hosted` (the `espressif/esp_wifi_remote` component, already in `Cargo.toml`). `EspWifi` will not init until `sdkconfig.defaults` carries the board's hosted transport (SDIO vs SPI) and its GPIOs. Those are specific to this Waveshare board and are **not** in esp-claw (which only has the P4-NANO). Fill from the board schematic / Waveshare BSP before the first real build. The C6 must also be running the `esp_hosted` slave firmware (Waveshare normally ships it pre-flashed).

## Build (once C6 config is in)

```powershell
. C:\Users\Owner\export-esp.ps1
$Env:CARGO_TARGET_DIR = "C:\vct"          # short path (MAX_PATH)
$Env:CARGO_WORKSPACE_DIR = "C:\Users\Owner\Documents\WorkProjects\MastertechProject\esp32-bench\voice-console"
$Env:ESP_IDF_TOOLS_INSTALL_DIR = "global"
$Env:VOICE_WIFI_SSID = "PCL2"; $Env:VOICE_WIFI_PASS = "..."
cargo build --release
espflash flash --port COM5 --chip esp32p4 --monitor C:\vct\riscv32imafc-esp-espidf\release\voice-console
```

Target `riscv32imafc-esp-espidf`, ESP-IDF v5.3.3 (shared with `hid-injector`). Same `CARGO_WORKSPACE_DIR` requirement as hid-injector (see its README for why).
