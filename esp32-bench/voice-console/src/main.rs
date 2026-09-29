//! ESP32-P4 bench voice console: push-to-talk to the shop assistant over the relay room.

#![cfg_attr(not(target_os = "espidf"), allow(dead_code))]

mod endpoint;
mod viz;

#[cfg(target_os = "espidf")]
mod console;
#[cfg(target_os = "espidf")]
mod device;
#[cfg(target_os = "espidf")]
mod ffi;
#[cfg(target_os = "espidf")]
mod settings;

#[cfg(target_os = "espidf")]
fn main() -> anyhow::Result<()> {
    device::run()
}

#[cfg(not(target_os = "espidf"))]
fn main() {}
