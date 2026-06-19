//! Run report overlay: renders a `stress_runner::RunReportModel` for the
//! just-finished run or any past run on this machine. Self-contained modal
//! painted on top of the active tab; mirrors the egui `report_view`.

use std::sync::{Arc, Mutex};

use crossbeam::channel::{unbounded, Receiver, Sender};
use mtech_tui::styling::{APP_BACKGROUND, THEME};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph, Row, Table, Wrap,
    },
    Frame,
};
use stress_runner::{RecordId, ReportSeries, RunReportModel, StressTestRun};

use crate::terminal_mode::context::QcContext;

enum ViewMsg {
    Model(Box<RunReportModel>),
    Error(String),
    Runs(Vec<StressTestRun>),
}

pub struct ReportModal {
    open: bool,
    loading: bool,
    error: Option<String>,
    model: Option<RunReportModel>,
    runs: Vec<StressTestRun>,
    picker_sel: usize,
    scroll: u16,
    tx: Sender<ViewMsg>,
    rx: Receiver<ViewMsg>,
    ctx: Arc<Mutex<QcContext>>,
}

impl ReportModal {
    pub fn new(ctx: Arc<Mutex<QcContext>>) -> Self {
        let (tx, rx) = unbounded();
        Self {
            open: false,
            loading: false,
            error: None,
            model: None,
            runs: Vec::new(),
            picker_sel: 0,
            scroll: 0,
            tx,
            rx,
            ctx,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the overlay and load one run's report, plus the past-run picker.
    pub fn open_run(&mut self, run_id: RecordId) {
        self.open = true;
        self.loading = true;
        self.error = None;
        self.scroll = 0;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let msg = match stress_runner::fetch_report_data(&run_id).await {
                Ok(data) => ViewMsg::Model(Box::new(RunReportModel::from_data(&data))),
                Err(e) => ViewMsg::Error(format!("report fetch failed: {e:#}")),
            };
            let _ = tx.send(msg);
        });
        self.refresh_runs();
    }

    fn refresh_runs(&mut self) {
        let Some(computer) = self.ctx.lock().ok().and_then(|c| c.computer.clone()) else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let msg = match StressTestRun::list_for_computer(&computer).await {
                Ok(runs) => ViewMsg::Runs(runs),
                Err(e) => ViewMsg::Error(format!("run list failed: {e:#}")),
            };
            let _ = tx.send(msg);
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ViewMsg::Model(model) => {
                    self.model = Some(*model);
                    self.loading = false;
                }
                ViewMsg::Error(err) => {
                    self.error = Some(err);
                    self.loading = false;
                }
                ViewMsg::Runs(runs) => {
                    self.runs = runs;
                    if self.picker_sel >= self.runs.len() {
                        self.picker_sel = self.runs.len().saturating_sub(1);
                    }
                }
            }
        }
    }

    pub fn handle_mouse_event(&mut self, ev: &MouseEvent) {
        match ev.kind {
            MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
            _ => {}
        }
    }

    /// Esc closes, arrows move the picker, Enter loads it, PgUp/PgDn scroll.
    /// Returns true while open to swallow input.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        if !self.open {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.open = false;
            }
            KeyCode::Up => {
                self.picker_sel = self.picker_sel.saturating_sub(1);
            }
            KeyCode::Down => {
                if self.picker_sel + 1 < self.runs.len() {
                    self.picker_sel += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(run) = self.runs.get(self.picker_sel) {
                    let id = run.id.clone();
                    self.open_run(id);
                }
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5);
            }
            _ => {}
        }
        true
    }

    pub fn draw<B: Backend>(&mut self, f: &mut Frame) {
        self.drain();
        if !self.open {
            return;
        }

        let full = f.area();
        let overlay = centered_rect(92, 90, full);
        f.render_widget(Clear, overlay);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(true))
            .title_style(THEME.title())
            .title("Run Report  (Esc close - Up/Down pick run - Enter load - PgUp/PgDn scroll)")
            .style(Style::default().bg(APP_BACKGROUND));
        let inner = block.inner(overlay);
        f.render_widget(block, overlay);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(38)])
            .split(inner);

        self.render_body(f, cols[0]);
        self.render_picker(f, cols[1]);
    }

    fn render_picker(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Past runs");
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.runs.is_empty() {
            f.render_widget(
                Paragraph::new("No past runs for this machine.")
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.text_muted)),
                inner,
            );
            return;
        }

        let rows: Vec<Row> = self
            .runs
            .iter()
            .enumerate()
            .map(|(i, run)| {
                let label = format!(
                    "{}  {}  {}",
                    started_at_label(run),
                    run.preset_label.as_deref().unwrap_or(&run.tool_label),
                    run.result.as_str(),
                );
                let style = if i == self.picker_sel {
                    Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(THEME.text)
                };
                Row::new(vec![label]).style(style)
            })
            .collect();
        let table = Table::new(rows, [Constraint::Percentage(100)]);
        f.render_widget(table, inner);
    }

    fn render_body(&self, f: &mut Frame, area: Rect) {
        if let Some(err) = &self.error {
            if self.model.is_none() {
                f.render_widget(
                    Paragraph::new(err.as_str())
                        .wrap(Wrap { trim: true })
                        .style(Style::default().fg(THEME.error)),
                    area,
                );
                return;
            }
        }

        let Some(m) = &self.model else {
            let msg = if self.loading {
                "Loading report..."
            } else {
                "No report loaded - pick a past run."
            };
            f.render_widget(
                Paragraph::new(msg).style(Style::default().fg(THEME.text_muted)),
                area,
            );
            return;
        };

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // verdict
                Constraint::Length(4),  // header grid
                Constraint::Length(3),  // summary grid
                Constraint::Min(6),     // stages table
                Constraint::Length(8),  // temp chart
                Constraint::Length(6),  // clock chart
                Constraint::Length(6),  // throughput chart
                Constraint::Min(4),     // timeline
            ])
            .split(area);

        // Verdict line.
        let (vtext, vcolor) = match m.result.as_str() {
            "pass" => ("PASS".to_string(), THEME.success),
            "fail" => (
                format!(
                    "FAIL - {}",
                    m.failure_detail
                        .clone()
                        .or_else(|| m.failure_kind.clone())
                        .unwrap_or_default()
                ),
                THEME.error,
            ),
            "aborted" => ("ABORTED".to_string(), THEME.warning),
            other => (other.to_uppercase(), THEME.text_muted),
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                vtext,
                Style::default().fg(vcolor).add_modifier(Modifier::BOLD),
            )),
            rows[0],
        );

        // Header grid.
        let machine = m.hostname.clone().or(m.machine_id.clone()).unwrap_or_default();
        let duration = m
            .duration_actual_secs
            .map(|s| format!("{:.1} min", s / 60.0))
            .unwrap_or_default();
        let header_lines = vec![
            kv_line("Run", &m.run_id, "Preset", m.preset_label.as_deref().unwrap_or(&m.tool_label)),
            kv_line("Machine", &machine, "Tech", m.tech.as_deref().unwrap_or("")),
            kv_line("Started", &m.started_at, "Duration", &duration),
            kv_line("Order", m.service_order.as_deref().unwrap_or(""), "", ""),
        ];
        f.render_widget(Paragraph::new(header_lines), rows[1]);

        // Summary grid.
        let summary_lines = vec![
            kv_line(
                "CPU max",
                &fmt_opt_f32(m.max_temp_c, "C"),
                "CPU avg",
                &fmt_opt_f32(m.avg_temp_c, "C"),
            ),
            kv_line(
                "GPU max",
                &fmt_opt_f32(m.max_gpu_temp_c, "C"),
                "Max clock",
                &m.max_clock_mhz.map(|c| format!("{c} MHz")).unwrap_or_else(|| "-".into()),
            ),
            kv_line(
                "WHEA/TDR",
                &format!("{}/{}", m.whea_delta_count, m.tdr_count),
                "Errors test/disk",
                &format!("{}/{}", m.test_errors, m.disk_io_errors),
            ),
        ];
        f.render_widget(Paragraph::new(summary_lines), rows[2]);

        self.render_stages(f, rows[3], m);

        report_chart(
            f,
            rows[4],
            "Temps (C)",
            &[
                (&m.cpu_temp, Color::Rgb(230, 130, 70)),
                (&m.gpu_temp, Color::Rgb(120, 180, 240)),
            ],
            m,
        );
        report_chart(
            f,
            rows[5],
            "Avg clock (MHz)",
            &[(&m.avg_clock, Color::Rgb(150, 220, 150))],
            m,
        );
        report_chart(
            f,
            rows[6],
            &format!("Throughput ({})", m.throughput.unit),
            &[(&m.throughput, Color::Rgb(200, 160, 240))],
            m,
        );

        self.render_timeline(f, rows[7], m);
    }

    fn render_stages(&self, f: &mut Frame, area: Rect, m: &RunReportModel) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Stages");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header = Row::new(vec![
            "", "Stage", "Stressor", "Time", "Avg tput", "Err", "WHEA/TDR", "Notes",
        ])
        .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = m
            .stages
            .iter()
            .map(|s| {
                let (mark, mcolor) = match s.result.as_deref() {
                    Some("pass") => ("OK", THEME.success),
                    Some("fail") => ("X", THEME.error),
                    _ => ("-", THEME.text_muted),
                };
                let tput = s
                    .avg_throughput
                    .map(|t| format!("{t:.1} {}", s.throughput_unit))
                    .unwrap_or_else(|| "-".into());
                let notes = if s.violations.is_empty() {
                    s.throughput_cv.map(|cv| format!("CV {cv:.3}")).unwrap_or_default()
                } else {
                    s.violations.join("; ")
                };
                Row::new(vec![
                    Span::styled(mark, Style::default().fg(mcolor)).to_string(),
                    s.label.clone(),
                    s.stressor.clone(),
                    format!("{:.1}m", s.duration_actual_secs / 60.0),
                    tput,
                    s.errors.to_string(),
                    format!("{}/{}", s.whea_delta, s.tdr_delta),
                    notes,
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Length(14),
                Constraint::Length(5),
                Constraint::Length(9),
                Constraint::Min(8),
            ],
        )
        .header(header)
        .style(Style::default().fg(THEME.text));
        f.render_widget(table, inner);
    }

    fn render_timeline(&self, f: &mut Frame, area: Rect, m: &RunReportModel) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Events");
        let inner = block.inner(area);
        f.render_widget(block, area);

        if m.timeline.is_empty() {
            f.render_widget(
                Paragraph::new("No events recorded.").style(Style::default().fg(THEME.text_muted)),
                inner,
            );
            return;
        }

        let rows: Vec<Row> = m
            .timeline
            .iter()
            .skip(self.scroll as usize)
            .take(200)
            .map(|e| {
                Row::new(vec![
                    format!("{:.0}s", e.at_secs),
                    e.kind.clone(),
                    e.code.clone().unwrap_or_default(),
                    e.detail.clone(),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(14),
                Constraint::Length(10),
                Constraint::Min(10),
            ],
        )
        .style(Style::default().fg(THEME.text));
        f.render_widget(table, inner);
    }
}

/// One chart with stage-boundary (grey) and event (red) markers drawn as thin
/// vertical datasets, since ratatui has no VLine.
fn report_chart(
    f: &mut Frame,
    area: Rect,
    title: &str,
    series: &[(&ReportSeries, Color)],
    m: &RunReportModel,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(THEME.border(false))
        .title(Span::styled(title.to_string(), THEME.title()));

    if series.iter().all(|(s, _)| s.points.is_empty()) {
        f.render_widget(block, area);
        return;
    }

    let mut x_max = 0.0_f64;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    for (s, _) in series {
        for (x, y) in &s.points {
            x_max = x_max.max(*x);
            y_min = y_min.min(*y);
            y_max = y_max.max(*y);
        }
    }
    if !y_min.is_finite() {
        y_min = 0.0;
    }
    if !y_max.is_finite() {
        y_max = 1.0;
    }
    let y_pad = ((y_max - y_min) * 0.1).max(1.0);
    let (y_lo, y_hi) = (y_min - y_pad, y_max + y_pad);
    let x_hi = x_max.max(1.0);

    // Vertical marker columns as 2-point datasets spanning the y range.
    let boundary_cols: Vec<Vec<(f64, f64)>> = m
        .stage_boundaries
        .iter()
        .map(|b| vec![(b.at_secs, y_lo), (b.at_secs, y_hi)])
        .collect();
    let event_cols: Vec<Vec<(f64, f64)>> = m
        .event_markers
        .iter()
        .map(|e| vec![(e.at_secs, y_lo), (e.at_secs, y_hi)])
        .collect();

    let mut datasets: Vec<Dataset> = Vec::new();
    for col in &boundary_cols {
        datasets.push(
            Dataset::default()
                .marker(symbols::Marker::Bar)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Rgb(90, 90, 90)))
                .data(col),
        );
    }
    for col in &event_cols {
        datasets.push(
            Dataset::default()
                .marker(symbols::Marker::Bar)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(Color::Rgb(200, 60, 60)))
                .data(col),
        );
    }
    for (s, color) in series {
        if s.points.is_empty() {
            continue;
        }
        datasets.push(
            Dataset::default()
                .name(s.label.clone())
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(*color))
                .data(&s.points),
        );
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds([0.0, x_hi]))
        .y_axis(Axis::default().bounds([y_lo, y_hi]));
    f.render_widget(chart, area);
}

fn kv_line(k1: &str, v1: &str, k2: &str, v2: &str) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{k1}: "), Style::default().fg(THEME.text_muted)),
        Span::styled(v1.to_string(), Style::default().fg(THEME.text)),
    ];
    if !k2.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(format!("{k2}: "), Style::default().fg(THEME.text_muted)));
        spans.push(Span::styled(v2.to_string(), Style::default().fg(THEME.text)));
    }
    Line::from(spans)
}

fn fmt_opt_f32(v: Option<f32>, unit: &str) -> String {
    v.map(|x| format!("{x:.1}{unit}")).unwrap_or_else(|| "-".into())
}

fn started_at_label(run: &StressTestRun) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(run.started_at.timestamp_millis())
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}
