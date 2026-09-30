//! ESP32-S3 USB HID injector firmware.
//!
//! Joins the Mastertech relay room as `role=client` and answers JSON commands from a
//! `role=master` peer. USB HID report emission is project 1 step 2; this build proves
//! the toolchain, Wi-Fi and relay round-trip and gates injection behind an arm state.

mod hid;
mod keymap;
mod payload;
mod protocol;

#[cfg(target_os = "espidf")]
mod device;
#[cfg(target_os = "espidf")]
mod usb;

#[cfg(target_os = "espidf")]
fn main() -> anyhow::Result<()> {
    device::run()
}

#[cfg(not(target_os = "espidf"))]
fn main() {}
