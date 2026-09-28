//! TinyUSB composite HID device: a keyboard (report 1) and an absolute mouse (report 2).

use std::ffi::c_char;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use esp_idf_svc::sys;

use crate::hid::Hid;
use crate::keymap::{ascii_to_key, parse_chord};
use crate::protocol::Button;

const REPORT_ID_KBD: u8 = 1;
const REPORT_ID_MOUSE: u8 = 2;
const ABS_MAX: i32 = 32767;

// TinyUSB device-class functions for HID instance 0; static-inline wrappers are not linkable.
extern "C" {
    fn tud_hid_n_ready(instance: u8) -> bool;
    fn tud_hid_n_report(instance: u8, report_id: u8, report: *const u8, len: u16) -> bool;
    fn tud_hid_n_keyboard_report(
        instance: u8,
        report_id: u8,
        modifier: u8,
        keycode: *const u8,
    ) -> bool;
}

// Keyboard (report 1) then absolute mouse (report 2), one collection each.
static HID_REPORT_DESC: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, REPORT_ID_KBD,
    0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02,
    0x95, 0x01, 0x75, 0x08, 0x81, 0x03,
    0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0xFF, 0x05, 0x07, 0x19, 0x00, 0x29, 0xFF, 0x81, 0x00,
    0xC0,
    0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, REPORT_ID_MOUSE, 0x09, 0x01, 0xA1, 0x00,
    0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02,
    0x95, 0x01, 0x75, 0x05, 0x81, 0x03,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x16, 0x00, 0x00, 0x26, 0xFF, 0x7F, 0x75, 0x10, 0x95, 0x02,
    0x81, 0x02,
    0xC0, 0xC0,
];

const CONFIG_TOTAL_LEN: usize = 9 + 9 + 9 + 7;

// Config: one HID interface, one interrupt IN endpoint (0x81).
static CONFIG_DESC: &[u8] = &[
    0x09, 0x02, (CONFIG_TOTAL_LEN & 0xFF) as u8, (CONFIG_TOTAL_LEN >> 8) as u8, 0x01, 0x01, 0x00, 0xA0, 0x32,
    0x09, 0x04, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00,
    0x09, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, (HID_REPORT_DESC.len() & 0xFF) as u8, (HID_REPORT_DESC.len() >> 8) as u8,
    0x07, 0x05, 0x81, 0x03, 0x10, 0x00, 0x05,
];

static LANGID: [u8; 2] = [0x09, 0x04];

/// Emits reports over TinyUSB. Tracks button state and last cursor position for clicks.
pub struct UsbHid {
    buttons: u8,
    last_x: u16,
    last_y: u16,
}

impl UsbHid {
    pub fn new() -> Result<Self> {
        let strings: Vec<*const c_char> = vec![
            LANGID.as_ptr() as *const c_char,
            c"PC Laptops".as_ptr(),
            c"Mastertech HID Injector".as_ptr(),
            c"MTECH-HID-1".as_ptr(),
        ];
        let strings = Box::leak(strings.into_boxed_slice());

        let mut cfg: sys::tinyusb_config_t = unsafe { core::mem::zeroed() };
        cfg.string_descriptor = strings.as_mut_ptr();
        cfg.string_descriptor_count = strings.len() as i32;
        cfg.self_powered = false;
        cfg.__bindgen_anon_1.device_descriptor = core::ptr::null();
        cfg.__bindgen_anon_2.__bindgen_anon_1.configuration_descriptor = CONFIG_DESC.as_ptr();

        let err = unsafe { sys::tinyusb_driver_install(&cfg) };
        if err != sys::ESP_OK {
            return Err(anyhow!("tinyusb_driver_install failed: {err}"));
        }
        Ok(Self { buttons: 0, last_x: 0, last_y: 0 })
    }

    fn wait_ready(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(1000);
        while Instant::now() < deadline {
            if unsafe { tud_hid_n_ready(0) } {
                return Ok(());
            }
            sleep(Duration::from_millis(2));
        }
        Err(anyhow!("usb hid endpoint not ready"))
    }

    fn key_report(&self, modifier: u8, usage: u8) -> Result<()> {
        let keys = [usage, 0, 0, 0, 0, 0];
        self.wait_ready()?;
        unsafe { tud_hid_n_keyboard_report(0, REPORT_ID_KBD, modifier, keys.as_ptr()) };
        sleep(Duration::from_millis(6));
        self.wait_ready()?;
        unsafe { tud_hid_n_keyboard_report(0, REPORT_ID_KBD, 0, [0u8; 6].as_ptr()) };
        sleep(Duration::from_millis(6));
        Ok(())
    }

    fn mouse_report(&self) -> Result<()> {
        let [xl, xh] = self.last_x.to_le_bytes();
        let [yl, yh] = self.last_y.to_le_bytes();
        let report = [self.buttons, xl, xh, yl, yh, 0];
        self.wait_ready()?;
        unsafe { tud_hid_n_report(0, REPORT_ID_MOUSE, report.as_ptr(), report.len() as u16) };
        sleep(Duration::from_millis(6));
        Ok(())
    }
}

impl Hid for UsbHid {
    fn type_text(&mut self, text: &str) -> Result<()> {
        for c in text.chars() {
            let Some((modifier, usage)) = ascii_to_key(c) else { continue };
            self.key_report(modifier, usage)?;
        }
        Ok(())
    }

    fn key(&mut self, chord: &str) -> Result<()> {
        let (modifier, usage) = parse_chord(chord).ok_or_else(|| anyhow!("unknown key chord: {chord}"))?;
        self.key_report(modifier, usage)
    }

    fn mouse_move(&mut self, x: i32, y: i32) -> Result<()> {
        self.last_x = x.clamp(0, ABS_MAX) as u16;
        self.last_y = y.clamp(0, ABS_MAX) as u16;
        self.mouse_report()
    }

    fn click(&mut self, button: Button) -> Result<()> {
        let bit = match button {
            Button::Left => 0x01,
            Button::Right => 0x02,
            Button::Middle => 0x04,
        };
        self.buttons |= bit;
        self.mouse_report()?;
        self.buttons &= !bit;
        self.mouse_report()
    }

    fn release_all(&mut self) {
        self.buttons = 0;
        let _ = self.wait_ready();
        unsafe { tud_hid_n_keyboard_report(0, REPORT_ID_KBD, 0, [0u8; 6].as_ptr()) };
        let _ = self.mouse_report();
    }

    fn delay(&mut self, ms: u32) {
        sleep(Duration::from_millis(ms as u64));
    }

    fn ready(&self) -> bool {
        unsafe { sys::tud_mounted() && tud_hid_n_ready(0) }
    }
}

/// TinyUSB requests the HID report descriptor for an instance.
#[no_mangle]
pub extern "C" fn tud_hid_descriptor_report_cb(_instance: u8) -> *const u8 {
    HID_REPORT_DESC.as_ptr()
}

/// No feature reports are served.
#[no_mangle]
pub extern "C" fn tud_hid_get_report_cb(
    _instance: u8,
    _report_id: u8,
    _report_type: u8,
    _buffer: *mut u8,
    _reqlen: u16,
) -> u16 {
    0
}

/// Host output reports (e.g. keyboard LEDs) are ignored.
#[no_mangle]
pub extern "C" fn tud_hid_set_report_cb(
    _instance: u8,
    _report_id: u8,
    _report_type: u8,
    _buffer: *const u8,
    _bufsize: u16,
) {
}
