#[cfg(feature = "native-telemetry")]
use stress_kit::telemetry::{
    CoreSample, DiskRateSample, GpuSample, MemorySample, NetworkRateSample, ProcessSample,
    TelemetrySnapshot,
};

use database::schema::SystemInformation;

#[cfg(feature = "native-telemetry")]
pub fn sysinfo_to_telemetry(info: &SystemInformation) -> TelemetrySnapshot {
    let used_pct = if info.total_memory > 0.0 {
        (info.used_memory / info.total_memory) * 100.0
    } else {
        0.0
    };

    let mut snap = TelemetrySnapshot {
        memory: MemorySample {
            total_mb: info.total_memory as u64,
            used_mb: info.used_memory as u64,
            used_pct,
            ..Default::default()
        },
        ..Default::default()
    };

    if !info.cpu_cores.is_empty() {
        for c in &info.cpu_cores {
            snap.cores.push(CoreSample {
                index: c.index,
                name: format!("cpu{}", c.index),
                brand: info.cpu.clone(),
                usage_pct: c.usage_pct,
                freq_mhz: c.freq_mhz,
                temp_c: c.temp_c,
            });
        }
    } else if !info.cpu.is_empty() || info.cpu_percentage > 0.0 {
        snap.cores.push(CoreSample {
            index: 0,
            name: "CPU".to_string(),
            brand: info.cpu.clone(),
            usage_pct: info.cpu_percentage,
            freq_mhz: info.cpu_clock.max(0.0) as u64,
            temp_c: info
                .component_temps
                .values()
                .copied()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
        });
    }

    for (label, temp) in &info.component_temps {
        snap.thermals.push(stress_kit::telemetry::ThermalReading {
            label: label.clone(),
            temp_c: *temp,
        });
    }

    for disk in &info.disks {
        snap.disks.push(DiskRateSample {
            name: if disk.mount_point.is_empty() {
                disk.device_name.clone()
            } else {
                format!("{} ({})", disk.mount_point, disk.device_name)
            },
            read_mb_per_s: 0.0,
            write_mb_per_s: 0.0,
        });
    }

    for iface in &info.network_interfaces {
        snap.networks.push(NetworkRateSample {
            name: iface.interface_name.clone(),
            rx_mbps: iface.total_received,
            tx_mbps: iface.total_transmitted,
        });
    }

    snap.processes = info
        .processes
        .iter()
        .map(|p| ProcessSample {
            pid: p.id,
            name: p.name.clone(),
            cpu_pct: p.cpu_usage,
            mem_mb: p.memory as u64,
            thread_count: None,
            parent_pid: None,
        })
        .collect();

    for (i, card) in info.gpu_info.card.iter().enumerate() {
        let usage = info.gpu_info.usage.get(i);
        snap.gpus.push(GpuSample {
            index: i,
            vendor: card.brand.clone(),
            name: card.name.clone(),
            // Zero is a missing sensor, not a reading.
            temp_c: (card.temperature > 0).then(|| card.temperature as f32),
            usage_pct: usage.map(|u| u.gpu as f32),
            memory_used_mb: usage.map(|u| u.memory_used / (1024 * 1024)),
            memory_total_mb: (card.memory > 0).then(|| card.memory / (1024 * 1024)),
            ..Default::default()
        });
    }

    // Pass WHEA / TDR counters through from the wire-level payload to
    // the local TelemetrySnapshot the hw_tables panels render from.
    // Both arrive on the client via the same shared `stress-kit`
    // telemetry agent, but the Cmd::LiveData payload doesn't carry
    // stress-kit types directly — `SystemInformation` mirrors the
    // counter pair so the schema stays stress-kit-free.
    if let Some(w) = info.whea.as_ref() {
        snap.whea = Some(stress_kit::telemetry::WheaCounters {
            delta_since_program_start: w.delta_since_program_start,
            total_retained: w.absolute_since_boot,
            ..Default::default()
        });
    }
    if let Some(t) = info.tdr.as_ref() {
        snap.tdr = Some(stress_kit::telemetry::TdrCounters {
            delta_since_program_start: t.delta_since_program_start,
            absolute_since_boot: t.absolute_since_boot,
        });
    }

    snap
}

/// Build the small static-fact `MachineInfo` panel from the live
/// `SystemInformation`. `set_machine_info` on the resource monitor was
/// previously never called, so the Home page's "Machine" collapsing
/// section always said "Machine info not available yet."
pub fn sysinfo_to_machine_info(info: &SystemInformation) -> crate::tabs::resource_monitor::MachineInfo {
    use crate::tabs::resource_monitor::{MachineDriveRow, MachineInfo};

    // RAM total comes through as MiB; round to GiB with one decimal so
    // 16384 MiB shows as "16.0 GB" instead of the noisy raw number.
    let ram_gb = if info.total_memory > 0.0 {
        format!("{:.1} GB", (info.total_memory as f64) / 1024.0)
    } else {
        "?".to_string()
    };

    let gpu_label = if info.gpu_info.card.is_empty() {
        String::new()
    } else {
        info.gpu_info
            .card
            .iter()
            .map(|c| {
                let name = c.name.trim();
                let brand = c.brand.trim();
                if !brand.is_empty() && !name.starts_with(brand) {
                    format!("{brand} {name}")
                } else {
                    name.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let drives = info
        .disks
        .iter()
        .enumerate()
        .map(|(idx, d)| MachineDriveRow {
            index: idx + 1,
            letter: if d.mount_point.is_empty() {
                d.device_name.clone()
            } else {
                d.mount_point.clone()
            },
            drive_type: d.file_system.clone(),
            space_label: format!(
                "{} / {}",
                fmt_bytes(d.available_space),
                fmt_bytes(d.total_space)
            ),
        })
        .collect();

    MachineInfo {
        hostname: if info.hostname.is_empty() {
            info.name.clone()
        } else {
            info.hostname.clone()
        },
        cpu: info.cpu.clone(),
        ram_gb,
        gpu: gpu_label,
        drives,
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let b = bytes as f64;
    if b >= TIB {
        format!("{:.1} TiB", b / TIB)
    } else if b >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
