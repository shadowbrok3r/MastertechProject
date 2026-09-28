# hid-injector — ESP32-S3 USB HID injector

Rust / `esp-idf-svc` firmware for the ESP32-S3 DevKitC-1. Presents as a USB keyboard + absolute mouse to a target PC and takes commands from Mastertech over the relay. See `../../docs/ESP32_BENCH_HARDWARE_PLAN.md` for the full plan.

Status: **step 1** — Wi-Fi + relay room round-trip + arm-gated JSON command dispatch. USB HID report emission is step 2 (injection commands currently reply `usb hid backend not built yet`).

## Layout

- `src/protocol.rs` — JSON request/response types. No ESP deps; host-testable.
- `src/hid.rs` — arm state and command dispatch. No ESP deps; host-testable.
- `src/device.rs` — Wi-Fi join + relay WebSocket client (compiled only for `target_os = "espidf"`).
- `src/main.rs` — target-gated entry point.

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

**`CARGO_WORKSPACE_DIR` is not optional here.** embuild derives the workspace root by walking a fixed number of levels up from `OUT_DIR`; with `CARGO_TARGET_DIR` at a drive root (`C:\hidt`) that lands on `C:\`, so esp-idf-sys reads empty `extra_components`, never fetches `esp_websocket_client`, and the build fails `unresolved import esp_idf_svc::ws::client`. Setting `CARGO_WORKSPACE_DIR` to this crate dir overrides the heuristic. (Alternatively, keep the target dir inside the crate — but that path is deep enough to hit MAX_PATH.)

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
