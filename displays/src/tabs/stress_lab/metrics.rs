//! The measurements the lab can chart, and how to read each one off a run or a
//! telemetry bucket. Keeping them as enums is what lets the comparison view put
//! any measurement on either axis without a match arm per chart.

use super::data::{RunRecord, SeriesBucket};

/// A single number summarising one whole run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunMetric {
    PeakThroughput,
    AvgThroughput,
    PeakTempC,
    AvgTempC,
    MaxGpuTempC,
    MaxClockMhz,
    AvgClockMhz,
    MaxPowerW,
    MaxFanRpm,
    MaxCpuUsagePct,
    AvgCpuUsagePct,
    WheaDelta,
    TdrCount,
    TestErrors,
    MemoryErrors,
    DiskIoErrors,
    MinV12,
    DurationSecs,
}

impl RunMetric {
    pub const VALUES: [Self; 18] = [
        Self::PeakThroughput,
        Self::AvgThroughput,
        Self::PeakTempC,
        Self::AvgTempC,
        Self::MaxGpuTempC,
        Self::MaxClockMhz,
        Self::AvgClockMhz,
        Self::MaxPowerW,
        Self::MaxFanRpm,
        Self::MaxCpuUsagePct,
        Self::AvgCpuUsagePct,
        Self::WheaDelta,
        Self::TdrCount,
        Self::TestErrors,
        Self::MemoryErrors,
        Self::DiskIoErrors,
        Self::MinV12,
        Self::DurationSecs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PeakThroughput => "Peak throughput",
            Self::AvgThroughput => "Avg throughput",
            Self::PeakTempC => "Peak temp (°C)",
            Self::AvgTempC => "Avg temp (°C)",
            Self::MaxGpuTempC => "Max GPU temp (°C)",
            Self::MaxClockMhz => "Max clock (MHz)",
            Self::AvgClockMhz => "Avg clock (MHz)",
            Self::MaxPowerW => "Max power (W)",
            Self::MaxFanRpm => "Max fan (RPM)",
            Self::MaxCpuUsagePct => "Max CPU usage (%)",
            Self::AvgCpuUsagePct => "Avg CPU usage (%)",
            Self::WheaDelta => "WHEA delta",
            Self::TdrCount => "TDR count",
            Self::TestErrors => "Verify errors",
            Self::MemoryErrors => "Memory errors",
            Self::DiskIoErrors => "Disk I/O errors",
            Self::MinV12 => "Min +12V (V)",
            Self::DurationSecs => "Duration (s)",
        }
    }

    /// Whether the numbers are only comparable within one `throughput_unit`.
    pub fn is_throughput(self) -> bool {
        matches!(self, Self::PeakThroughput | Self::AvgThroughput)
    }

    /// The `summary` key this metric reads, or `None` when it is derived.
    fn summary_key(self) -> Option<&'static str> {
        Some(match self {
            Self::PeakThroughput => "peak_throughput",
            Self::AvgThroughput => "avg_throughput",
            Self::AvgTempC => "avg_temp_c",
            Self::MaxGpuTempC => "max_gpu_temp_c",
            Self::MaxClockMhz => "max_clock_mhz",
            Self::AvgClockMhz => "avg_clock_mhz",
            Self::MaxPowerW => "max_power_w",
            Self::MaxFanRpm => "max_fan_rpm",
            Self::MaxCpuUsagePct => "max_cpu_usage_pct",
            Self::AvgCpuUsagePct => "avg_cpu_usage_pct",
            Self::WheaDelta => "whea_delta_count",
            Self::TdrCount => "tdr_count",
            Self::TestErrors => "test_errors",
            Self::MemoryErrors => "memory_errors",
            Self::DiskIoErrors => "disk_io_errors",
            Self::MinV12 => "min_v12_v",
            Self::PeakTempC | Self::DurationSecs => return None,
        })
    }

    pub fn value(self, run: &RunRecord) -> Option<f64> {
        match self {
            Self::PeakTempC => run.peak_temp_c(),
            Self::DurationSecs => run.duration_actual_secs,
            other => run.summary_num(other.summary_key()?),
        }
    }
}

/// A time series read off the bucketed telemetry.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeriesMetric {
    Throughput,
    CpuTempC,
    GpuTempC,
    CpuClockMhz,
    GpuClockMhz,
    PowerW,
    GpuPowerW,
    CpuUsagePct,
    GpuUsagePct,
    MemoryUsedPct,
    WheaDelta,
}

impl SeriesMetric {
    pub const VALUES: [Self; 11] = [
        Self::Throughput,
        Self::CpuTempC,
        Self::GpuTempC,
        Self::CpuClockMhz,
        Self::GpuClockMhz,
        Self::PowerW,
        Self::GpuPowerW,
        Self::CpuUsagePct,
        Self::GpuUsagePct,
        Self::MemoryUsedPct,
        Self::WheaDelta,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Throughput => "Throughput",
            Self::CpuTempC => "CPU temp (°C)",
            Self::GpuTempC => "GPU temp (°C)",
            Self::CpuClockMhz => "CPU clock (MHz)",
            Self::GpuClockMhz => "GPU clock (MHz)",
            Self::PowerW => "Board power (W)",
            Self::GpuPowerW => "GPU power (W)",
            Self::CpuUsagePct => "CPU usage (%)",
            Self::GpuUsagePct => "GPU usage (%)",
            Self::MemoryUsedPct => "Memory used (%)",
            Self::WheaDelta => "WHEA delta",
        }
    }

    pub fn is_throughput(self) -> bool {
        matches!(self, Self::Throughput)
    }

    /// `None` when no tick in the bucket carried the value — a gap in the
    /// sampler, which must not be drawn as a zero.
    pub fn value(self, b: &SeriesBucket) -> Option<f64> {
        // The query sums; dividing here is what keeps a sensor sampled on only
        // some ticks from reading low.
        let mean = |n: u32, sum: f64| (n > 0).then(|| sum / f64::from(n));
        let peak = |n: u32, v: f64| (n > 0).then_some(v);
        match self {
            Self::Throughput => mean(b.throughput_n, b.throughput),
            Self::CpuTempC => peak(b.cpu_temp_n, b.max_cpu_temp_c),
            Self::GpuTempC => peak(b.gpu_temp_n, b.max_gpu_temp_c),
            Self::CpuClockMhz => mean(b.clock_n, b.clock_mhz),
            Self::GpuClockMhz => mean(b.gpu_clock_n, b.gpu_clock_mhz),
            Self::PowerW => mean(b.power_n, b.power_w),
            Self::GpuPowerW => mean(b.gpu_power_n, b.gpu_power_w),
            Self::CpuUsagePct => mean(b.cpu_usage_n, b.cpu_usage_pct),
            Self::GpuUsagePct => mean(b.gpu_usage_n, b.gpu_usage_pct),
            Self::MemoryUsedPct => mean(b.memory_n, b.memory_used_pct),
            Self::WheaDelta => peak(b.whea_n, b.whea_delta as f64),
        }
    }

    /// Whether any bucket in the set carries this series at all.
    pub fn present_in(self, buckets: &[SeriesBucket]) -> bool {
        buckets.iter().any(|b| self.value(b).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::RecordId;

    fn bucket() -> SeriesBucket {
        SeriesBucket {
            run: RecordId::new("stress_test_run", "x"),
            bucket: 0,
            throughput: 0.0,
            throughput_n: 0,
            max_cpu_temp_c: 0.0,
            cpu_temp_n: 0,
            max_gpu_temp_c: 0.0,
            gpu_temp_n: 0,
            clock_mhz: 0.0,
            clock_n: 0,
            gpu_clock_mhz: 0.0,
            gpu_clock_n: 0,
            power_w: 0.0,
            power_n: 0,
            gpu_power_w: 0.0,
            gpu_power_n: 0,
            cpu_usage_pct: 0.0,
            cpu_usage_n: 0,
            gpu_usage_pct: 0.0,
            gpu_usage_n: 0,
            memory_used_pct: 0.0,
            memory_n: 0,
            whea_delta: 0,
            whea_n: 0,
            ticks: 0,
        }
    }

    #[test]
    fn an_unsampled_series_reads_as_absent_not_zero() {
        let b = bucket();
        assert_eq!(SeriesMetric::PowerW.value(&b), None);
        assert!(!SeriesMetric::PowerW.present_in(std::slice::from_ref(&b)));
    }

    #[test]
    fn a_sampled_zero_is_kept() {
        let mut b = bucket();
        b.power_n = 3;
        assert_eq!(SeriesMetric::PowerW.value(&b), Some(0.0));
        assert!(SeriesMetric::PowerW.present_in(std::slice::from_ref(&b)));
    }

    #[test]
    fn an_average_divides_by_the_ticks_that_carried_a_value() {
        let mut b = bucket();
        b.ticks = 10;
        b.power_n = 4;
        b.power_w = 200.0;
        // 200/4, not 200/10: half the ticks having no sensor must not halve the reading.
        assert_eq!(SeriesMetric::PowerW.value(&b), Some(50.0));
    }

    #[test]
    fn an_unscanned_whea_counter_is_absent_rather_than_zero() {
        let mut b = bucket();
        b.ticks = 10;
        assert_eq!(SeriesMetric::WheaDelta.value(&b), None);
        b.whea_n = 2;
        assert_eq!(SeriesMetric::WheaDelta.value(&b), Some(0.0));
    }
}
