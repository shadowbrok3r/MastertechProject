//! Vendor-neutral GPU sensors from the WDDM adapter performance data that Task Manager reads.

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::Wdk::Graphics::Direct3D::{
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTER_PERFDATACAPS, D3DKMT_CLOSEADAPTER,
    D3DKMT_NODE_PERFDATA, D3DKMT_NODEMETADATA, D3DKMT_OPENADAPTERFROMLUID, D3DKMT_QUERYADAPTERINFO,
    D3DKMT_QUERYSTATISTICS, D3DKMT_QUERYSTATISTICS_ADAPTER, D3DKMT_QUERYSTATISTICS_NODE,
    D3DKMT_QUERYSTATISTICS_QUERY_NODE, D3DKMT_QUERYSTATISTICS_QUERY_SEGMENT,
    D3DKMT_QUERYSTATISTICS_SEGMENT, D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid,
    D3DKMTQueryAdapterInfo, D3DKMTQueryStatistics, DXGK_ENGINE_TYPE_3D, KMTQAITYPE_ADAPTERPERFDATA,
    KMTQAITYPE_ADAPTERPERFDATA_CAPS, KMTQAITYPE_NODEMETADATA, KMTQAITYPE_NODEPERFDATA,
    KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::{LUID, NTSTATUS};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, IDXGIFactory6,
};
use windows::core::Interface;

use super::gpu::{GpuSample, GpuSource};

/// Adapter LUID as `(LowPart, HighPart)`.
pub(crate) type LuidKey = (u32, i32);

/// Adapter list refresh interval.
const REENUMERATE: Duration = Duration::from_secs(30);
/// Upper bounds on adapters, and on engines and memory segments probed per adapter.
const MAX_ADAPTERS: u32 = 16;
const MAX_NODES: u32 = 64;
const MAX_SEGMENTS: u32 = 64;

fn succeeded(status: NTSTATUS) -> bool {
    status.0 >= 0
}

/// Vendor label for a PCI vendor id; `None` for software and virtual adapters.
fn vendor_label(vendor_id: u32) -> Option<&'static str> {
    match vendor_id {
        0x1002 | 0x1022 => Some("AMD"),
        0x10DE => Some("NVIDIA"),
        0x8086 => Some("Intel"),
        0x5143 => Some("Qualcomm"),
        _ => None,
    }
}

/// Degrees from a perf-data temperature, which drivers report in deci-Celsius.
fn deci_celsius(raw: u32) -> Option<f32> {
    (raw > 0).then(|| raw as f32 / 10.0)
}

/// Throttle temperature from the caps, accepting either deci-Celsius or whole degrees.
fn throttle_celsius(raw: u32) -> Option<f32> {
    match raw {
        300..=2000 => Some(raw as f32 / 10.0),
        30..=200 => Some(raw as f32),
        _ => None,
    }
}

/// `a.b.c.d` from the user-mode driver version DXGI packs into 64 bits.
fn umd_version(raw: i64) -> String {
    let v = raw as u64;
    format!(
        "{}.{}.{}.{}",
        v >> 48,
        (v >> 32) & 0xFFFF,
        (v >> 16) & 0xFFFF,
        v & 0xFFFF
    )
}

/// Busy share of a node between two cumulative running times in 100 ns units.
fn busy_pct(previous: i64, current: i64, wall: Duration) -> Option<f32> {
    let wall = wall.as_secs_f64();
    if wall <= 0.0 || current < previous {
        return None;
    }
    let busy = (current - previous) as f64 * 1e-7;
    Some(((busy / wall) * 100.0).clamp(0.0, 100.0) as f32)
}

/// What a `D3DKMTQueryStatistics` call reads.
#[derive(Clone, Copy)]
enum Query {
    Adapter,
    Node(u32),
    Segment(u32),
}

struct Adapter {
    luid: LUID,
    handle: u32,
    vendor: &'static str,
    name: String,
    vram_mb: Option<u64>,
    driver_version: Option<String>,
    max_fan_rpm: Option<u32>,
    throttle_c: Option<f32>,
    node_count: u32,
    graphics_nodes: Vec<u32>,
    segment_count: u32,
    /// Last cumulative running time per node and when it was read.
    running: HashMap<u32, (i64, Instant)>,
}

impl Adapter {
    fn open(adapter: &IDXGIAdapter1, vendor_of: fn(u32) -> Option<&'static str>) -> Option<Self> {
        let desc = unsafe { adapter.GetDesc1() }.ok()?;
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            return None;
        }
        let vendor = vendor_of(desc.VendorId)?;
        let len = desc
            .Description
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..len])
            .trim()
            .to_string();
        if name.is_empty() {
            return None;
        }
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: desc.AdapterLuid,
            hAdapter: 0,
        };
        if !succeeded(unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }) {
            return None;
        }
        let driver_version = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .ok()
            .map(umd_version);
        let mut this = Self {
            luid: desc.AdapterLuid,
            handle: open.hAdapter,
            vendor,
            name,
            vram_mb: (desc.DedicatedVideoMemory > 0)
                .then_some(desc.DedicatedVideoMemory as u64 / (1024 * 1024)),
            driver_version,
            max_fan_rpm: None,
            throttle_c: None,
            node_count: 0,
            graphics_nodes: Vec::new(),
            segment_count: 0,
            running: HashMap::new(),
        };
        let mut caps = D3DKMT_ADAPTER_PERFDATACAPS::default();
        if this.query(KMTQAITYPE_ADAPTERPERFDATA_CAPS, &mut caps) {
            this.max_fan_rpm = (caps.MaxFanRPM > 0).then_some(caps.MaxFanRPM);
            this.throttle_c = throttle_celsius(caps.TemperatureWarning);
        }
        if let Some(stats) = this.statistics(Query::Adapter) {
            let info = unsafe { stats.QueryResult.AdapterInformation };
            this.node_count = info.NodeCount.min(MAX_NODES);
            this.segment_count = info.NbSegments.min(MAX_SEGMENTS);
        }
        this.graphics_nodes = (0..this.node_count)
            .filter(|n| this.is_graphics_node(*n))
            .collect();
        if this.graphics_nodes.is_empty() && this.node_count > 0 {
            this.graphics_nodes.push(0);
        }
        Some(this)
    }

    fn key(&self) -> LuidKey {
        (self.luid.LowPart, self.luid.HighPart)
    }

    fn query<T>(&self, kind: KMTQUERYADAPTERINFOTYPE, data: &mut T) -> bool {
        let mut info = D3DKMT_QUERYADAPTERINFO {
            hAdapter: self.handle,
            Type: kind,
            pPrivateDriverData: data as *mut T as *mut c_void,
            PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
        };
        succeeded(unsafe { D3DKMTQueryAdapterInfo(&mut info) })
    }

    fn statistics(&self, query: Query) -> Option<D3DKMT_QUERYSTATISTICS> {
        let mut stats = D3DKMT_QUERYSTATISTICS {
            AdapterLuid: self.luid,
            ..Default::default()
        };
        match query {
            Query::Adapter => stats.Type = D3DKMT_QUERYSTATISTICS_ADAPTER,
            Query::Node(id) => {
                stats.Type = D3DKMT_QUERYSTATISTICS_NODE;
                stats.Anonymous.QueryNode = D3DKMT_QUERYSTATISTICS_QUERY_NODE { NodeId: id };
            }
            Query::Segment(id) => {
                stats.Type = D3DKMT_QUERYSTATISTICS_SEGMENT;
                stats.Anonymous.QuerySegment =
                    D3DKMT_QUERYSTATISTICS_QUERY_SEGMENT { SegmentId: id };
            }
        }
        // The kernel writes `QueryResult` through this pointer.
        let status = unsafe { D3DKMTQueryStatistics((&raw mut stats).cast_const()) };
        succeeded(status).then_some(stats)
    }

    /// MiB resident in the adapter's own memory segments; aperture segments are system RAM.
    fn dedicated_used_mb(&self) -> Option<u64> {
        let resident: Vec<u64> = (0..self.segment_count)
            .filter_map(|id| self.statistics(Query::Segment(id)))
            .map(|stats| unsafe { stats.QueryResult.SegmentInformation })
            .filter(|segment| segment.Aperture == 0)
            .map(|segment| segment.BytesResident)
            .collect();
        (!resident.is_empty()).then(|| resident.iter().sum::<u64>() / (1024 * 1024))
    }

    fn is_graphics_node(&self, node: u32) -> bool {
        let mut meta = D3DKMT_NODEMETADATA {
            NodeOrdinalAndAdapterIndex: node,
            ..Default::default()
        };
        if !self.query(KMTQAITYPE_NODEMETADATA, &mut meta) {
            return false;
        }
        let engine = meta.NodeData.EngineType;
        engine == DXGK_ENGINE_TYPE_3D
    }

    /// Identity plus every sensor this adapter answered for; unanswered sensors stay `None`.
    fn sample(&mut self, now: Instant) -> GpuSample {
        let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
        let perf = self
            .query(KMTQAITYPE_ADAPTERPERFDATA, &mut perf)
            .then_some(perf);
        let mut usage_pct: Option<f32> = None;
        for node in 0..self.node_count {
            let Some(stats) = self.statistics(Query::Node(node)) else {
                continue;
            };
            let running = unsafe {
                stats
                    .QueryResult
                    .NodeInformation
                    .GlobalInformation
                    .RunningTime
            };
            if let Some((previous, at)) = self.running.insert(node, (running, now))
                && let Some(pct) = busy_pct(previous, running, now.duration_since(at))
            {
                usage_pct = Some(usage_pct.map_or(pct, |u| u.max(pct)));
            }
        }

        let gpu_clock_mhz = self
            .graphics_nodes
            .iter()
            .filter_map(|n| {
                let mut node = D3DKMT_NODE_PERFDATA {
                    NodeOrdinal: *n,
                    ..Default::default()
                };
                (self.query(KMTQAITYPE_NODEPERFDATA, &mut node) && node.Frequency > 0)
                    .then_some((node.Frequency / 1_000_000) as u32)
            })
            .max();
        let temp_c = perf.and_then(|p| deci_celsius(p.Temperature));
        let fan_rpm = perf.and_then(|p| (p.FanRPM > 0).then_some(p.FanRPM));
        let fan_pct = match (fan_rpm, self.max_fan_rpm) {
            (Some(rpm), Some(max)) => {
                Some(((rpm as f64 / max as f64) * 100.0).round().min(100.0) as u32)
            }
            _ => None,
        };
        let throttle_reasons = match (temp_c, self.throttle_c) {
            (Some(t), Some(limit)) if t >= limit => {
                vec![format!(
                    "thermal (at the driver's {limit:.0} C throttle temperature)"
                )]
            }
            _ => Vec::new(),
        };

        GpuSample {
            vendor: self.vendor.to_string(),
            name: self.name.clone(),
            temp_c,
            usage_pct,
            memory_used_mb: self.dedicated_used_mb(),
            memory_total_mb: self.vram_mb,
            power_pct: perf.and_then(|p| (p.Power > 0).then(|| p.Power as f32 / 10.0)),
            gpu_clock_mhz,
            mem_clock_mhz: perf.and_then(|p| {
                (p.MemoryFrequency > 0).then_some((p.MemoryFrequency / 1_000_000) as u32)
            }),
            fan_pct,
            fan_rpm,
            throttle_reasons,
            driver_version: self.driver_version.clone(),
            throttle_temp_c: self.throttle_c,
            source: GpuSource::Wddm,
            ..Default::default()
        }
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        let close = D3DKMT_CLOSEADAPTER {
            hAdapter: self.handle,
        };
        let _ = unsafe { D3DKMTCloseAdapter(&close) };
    }
}

/// Adapters `vendor_of` names, in high-performance order.
fn enumerate(vendor_of: fn(u32) -> Option<&'static str>) -> Vec<Adapter> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        log::debug!("[stress-kit/gpu] DXGI factory unavailable; WDDM GPU telemetry disabled");
        return Vec::new();
    };
    let by_preference = factory.cast::<IDXGIFactory6>().ok();
    (0..MAX_ADAPTERS)
        .map_while(|i| match &by_preference {
            Some(f) => unsafe {
                f.EnumAdapterByGpuPreference::<IDXGIAdapter1>(
                    i,
                    DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
                )
            }
            .ok(),
            None => unsafe { factory.EnumAdapters1(i) }.ok(),
        })
        .filter_map(|a| Adapter::open(&a, vendor_of))
        .collect()
}

/// WDDM GPU reader that keeps engine running times between ticks for usage.
#[derive(Default)]
pub(crate) struct WddmGpuMonitor {
    adapters: Vec<Adapter>,
    enumerated_at: Option<Instant>,
}

impl WddmGpuMonitor {
    pub(crate) fn sample(&mut self) -> Vec<(LuidKey, GpuSample)> {
        let now = Instant::now();
        if self
            .enumerated_at
            .is_none_or(|at| now.duration_since(at) >= REENUMERATE)
        {
            self.refresh(now);
        }
        self.adapters
            .iter_mut()
            .map(|a| (a.key(), a.sample(now)))
            .collect()
    }

    /// Re-enumerates, keeping running-time history for adapters that are still present.
    fn refresh(&mut self, now: Instant) {
        let mut previous: HashMap<LuidKey, HashMap<u32, (i64, Instant)>> = self
            .adapters
            .drain(..)
            .map(|mut a| (a.key(), std::mem::take(&mut a.running)))
            .collect();
        self.adapters = enumerate(vendor_label);
        for adapter in &mut self.adapters {
            if let Some(running) = previous.remove(&adapter.key()) {
                adapter.running = running;
            }
        }
        self.enumerated_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_and_software_vendors_are_not_gpus() {
        assert_eq!(vendor_label(0x1002), Some("AMD"));
        assert_eq!(vendor_label(0x10DE), Some("NVIDIA"));
        assert_eq!(vendor_label(0x8086), Some("Intel"));
        for virtual_vendor in [0x1414, 0x15AD, 0x80EE, 0x1AB8, 0x1234, 0x1AF4] {
            assert_eq!(vendor_label(virtual_vendor), None, "0x{virtual_vendor:04X}");
        }
    }

    #[test]
    fn a_zero_reading_is_no_sensor() {
        assert_eq!(deci_celsius(0), None);
        assert_eq!(deci_celsius(655), Some(65.5));
    }

    #[test]
    fn throttle_temperature_accepts_both_units() {
        assert_eq!(throttle_celsius(950), Some(95.0));
        assert_eq!(throttle_celsius(95), Some(95.0));
        assert_eq!(throttle_celsius(0), None);
        assert_eq!(throttle_celsius(5), None);
        assert_eq!(throttle_celsius(250), None);
    }

    #[test]
    fn driver_version_unpacks_four_fields() {
        let raw = (32i64 << 48) | (21001i64 << 16) | 9005;
        assert_eq!(umd_version(raw), "32.0.21001.9005");
    }

    #[test]
    fn busy_share_is_running_time_over_wall_time() {
        assert_eq!(busy_pct(0, 5_000_000, Duration::from_secs(1)), Some(50.0));
        assert_eq!(busy_pct(0, 30_000_000, Duration::from_secs(1)), Some(100.0));
        assert_eq!(busy_pct(10, 5, Duration::from_secs(1)), None);
        assert_eq!(busy_pct(0, 5, Duration::ZERO), None);
    }

    #[test]
    fn every_query_answers_or_declines_on_any_wddm_adapter() {
        let now = Instant::now();
        let mut adapters = enumerate(|_| Some("any"));
        for adapter in &mut adapters {
            let first = adapter.sample(now);
            let second = adapter.sample(now + Duration::from_secs(1));
            println!(
                "{}: nodes={} graphics={:?} segments={} first={first:?} second={second:?}",
                adapter.name, adapter.node_count, adapter.graphics_nodes, adapter.segment_count
            );
            assert_eq!(second.name, first.name);
            assert!(second.memory_used_mb.is_none_or(|used| used < 1 << 20));
            assert!(second.usage_pct.is_none_or(|u| (0.0..=100.0).contains(&u)));
        }
    }

    #[test]
    fn sampling_never_reports_a_software_adapter() {
        let mut monitor = WddmGpuMonitor::default();
        for (_, gpu) in monitor.sample() {
            assert!(
                !gpu.name.to_lowercase().contains("microsoft basic"),
                "{}",
                gpu.name
            );
            assert_eq!(gpu.source, GpuSource::Wddm);
        }
    }
}
