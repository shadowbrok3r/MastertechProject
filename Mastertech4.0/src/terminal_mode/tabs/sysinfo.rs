use std::{collections::HashMap, time::Instant};

use crossbeam::channel::{Receiver, Sender};
use ratatui::{
    crossterm::event::KeyCode, layout::{Constraint, Direction, Layout, Margin, Rect}, prelude::Backend, style::Style, widgets::{canvas::{Canvas, Line}, Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Table, TableState, WidgetRef}, Frame
};
use crate::{filesystem::system_info::{get_sysinfo, get_sysinfo_no_gpu}, terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCIN, widgets::HandleWidget}};
use database::schema::SystemInformation;

pub struct SysinfoTab {
    system: SystemInformation,
    should_quit: bool,
    first_run: bool,
    process_scroll_state: ScrollbarState,
    process_table_state: TableState,
    cpu_history: Vec<Sample>,
    mem_history: Vec<Sample>,
    gpu_history: Vec<Sample>,
    component_temp_history: HashMap<String, Vec<Sample>>,

    tx: Sender<SystemInformation>,
    rx: Receiver<SystemInformation>,

    start_time: Instant,
    pub effect_stage: EffectStage<UniqueEffectId>,
}

/// A sample that records the elapsed time (in seconds) and the value.
#[derive(Debug)]
struct Sample {
    time: f64,  // seconds since start
    value: f64,
}

impl SysinfoTab {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            system: Default::default(), 
            should_quit: false, 
            first_run: true,
            process_table_state: TableState::default(),
            process_scroll_state: ScrollbarState::default(),

            cpu_history: Vec::new(),
            mem_history: Vec::new(),
            gpu_history: Vec::new(),
            component_temp_history: HashMap::new(),

            start_time: Instant::now(),
            effect_stage: EffectStage::default(),

            tx, 
            rx,
        }
    }

    pub fn set_sysinfo(&mut self, sysinfo: SystemInformation) -> &mut Self {
        self.system = sysinfo;
        self
    }

    fn get_sysinfo(&mut self) {
        if !self.should_quit {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                loop {
                    let _ = tx.try_send(get_sysinfo_no_gpu().await.unwrap_or_default());
                    tokio::time::sleep(std::time::Duration::from_secs_f32(0.2)).await;
                }
                // log::info!("Res: {res:?}");
            });
        }
    }

    /// Call this on every update (or in your draw loop) to record the latest value.
    fn update_history(&mut self, system: SystemInformation) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs_f64();

        // CPU history
        self.cpu_history.push(Sample {
            time: elapsed,
            value: system.cpu_percentage as f64,
        });
        // Memory history
        let mem_percent = if system.total_memory > 0.0 {
            system.used_memory / system.total_memory * 100.0
        } else {
            0.0
        };
        self.mem_history.push(Sample {
            time: elapsed,
            value: mem_percent as f64,
        });
        // GPU history
        let gpu_percent = system
            .gpu_info
            .usage
            .get(0)
            .map(|u| u.gpu as f64)
            .unwrap_or(0.0);
        self.gpu_history.push(Sample {
            time: elapsed,
            value: gpu_percent,
        });
        // Component temperatures: update history for each component.
        for (comp, &temp) in system.component_temps.iter() {
            self.component_temp_history
                .entry(comp.clone())
                .or_insert_with(Vec::new)
                .push(Sample {
                    time: elapsed,
                    value: temp as f64,
                });
        }
        log::info!("self.component_temp_history: {:?}", self.component_temp_history.len());
        // (Optionally, trim histories if they exceed a desired maximum length.)
    }

    fn draw_cpu_chart(&mut self, current_time: f64, lower_bound: f64) -> impl WidgetRef {
        // Get CPU samples in the current window.
        let cpu_points: Vec<(f64, f64)> = self.cpu_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        // Determine current value (or fallback to 0.0).
        let current_cpu = cpu_points.last().map(|p| p.1).unwrap_or(0.0);
        let cpu_canvas = Canvas::default()
            .block(Block::default().borders(Borders::ALL).title("CPU Usage"))
            .x_bounds([lower_bound, current_time])
            .y_bounds([0.0, 100.0])
            .paint(move |ctx| {
                // Draw the CPU usage line.
                for window in interpolate_points(&cpu_points, 5).windows(2) {
                    if let [p1, p2] = window {
                        ctx.draw(&Line {
                            x1: p1.0,
                            y1: p1.1,
                            x2: p2.0,
                            y2: p2.1,
                            color: CATPPUCCIN.maroon,
                        });
                    }
                }
                // Draw the axis labels.
                ctx.print(lower_bound, 100.0, "100%");
                ctx.print(lower_bound, 0.0, "0%");
                // Print the current value at the top right (offset left slightly).
                ctx.print(current_time - 0.5, 100.0, format!("{:.1}%", current_cpu));
            });
        cpu_canvas
    }
    
    fn draw_mem_chart(&mut self, current_time: f64, lower_bound: f64) -> impl WidgetRef {
        let mem_points: Vec<(f64, f64)> = self.mem_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        let current_mem = mem_points.last().map(|p| p.1).unwrap_or(0.0);
        let mem_canvas = Canvas::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Memory Usage")
                    .style(Style::default().fg(CATPPUCCIN.rosewater)),
            )
            .x_bounds([lower_bound, current_time])
            .y_bounds([0.0, 100.0])
            .paint(move |ctx| {
                for window in interpolate_points(&mem_points, 5).windows(2) {
                    if let [p1, p2] = window {
                        ctx.draw(&Line {
                            x1: p1.0,
                            y1: p1.1,
                            x2: p2.0,
                            y2: p2.1,
                            color: CATPPUCCIN.lavender,
                        });
                    }
                }
                // Draw the axis labels.
                ctx.print(lower_bound, 100.0, "100%");
                ctx.print(lower_bound, 0.0, "0%");
                // Print the current value at the top right (offset left slightly).
                ctx.print(current_time - 0.5, 100.0, format!("{:.1}%", current_mem));
            });
        mem_canvas
    }
    
    fn draw_gpu_chart(&mut self, current_time: f64, lower_bound: f64) -> impl WidgetRef {
        let gpu_points: Vec<(f64, f64)> = self.gpu_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        let current_gpu = gpu_points.last().map(|p| p.1).unwrap_or(0.0);
        let gpu_points_interp = interpolate_points(&gpu_points, 5);
        let gpu_canvas = Canvas::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("GPU Usage")
                    .style(Style::default().fg(CATPPUCCIN.mauve)),
            )
            .x_bounds([lower_bound, current_time])
            .y_bounds([0.0, 100.0])
            .paint(move |ctx| {
                for window in gpu_points_interp.windows(2) {
                    if let [p1, p2] = window {
                        ctx.draw(&Line {
                            x1: p1.0,
                            y1: p1.1,
                            x2: p2.0,
                            y2: p2.1,
                            color: CATPPUCCIN.teal,
                        });
                    }
                }
                // Draw the axis labels.
                ctx.print(lower_bound, 100.0, "100%");
                ctx.print(lower_bound, 0.0, "0%");
                // Print the current value at the top right (offset left slightly).
                ctx.print(current_time - 0.5, 100.0, format!("{:.1}%", current_gpu));
            });
        gpu_canvas
    }
    
    fn draw_temp_chart(&mut self, current_time: f64, lower_bound: f64, f: &mut Frame, area: Rect) {
        // --- Component Temperatures Chart (Right‑Bottom) ---
        // Build an effective history for each component: use samples in the current window,
        // but if none are present, fall back to the full history.
        let mut comp_effective: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
        for (comp, history) in self.component_temp_history.iter() {
            let filtered: Vec<(f64, f64)> = history
                .iter()
                .filter(|s| s.time >= lower_bound)
                .map(|s| (s.time, s.value))
                .collect();
            let effective = if filtered.is_empty() {
                history.iter().map(|s| (s.time, s.value)).collect()
            } else {
                filtered
            };
            comp_effective.insert(comp.to_string(), effective);
        }

        // For consistent ordering, sort components by name.
        let mut comp_entries: Vec<(&String, &Vec<(f64, f64)>)> = comp_effective.iter().collect();
        comp_entries.sort_by_key(|(comp, _)| *comp);

        // Compute overall min and max temperature from all effective histories.
        let mut overall_temp_min = f64::INFINITY;
        let mut overall_temp_max = f64::NEG_INFINITY;
        for (_comp, points) in comp_entries.iter() {
            for &(_t, val) in *points {
                if val < overall_temp_min { overall_temp_min = val; }
                if val > overall_temp_max { overall_temp_max = val; }
            }
        }
        if overall_temp_min == f64::INFINITY || overall_temp_max == f64::NEG_INFINITY {
            overall_temp_min = 0.0;
            overall_temp_max = 100.0;
        }
        // Ensure a minimum range (e.g. at least 1°C).
        if (overall_temp_max - overall_temp_min).abs() < 1.0 {
            overall_temp_max = overall_temp_min + 1.0;
        }

        // Add some vertical padding.
        let padding = 2.0;
        let y_bottom = overall_temp_min - padding;
        let y_top = overall_temp_max + padding;

        // Use only CATPPUCCIN colors in a fixed order.
        let comp_colors = [
            CATPPUCCIN.yellow,
            CATPPUCCIN.green,
            CATPPUCCIN.blue,
            CATPPUCCIN.lavender,
            CATPPUCCIN.teal,
            CATPPUCCIN.peach,
        ];

        let temp_canvas = Canvas::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Component Temps"),
            )
            // Use the actual time window for x.
            .x_bounds([lower_bound, current_time])
            // Use the computed bounds in increasing order.
            .y_bounds([y_bottom, y_top])
            .paint(move |ctx| {
                // For debugging, print the computed y-axis bounds.
                ctx.print(lower_bound, y_bottom, format!("Min: {:.1}°C", y_bottom));
                ctx.print(lower_bound, y_top, format!("Max: {:.1}°C", y_top));

                // For each component in sorted order, draw its history.
                for (idx, (comp, points)) in comp_entries.iter().enumerate() {
                    let interp = interpolate_points(points, 5);
                    let color = comp_colors[idx % comp_colors.len()];
                    if interp.len() < 2 {
                        if let Some(&(x, y)) = interp.first() {
                            ctx.print(x + 0.5, y, "*");
                        }
                    } else {
                        for window in interp.windows(2) {
                            if let [p1, p2] = window {
                                ctx.draw(&Line {
                                    x1: p1.0,
                                    y1: p1.1,
                                    x2: p2.0,
                                    y2: p2.1,
                                    color,
                                });
                            }
                        }
                    }
                    // Instead of using the first point’s x, print the label at a fixed offset
                    // from the left bound.
                    ctx.print(lower_bound + 0.5, y_top, comp.to_string());
                }
            });

        f.render_widget_ref(temp_canvas, area);
    }
    
    fn draw_process_table(&mut self) -> impl StatefulWidget<State = TableState> + use<> {
        self.system.processes.sort_by(|a, b| b.cpu_usage.total_cmp(&a.cpu_usage));
        let header = Row::new(
            ["PID", "Name", "CPU %", "Memory"]
                .iter()
                .map(|h| Cell::from(*h).style(Style::default().fg(CATPPUCCIN.peach)))
                .collect::<Vec<_>>(),
        )
        .height(1);
        let rows = self.system.processes.iter().map(|proc| {
            let cells = vec![
                Cell::from(proc.id.to_string()),
                Cell::from(proc.name.clone()),
                Cell::from(format!("{:.1}", proc.cpu_usage)),
                Cell::from(format!("{:.1} MB", proc.memory)),
            ];
            Row::new(cells).height(1)
        });
        let table = Table::new(rows, vec![
            Constraint::Length(6),
            Constraint::Percentage(50),
            Constraint::Length(8),
            Constraint::Length(10),
        ])
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Processes")
                .style(Style::default().fg(CATPPUCCIN.red)),
        )
        .highlight_symbol(">>")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::WhenSelected);

        table
    }

    fn draw_sysinfo_summary(&mut self) -> impl WidgetRef {
        // let disks: Vec<Disk> = serde_json::from_str(&self.system.disks).unwrap_or_default();

        let details_text = format!(
            "OS Name: {}\nKernel: {}\nOS Version: {}\nHostname: {}\n{}\nDisks: {}",
            self.system.name,
            self.system.kernel_version,
            self.system.os_version,
            self.system.hostname,
            self.system.number_of_cpus,
            self.system.disks
        );
        let details_paragraph = Paragraph::new(details_text)
            .style(Style::default().fg(CATPPUCCIN.subtext0));

        details_paragraph
    }
}

impl<'a> HandleWidget<'a> for SysinfoTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // Update histories and fetch new sysinfo if available.
        if self.first_run {
            self.first_run = false;
            self.get_sysinfo();
        }
        if let Ok(sysinfo) = self.rx.try_recv() {
            self.update_history(sysinfo);
        }

        // --- Overall Layout ---
        // Split the terminal vertically into two halves:
        // Top half for charts (2 rows × 2 columns)
        // Bottom half for textual info (left) and process list (right).
        let overall_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(area);

        // --- Top Half: 2×2 Grid of Charts ---
        // Split the top half horizontally into two equal columns.
        let top_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(overall_chunks[0]);

        // Left column: split vertically into two equal panels.
        let left_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(top_columns[0]);

        // Right column: split vertically into two equal panels.
        let right_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(top_columns[1]);

        // Define a time window for the usage charts.
        let time_window = 15.0; // seconds
        let current_time = Instant::now().duration_since(self.start_time).as_secs_f64();
        let lower_bound = if current_time > time_window {
            current_time - time_window
        } else {
            0.0
        };

        
        f.render_widget_ref(self.draw_cpu_chart(current_time, lower_bound), left_grid[0]);
        f.render_widget_ref(self.draw_mem_chart(current_time, lower_bound), left_grid[1]);
        f.render_widget_ref(self.draw_gpu_chart(current_time, lower_bound), right_grid[0]);
        self.draw_temp_chart(current_time, lower_bound, f, right_grid[1]);

        // --- Bottom Half: Textual Info & Process List ---
        // Split the bottom half horizontally into two columns:
        // Left: System details and (optionally) other textual info.
        // Right: Process list.
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
            .split(overall_chunks[1]);

        // Left column: System Details Panel.
        let details_block = Block::default().borders(Borders::ALL).title("System Details");
        f.render_widget_ref(details_block, bottom_chunks[0]);
        f.render_widget_ref(self.draw_sysinfo_summary(), inner_rect(bottom_chunks[0], 1));

        // Right column: Process List Panel.
        f.render_stateful_widget(self.draw_process_table(), bottom_chunks[1], &mut self.process_table_state);

        // Render a scrollbar for the process list.
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            bottom_chunks[1].inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            &mut self.process_scroll_state,
        );
    }
    
    // fn handle_mouse_event(&self, mouse_event: &ratatui::crossterm::event::MouseEvent) { 
    //     match mouse_event.kind {
    //         // ratatui::crossterm::event::MouseEventKind::Down(mouse_button) => todo!(),
    //         // ratatui::crossterm::event::MouseEventKind::Up(mouse_button) => todo!(),
    //         // ratatui::crossterm::event::MouseEventKind::Drag(mouse_button) => todo!(),
    //         ratatui::crossterm::event::MouseEventKind::Moved => {
    //             // if self.process_table_state.
    //         },
    //         ratatui::crossterm::event::MouseEventKind::ScrollDown => self.process_table_state.scroll_down_by(1),
    //         ratatui::crossterm::event::MouseEventKind::ScrollUp => self.process_table_state.scroll_up_by(1),
    //         _ => {}
    //     }
    // }
    
    fn handle_key_event(&mut self, key_event: ratatui::crossterm::event::KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Up => self.process_table_state.scroll_up_by(1),
            KeyCode::Down => self.process_table_state.scroll_down_by(1),
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        true
     }
}


fn interpolate_points(points: &[(f64, f64)], steps: usize) -> Vec<(f64, f64)> {
    let mut new_points = Vec::new();
    for window in points.windows(2) {
        if let [p1, p2] = window {
            new_points.push(*p1);
            for s in 1..steps {
                let t = s as f64 / steps as f64;
                let x = p1.0 + (p2.0 - p1.0) * t;
                let y = p1.1 + (p2.1 - p1.1) * t;
                new_points.push((x, y));
            }
        }
    }
    if let Some(last) = points.last() {
        new_points.push(*last);
    }
    new_points
}



// Helper to create an inner rect with a margin
fn inner_rect(area: Rect, margin: u16) -> Rect {
    Rect {
        x: area.x + margin,
        y: area.y + margin,
        width: area.width.saturating_sub(margin * 2),
        height: area.height.saturating_sub(margin * 2),
    }
}