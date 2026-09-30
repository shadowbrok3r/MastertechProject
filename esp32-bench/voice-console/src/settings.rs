//! Console settings kept in NVS across reboots.

use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition, EspNvs};

const NAMESPACE: &str = "voice";
const VOLUME: &str = "volume";
const WAKE: &str = "wake";

pub struct Settings(EspDefaultNvs);

impl Settings {
    pub fn open(partition: EspDefaultNvsPartition) -> anyhow::Result<Self> {
        Ok(Self(EspNvs::new(partition, NAMESPACE, true)?))
    }

    /// The saved speaker volume (0-100), if any.
    pub fn volume(&self) -> Option<u8> {
        self.0.get_u8(VOLUME).ok().flatten().map(|v| v.min(100))
    }

    pub fn set_volume(&self, level: u8) {
        if let Err(e) = self.0.set_u8(VOLUME, level) {
            log::warn!("could not save the volume: {e}");
        }
    }

    /// Whether the wake word engine starts at boot; on unless turned off.
    pub fn wake(&self) -> bool {
        self.0.get_u8(WAKE).ok().flatten() != Some(0)
    }

    pub fn set_wake(&self, on: bool) {
        if let Err(e) = self.0.set_u8(WAKE, u8::from(on)) {
            log::warn!("could not save the wake word setting: {e}");
        }
    }
}
