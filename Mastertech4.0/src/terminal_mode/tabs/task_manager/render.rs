
use ratatui::{crossterm::event::KeyCode, layout::{Constraint, Direction, Layout, Margin, Rect}, prelude::Backend, style::{Modifier, Style, Stylize}, widgets::{canvas::{Canvas, Line}, Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, StatefulWidget, Table, TableState, Widget, WidgetRef, FrameExt}, Frame};
use crate::terminal_mode::{styling::{CATPPUCCIN, APP_BACKGROUND}, widgets::HandleWidget};
use std::{collections::HashMap, time::Instant};
use super::SysinfoTab;

impl SysinfoTab {
    fn draw_cpu_chart(&mut self, current_time: f64, lower_bound: f64) -> impl Widget {
        // Get CPU samples in the current window.
        let cpu_points: Vec<(f64, f64)> = self.cpu_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        // Determine current value (or fallback to 0.0).
        let current_cpu = cpu_points.last().map(|p| p.1).unwrap_or(0.0);
        let cpu_canvas = Canvas::default()
            .background_color(APP_BACKGROUND)
            .block(
                Block::default().borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().bg(APP_BACKGROUND))
                    .style(Style::new().bg(APP_BACKGROUND))
                    .title("CPU Usage")
                    .add_modifier(Modifier::BOLD)
            )
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
    
    fn draw_mem_chart(&mut self, current_time: f64, lower_bound: f64) -> impl Widget {
        let mem_points: Vec<(f64, f64)> = self.mem_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        let current_mem = mem_points.last().map(|p| p.1).unwrap_or(0.0);
        let mem_canvas = Canvas::default()
            .background_color(APP_BACKGROUND)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Memory Usage")
                    .add_modifier(Modifier::BOLD)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(CATPPUCCIN.rosewater).bg(APP_BACKGROUND))
                    .style(Style::default().fg(CATPPUCCIN.rosewater).bg(APP_BACKGROUND)),
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
    
    fn draw_gpu_chart(&mut self, current_time: f64, lower_bound: f64) -> impl Widget {
        let gpu_points: Vec<(f64, f64)> = self.gpu_history
            .iter()
            .filter(|s| s.time >= lower_bound)
            .map(|s| (s.time, s.value))
            .collect();
        let current_gpu = gpu_points.last().map(|p| p.1).unwrap_or(0.0);
        let gpu_points_interp = interpolate_points(&gpu_points, 5);
        let gpu_canvas = Canvas::default()
            .background_color(APP_BACKGROUND)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("GPU Usage")
                    .add_modifier(Modifier::BOLD)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(CATPPUCCIN.mauve).bg(APP_BACKGROUND))
                    .style(Style::default().fg(CATPPUCCIN.mauve).bg(APP_BACKGROUND)),
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
            log::info!("Component: {} / {:?}", comp, history);
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
            .background_color(APP_BACKGROUND)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().bg(APP_BACKGROUND))
                    .style(Style::default().bg(APP_BACKGROUND))
                    .title("Component Temps")
                    .add_modifier(Modifier::BOLD),
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

        f.render_widget(temp_canvas, area);
    }
    
    fn draw_process_table(&mut self) -> impl StatefulWidget<State = TableState> + use<> {
        self.processes.sort_by(|a, b| b.cpu_usage.total_cmp(&a.cpu_usage));
        let header = Row::new(
            ["PID", "CPU %", "Memory", "Name"]
                .iter()
                .map(|h| Cell::from(*h).style(Style::default().fg(CATPPUCCIN.peach)))
                .collect::<Vec<_>>(),
        )
        .height(1);
        
        let rows = self.processes.iter().map(|proc| {
            let cells = vec![
                Cell::from(proc.id.to_string()),
                Cell::from(format!("{:.1}", proc.cpu_usage)),
                Cell::from(format!("{:.1} MB", proc.memory)),
                Cell::from(proc.name.clone()),
            ];
            Row::new(cells).height(1)
        });
        
        let table = Table::new(rows, vec![
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Percentage(50),
        ])
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title("Processes")
                .add_modifier(Modifier::BOLD)
                .border_style(Style::default().fg(CATPPUCCIN.red).bg(APP_BACKGROUND))
                .style(Style::default().fg(CATPPUCCIN.red).bg(APP_BACKGROUND)),
        )
        .highlight_symbol(">>")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::WhenSelected);

        table
    }

    fn draw_sysinfo_summary(&mut self) -> impl Widget {
        // let disks: Vec<Disk> = serde_json::from_str(&self.system.disks).unwrap_or_default();

        let mut details_text = format!(
            "Operating System: {} {}\nHostname: {}\nCPU: {}\nMotherboard: {} Vendor: {} S/N: {}\nProduct Name: {} Vendor: {}\n",
            self.system.name,
            self.system.os_version,
            self.system.hostname,
            self.system.cpu,
            self.system.motherboard_name,
            self.system.motherboard_vendor,
            self.system.motherboard_serial,
            self.system.product_name,
            self.system.product_vendor
        );

        for disk in self.system.disks.iter() {
            details_text.push_str(&format!(
                "{}: {} - Total: {:.2}Gb / Avail: {:.2}Gb \n",
                disk.mount_point,
                disk.device_name,
                disk.total_space as f32 / 1e9,
                disk.available_space as f32 / 1e9,
            ));
        }

        let details_paragraph = Paragraph::new(details_text)
            .style(Style::default().fg(CATPPUCCIN.pink).bg(APP_BACKGROUND));

        details_paragraph
    }
}

impl<'a> HandleWidget<'a> for SysinfoTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // Update histories and fetch new sysinfo if available.
        self.ensure_polling();

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

        
        f.render_widget(self.draw_cpu_chart(current_time, lower_bound), left_grid[0]);
        f.render_widget(self.draw_mem_chart(current_time, lower_bound), left_grid[1]);
        f.render_widget(self.draw_gpu_chart(current_time, lower_bound), right_grid[0]);
        self.draw_temp_chart(current_time, lower_bound, f, right_grid[1]);

        // --- Bottom Half: Textual Info & Process List ---
        // Split the bottom half horizontally into two columns:
        // Left: System details and (optionally) other textual info.
        // Right: Process list.
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(overall_chunks[1]);

        // Left column: System Details Panel.
        let details_block = Block::default()
            .border_type(ratatui::widgets::BorderType::Rounded)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CATPPUCCIN.sapphire).bg(APP_BACKGROUND))
            .add_modifier(Modifier::BOLD)
            .title("System Details");

        f.render_widget(details_block, bottom_chunks[0]);
        f.render_widget(self.draw_sysinfo_summary(), inner_rect(bottom_chunks[0], 1));

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