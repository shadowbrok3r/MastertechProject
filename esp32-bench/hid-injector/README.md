# hid-injector — ESP32-S3 USB HID injector

Rust / `esp-idf-svc` firmware for the ESP32-S3 DevKitC-1. Presents as a USB keyboard + absolute mouse to a target PC and takes commands from Mastertech over the relay. See `../../docs/ESP32_BENCH_HARDWARE_PLAN.md` for the full plan.

Status: **step 2** — Wi-Fi + relay round-trip, arm-gated dispatch, and a TinyUSB composite **keyboard + absolute mouse**. Builds/enumerates (VID 0x303A / PID 0x4004); live keystroke/click injection is not yet verified on a target host.

## Layout

- `src/protocol.rs` — JSON request/response types. No ESP deps; host-testable.
- `src/keymap.rs` — ASCII→HID usage and chord parsing. No ESP deps; host-testable.
- `src/hid.rs` — arm state, dispatch, and the `Hid` backend trait. No ESP deps; host-testable.
- `src/usb.rs` — TinyUSB composite keyboard + absolute mouse `Hid` backend (`target_os = "espidf"`).
- `src/device.rs` — installs USB, joins Wi-Fi, holds the relay socket (`target_os = "espidf"`).
- `src/main.rs` — target-gated entry point.

## USB ports

The DevKitC-1 has two USB connectors:
- **UART (CH343 → COM port):** flashing and logs.
- **USB-OTG (GPIO19/20, "USB"):** the HID data link — plug this into the target PC. USB init runs before Wi-Fi, so the device enumerates even with no network.

## Host tests (fast, no ESP-IDF)

```bash
rustup run stable cargo test --target x86_64-pc-windows-msvc
```

`esp-idf-svc` is gated to the device target, so this compiles only the pure protocol/dispatch and runs their tests.

## Build the firmware

Toolchain (already installed on the admin box): `espup install --std --targets esp32s3,esp32p4`, plus `espflash` and `ldproxy` (`cargo binstall espup espflash ldproxy`). `rust-toolchain.toml` pins the `esp` channel.

PowerShell, from this directory:

```powershell
. C:\Users\Owner\export-esp.ps1        # LIBCLANG_PATH + esp-clang on PATH
$Env:CARGO_TARGET_DIR = "C:\hidt"      # short path; ESP-IDF build blows past MAX_PATH otherwise
$Env:CARGO_WORKSPACE_DIR = "C:\Users\Owner\Documents\WorkProjects\MastertechProject\esp32-bench\hid-injector"
$Env:ESP_IDF_TOOLS_INSTALL_DIR = "global"
cargo build --release
```

First build downloads and compiles ESP-IDF v5.3.3 (~10 min). `.cargo/config.toml` sets the `xtensa-esp32s3-espidf` target, `ldproxy` linker and `build-std`.

**`CARGO_WORKSPACE_DIR` is not optional here.** embuild derives the workspace root by walking a fixed number of levels up from `OUT_DIR`; with `CARGO_TARGET_DIR` at a drive root (`C:\hidt`) that lands on `C:\`, so esp-idf-sys reads empty `extra_components`, never fetches the remote components (`esp_websocket_client`, `esp_tinyusb`), and the build fails `unresolved import esp_idf_svc::ws::client`. Setting `CARGO_WORKSPACE_DIR` to this crate dir overrides the heuristic. (Alternatively, keep the target dir inside the crate — but that path is deep enough to hit MAX_PATH.)

**TinyUSB build notes** (all handled in `Cargo.toml` / `.cargo/config.toml` / `sdkconfig.defaults`, listed so a version bump doesn't re-break them): `esp_tinyusb` is pinned `<2.0.0` because esp-idf-sys 0.36's bindings reference 1.x-only headers; `.cargo/config.toml` sets `BINDGEN_EXTRA_CLANG_ARGS=-DCFG_TUSB_OS_INC_PATH=freertos/` so bindgen resolves TinyUSB's `FreeRTOS.h`; and `CONFIG_TINYUSB_HID_COUNT=1` compiles the HID class in (the `tud_hid_n_*` symbols are hand-declared in `usb.rs`). Changing `extra_components` needs the esp-idf-sys build re-run — delete `<CARGO_TARGET_DIR>/<triple>/release/.fingerprint/esp-idf-sys-*` and rebuild.

### Device config (compile-time env, all optional)

| Var | Default | Meaning |
|---|---|---|
| `HID_WIFI_SSID` | `""` | Wi-Fi SSID (empty ⇒ open auth, join fails) |
| `HID_WIFI_PASS` | `""` | Wi-Fi password |
| `HID_DEVICE_ID` | `HID-DEV` | relay room id; **must be unique per device** |
| `HID_RELAY_URL` | `wss://socket.master-tech.app/websocket` | relay base (local: `ws://<host>:8081/websocket`) |

Set them before `cargo build` (e.g. `$Env:HID_WIFI_SSID = "PCL2"`).

## Flash

The DevKitC-1's **UART** port (the CH343 bridge — COM6 on the admin box) flashes and monitors; the **USB-OTG** port (GPIO19/20) is the HID link to the target and is not for flashing.

`flash.ps1` does the whole build + flash + monitor with the right env, and prompts for the Wi-Fi SSID and password (password read as a SecureString, kept in-process only):

```powershell
.\flash.ps1 -Port COM6
```

Expected log after flashing: `wifi up on <ssid>, ip …`, then `relay connected`. With no SSID configured the firmware parks with `no Wi-Fi configured`.

## Smoke test (step 1)

`relay-ping.ps1` joins the device's room as `role=master` and sends `ping` + `status`:

```powershell
.\relay-ping.ps1 -DeviceId HID-DEV
```

Device online: `<< {"id":"1","ok":true,"result":{"pong":true,"armed":false}}`. Device offline: `<< NO_AGENT_IN_ROOM` per command.
