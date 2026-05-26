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

    if !info.cpu.is_empty() || info.cpu_percentage > 0.0 {
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
            temp_c: Some(card.temperature as f32),
            usage_pct: usage.map(|u| u.gpu as f32),
            memory_used_mb: usage.map(|u| u.memory_used / (1024 * 1024)),
            memory_total_mb: Some(card.memory / (1024 * 1024)),
            ..Default::default()
        });
    }

    snap
}
