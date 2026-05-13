//! Per-adapter network rx/tx rate sample. `sysinfo::NetworkData::received` and
//! `::transmitted` return bytes since the last `Networks::refresh`.

use serde::{Deserialize, Serialize};
use sysinfo::Networks;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkRateSample {
    pub name: String,
    pub rx_mbps: f32,
    pub tx_mbps: f32,
}

pub fn sample_networks(networks: &Networks, interval_secs: f32) -> Vec<NetworkRateSample> {
    let interval = interval_secs.max(f32::EPSILON);
    networks
        .iter()
        .map(|(name, data)| {
            // bytes -> megabits = bytes * 8 / 1_000_000
            let rx_mb = (data.received() as f32 * 8.0) / 1_000_000.0;
            let tx_mb = (data.transmitted() as f32 * 8.0) / 1_000_000.0;
            NetworkRateSample {
                name: name.clone(),
                rx_mbps: rx_mb / interval,
                tx_mbps: tx_mb / interval,
            }
        })
        .collect()
}
