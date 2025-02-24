use crossbeam::channel::{Receiver, Sender};
use ratatui::{
    crossterm::event::KeyCode, layout::{Constraint, Direction, Layout, Margin, Rect}, prelude::Backend, style::{Color, Style}, widgets::{canvas::{Canvas, Line}, Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState}, Frame
};
use crate::{filesystem::system_info::get_sysinfo, terminal_mode::{fx::{effect::UniqueEffectId, EffectStage}, styling::CATPPUCCIN, widgets::HandleWidget}};
use database::schema::SystemInformation;

pub struct SysinfoTab {
    system: SystemInformation,
    should_pause: bool,
    first_run: bool,
    process_scroll_state: ScrollbarState,
    process_table_state: TableState,
    cpu_history: Vec<f64>,
    mem_history: Vec<f64>,
    gpu_history: Vec<f64>,
    
    tx: Sender<SystemInformation>,
    rx: Receiver<SystemInformation>,

    pub effect_stage: EffectStage<UniqueEffectId>,
}

impl SysinfoTab {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            system: Default::default(), 
            should_pause: false, 
            first_run: true,
            process_table_state: TableState::default(),
            process_scroll_state: ScrollbarState::default(),

            cpu_history: Vec::new(),
            mem_history: Vec::new(),
            gpu_history: Vec::new(),
            
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
        if !self.should_pause {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                loop {
                    let _ = tx.try_send(get_sysinfo().await.unwrap_or_default());
                    tokio::time::sleep(std::time::Duration::from_secs_f32(0.2)).await;
                }
                // log::info!("Res: {res:?}");
            });
        }
    }
}

impl<'a> HandleWidget<'a> for SysinfoTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        if self.first_run {
            self.first_run = false;
            self.get_sysinfo();
        }
        if let Ok(sysinfo) = self.rx.try_recv() {
            self.set_sysinfo(sysinfo);
        }

        // Update history buffers (using a fixed maximum history length)
        const HISTORY_LENGTH: usize = 20;
        self.cpu_history.push(self.system.cpu_percentage as f64);
        if self.cpu_history.len() > HISTORY_LENGTH {
            self.cpu_history.remove(0);
        }
        let mem_percent = if self.system.total_memory > 0.0 {
            self.system.used_memory / self.system.total_memory * 100.0
        } else {
            0.0
        };
        self.mem_history.push(mem_percent.into());
        if self.mem_history.len() > HISTORY_LENGTH {
            self.mem_history.remove(0);
        }
        let gpu_percent = self
            .system
            .gpu_info
            .usage
            .get(0)
            .map(|u| u.gpu as f64)
            .unwrap_or(0.0);
        self.gpu_history.push(gpu_percent);
        if self.gpu_history.len() > HISTORY_LENGTH {
            self.gpu_history.remove(0);
        }

        // Split horizontally: left (65%) for charts, right (35%) for process table and footer.
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)].as_ref())
            .split(area);

        // Left side: stack CPU, Memory, and GPU charts vertically.
        let chart_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ]
                .as_ref(),
            )
            .split(h_chunks[0]);

        // --- CPU Chart ---
        let smoothed_cpu = smooth_history(&self.cpu_history, 3);
        let cpu_points: Vec<(f64, f64)> = self
            .cpu_history
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = if smoothed_cpu.len() > 1 {
                    i as f64 / ((smoothed_cpu.len() - 1) as f64) * 100.0
                } else {
                    0.0
                };
                (x, val)
            })
            .collect();
        let cpu_canvas = Canvas::default()
            .block(
                Block::default().borders(Borders::ALL).title("CPU Usage").style(Style::default().fg(CATPPUCCIN.sky))
            )
            .x_bounds([0.0, 20.0])
            .y_bounds([0.0, 100.0])
            .paint(|ctx| {
                // Draw each segment connecting consecutive points.
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
                // Add simple axis labels.
                ctx.print(0.0, 100.0, "100%");
                ctx.print(0.0, 50.0, "50%");
                ctx.print(0.0, 0.0, "0%");
            });
        f.render_widget_ref(cpu_canvas, chart_chunks[0]);

        // --- Memory Chart ---
        let mem_points: Vec<(f64, f64)> = self
            .mem_history
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = if self.mem_history.len() > 1 {
                    i as f64 / ((self.mem_history.len() - 1) as f64) * 100.0
                } else {
                    0.0
                };
                (x, val)
            })
            .collect();
        let mem_canvas = Canvas::default()
            .block(
                Block::default().borders(Borders::ALL).title("Memory Usage").style(Style::default().fg(CATPPUCCIN.rosewater))
            )
            .x_bounds([0.0, 100.0])
            .y_bounds([0.0, 100.0])
            .paint(|ctx| {
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
                ctx.print(0.0, 100.0, "100%");
                ctx.print(0.0, 50.0, "50%");
                ctx.print(0.0, 0.0, "0%");
            });
        f.render_widget_ref(mem_canvas, chart_chunks[1]);

        // --- GPU Chart ---
        let gpu_points: Vec<(f64, f64)> = self
            .gpu_history
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = if self.gpu_history.len() > 1 {
                    i as f64 / ((self.gpu_history.len() - 1) as f64) * 100.0
                } else {
                    0.0
                };
                (x, val)
            })
            .collect();
        let gpu_canvas = Canvas::default()
            .block(
                Block::default().borders(Borders::ALL).title("GPU Usage").style(Style::default().fg(CATPPUCCIN.mauve))
            )
            .x_bounds([0.0, 20.0])
            .y_bounds([0.0, 100.0])
            .paint(|ctx| {
                for window in interpolate_points(&gpu_points, 5).windows(2) {
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
                ctx.print(0.0, 100.0, "100%");
                ctx.print(0.0, 50.0, "50%");
                ctx.print(0.0, 0.0, "0%");
            });
        f.render_widget_ref(gpu_canvas, chart_chunks[2]);

        // Right side: split vertically into the process table and a footer.
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(4)].as_ref())
            .split(h_chunks[1]);

        // --- Process Table ---
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
        .block(Block::default().borders(Borders::ALL).title("Processes").style(Style::default().fg(CATPPUCCIN.red)))
        .highlight_symbol(">>")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::WhenSelected);
        f.render_stateful_widget(table, v_chunks[0], &mut self.process_table_state);

        // --- Scrollbar ---
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            v_chunks[0].inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            &mut self.process_scroll_state,
        );

        // --- Footer ---
        let footer = Paragraph::new("(Esc) quit | (↑/↓) scroll")
            .style(Style::default().fg(CATPPUCCIN.text).bg(Color::Rgb(10, 10, 12)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Double)
                    .border_style(Style::default().fg(CATPPUCCIN.blue)),
            )
            .centered();
        f.render_widget_ref(footer, v_chunks[1]);
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



fn smooth_history(history: &Vec<f64>, window: usize) -> Vec<f64> {
    // If the window is too small or there’s not enough data, just return a clone.
    if window < 2 || history.len() < window {
        return history.clone();
    }
    let half = window / 2;
    let mut smoothed = Vec::with_capacity(history.len());
    for i in 0..history.len() {
        let mut sum = 0.0;
        let mut total_weight = 0.0;
        let start = if i >= half { i - half } else { 0 };
        let end = (i + half + 1).min(history.len());
        for j in start..end {
            // Triangular weight: maximum at the center, falling off linearly.
            let weight = 1.0 - ((i as isize - j as isize).abs() as f64) / (half as f64 + 1.0);
            sum += history[j] * weight;
            total_weight += weight;
        }
        smoothed.push(sum / total_weight);
    }
    smoothed
}
