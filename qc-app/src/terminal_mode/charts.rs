//! Live telemetry chart board for terminal mode. Ten ring-buffer series fed
//! from the shared `HwSampler`, rendered as a 2-column grid of braille line
//! charts. Mirrors the egui `charts::ChartBoard` aggregation 1:1.

use std::collections::VecDeque;
use std::time::Instant;

use mtech_tui::styling::THEME;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType},
    Frame,
};
use stress_kit::telemetry::TelemetrySnapshot;

const HISTORY_SECS: f64 = 120.0;
const MAX_SAMPLES: usize = 2048;

#[derive(Default)]
pub struct TuiChartBoard {
    started_at: Option<Instant>,
    avg_cpu_pct: Series,
    peak_cpu_pct: Series,
    avg_freq_mhz: Series,
    ram_used_pct: Series,
    page_file_used_pct: Series,
    top_disk_mb_s: Series,
    top_net_mbps: Series,
    process_count: Series,
    cpu_temp_c: Series,
    gpu_temp_c: Series,
}

impl TuiChartBoard {
    pub fn push(&mut self, snap: &TelemetrySnapshot) {
        let t = match self.started_at {
            Some(start) => start.elapsed().as_secs_f64(),
            None => {
                self.started_at = Some(Instant::now());
                0.0
            }
        };

        let n = snap.cores.len() as f32;
        let avg_pct = if n > 0.0 {
            snap.cores.iter().map(|c| c.usage_pct).sum::<f32>() / n
        } else {
            0.0
        };
        let peak_pct = snap.cores.iter().map(|c| c.usage_pct).fold(0.0_f32, f32::max);
        let avg_mhz = if n > 0.0 {
            snap.cores.iter().map(|c| c.freq_mhz as f32).sum::<f32>() / n
        } else {
            0.0
        };
        let top_disk = snap
            .disks
            .iter()
            .map(|d| d.read_mb_per_s + d.write_mb_per_s)
            .fold(0.0_f32, f32::max);
        let top_net = snap
            .networks
            .iter()
            .map(|nw| nw.rx_mbps + nw.tx_mbps)
            .fold(0.0_f32, f32::max);

        self.avg_cpu_pct.push(t, avg_pct as f64);
        self.peak_cpu_pct.push(t, peak_pct as f64);
        self.avg_freq_mhz.push(t, avg_mhz as f64);
        self.ram_used_pct.push(t, snap.memory.used_pct as f64);
        self.page_file_used_pct.push(t, snap.memory.page_file_used_pct as f64);
        self.top_disk_mb_s.push(t, top_disk as f64);
        self.top_net_mbps.push(t, top_net as f64);
        self.process_count.push(t, snap.processes.len() as f64);

        // The snapshot's own CPU pick: die first, never a bare board zone.
        if let Some(cpu_temp) = snap.cpu_package_temp_c() {
            self.cpu_temp_c.push(t, cpu_temp as f64);
        }
        let gpu_temp = snap
            .gpus
            .iter()
            .filter_map(|g| g.temp_c)
            .fold(f32::NEG_INFINITY, f32::max);
        if gpu_temp.is_finite() {
            self.gpu_temp_c.push(t, gpu_temp as f64);
        }
    }

    /// Render the chart grid into `area` as five rows of two charts.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let panels: [(&Series, &str, &str, Color, YBounds); 10] = [
            (&self.avg_cpu_pct, "Avg CPU", "%", Color::Rgb(120, 200, 255), YBounds::Pct),
            (&self.peak_cpu_pct, "Peak CPU", "%", Color::Rgb(220, 100, 100), YBounds::Pct),
            (&self.cpu_temp_c, "CPU temp", "C", Color::Rgb(230, 140, 90), YBounds::Temp),
            (&self.gpu_temp_c, "GPU temp", "C", Color::Rgb(230, 180, 90), YBounds::Temp),
            (&self.avg_freq_mhz, "Avg clock", "MHz", Color::Rgb(170, 230, 140), YBounds::Auto),
            (&self.ram_used_pct, "RAM used", "%", Color::Rgb(220, 170, 90), YBounds::Pct),
            (&self.page_file_used_pct, "Page file", "%", Color::Rgb(200, 120, 220), YBounds::Pct),
            (&self.top_disk_mb_s, "Top disk", "MB/s", Color::Rgb(140, 200, 200), YBounds::Auto),
            (&self.top_net_mbps, "Top net", "Mbps", Color::Rgb(200, 200, 130), YBounds::Auto),
            (&self.process_count, "Processes", "", Color::Rgb(180, 180, 220), YBounds::Auto),
        ];

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(1, 5); 5])
            .split(area);

        for (r, row) in rows.iter().enumerate() {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2); 2])
                .split(*row);
            for c in 0..2 {
                let (series, title, unit, color, bounds) = panels[r * 2 + c];
                series.render(f, cols[c], title, unit, color, bounds);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum YBounds {
    Pct,
    Temp,
    Auto,
}

#[derive(Default)]
struct Series {
    samples: VecDeque<(f64, f64)>,
}

impl Series {
    fn push(&mut self, t: f64, y: f64) {
        let cutoff = t - HISTORY_SECS;
        while let Some(&(ts, _)) = self.samples.front() {
            if ts < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        while self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((t, y));
    }

    fn render(&self, f: &mut Frame, area: Rect, title: &str, unit: &str, color: Color, bounds: YBounds) {
        let latest = self.samples.back().map(|(_, y)| *y).unwrap_or(0.0);
        let header = if unit.is_empty() {
            format!("{title}  {latest:.0}")
        } else {
            format!("{title}  {latest:.1} {unit}")
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title(Span::styled(header, Style::default().fg(color)));

        if self.samples.is_empty() {
            f.render_widget(block, area);
            return;
        }

        let points: Vec<(f64, f64)> = self.samples.iter().copied().collect();
        let t_max = points.last().map(|(t, _)| *t).unwrap_or(0.0);
        let t_min = (t_max - HISTORY_SECS).max(points.first().map(|(t, _)| *t).unwrap_or(0.0));

        let (y_lo, y_hi) = match bounds {
            YBounds::Pct => (0.0, 100.0),
            YBounds::Temp => (20.0, 100.0),
            YBounds::Auto => {
                let lo = points.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let hi = points.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
                let pad = ((hi - lo) * 0.1).max(1.0);
                (lo - pad, hi + pad)
            }
        };

        let dataset = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(color))
            .data(&points);

        let chart = Chart::new(vec![dataset])
            .block(block)
            .x_axis(Axis::default().bounds([t_min, t_max.max(t_min + 1.0)]))
            .y_axis(Axis::default().bounds([y_lo, y_hi]));
        f.render_widget(chart, area);
    }
}
