use eframe::egui::{Align, Button, CentralPanel, Color32, FontId, Layout, Response, RichText, ScrollArea, TextStyle, TopBottomPanel, Ui, Vec2, Widget};
use egui_plot::{Bar, BarChart, Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use displays::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use std::{collections::{HashMap, VecDeque}, time::Instant};
use database::schema::SystemInformation;
use tokio::spawn;
use log::info;
use crate::filesystem::system_info::get_sysinfo;

mod process_table;


#[derive(Default)]
pub enum ResourceMonitorState {
    #[default]
    Cpu,
    Ram,
    Gpu,
    Processes,
    Network,
    Temperatures,
}

pub struct ResourceMonitor {
    sysinfo_channel: (Sender<SystemInformation>, Receiver<SystemInformation>),
    cpu_usage_chart: MetricPlot,
    cpu_clock_chart: MetricPlot,
    ram_usage_chart: MetricPlot,
    component_temp_plot: LinePlot,
    disk_usage_plot: LinePlot,
    network_interface_plot: LinePlot,
    processes: Vec<database::schema::Process>,
    start_time: Instant,
    state: ResourceMonitorState
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        let sysinfo_channel = SystemInformation::create_unbounded_channel();
        Self {
            sysinfo_channel,
            cpu_usage_chart: MetricPlot::new("Time (s)", "CPU Usage (%)"),
            cpu_clock_chart: MetricPlot::new("Time (s)", "CPU Clock (MHz)"),
            ram_usage_chart: MetricPlot::new("Time (s)", "RAM Usage (GB)"),
            component_temp_plot: LinePlot::new(50),
            disk_usage_plot: LinePlot::new(50),
            network_interface_plot: LinePlot::new(50),
            start_time: Instant::now(), // Initialize the timer
            state: ResourceMonitorState::default(),
            processes: Default::default(),
        }
    }
}

impl ResourceMonitor {
    fn receive(&mut self) {
        if let Ok(sysinfo) = self.sysinfo_channel.1.try_recv() {
            info!("got sysinfo: {sysinfo:?}");
            let elapsed_time = self.start_time.elapsed().as_secs_f32();


            // if elapsed_time > 2. { }
            self.cpu_usage_chart.update(elapsed_time, sysinfo.cpu_percentage);
            self.cpu_clock_chart.update(elapsed_time, normalize(sysinfo.cpu_clock, 0.0, 100.0)); // Normalize MHz
            self.ram_usage_chart.update(elapsed_time, if sysinfo.total_memory > 0.0 { (sysinfo.used_memory / sysinfo.total_memory) * 100.0 } else { 0.0 }); // Convert MB to GB
            // Update component temperatures
            for (component, &temp) in &sysinfo.component_temps {
                self.component_temp_plot.update_line(component, elapsed_time, temp);
            }

            // Update disk usage
            for disk_info in sysinfo.disks.split("Disk").skip(1) {
                if let Some((disk_name, used, _total)) = parse_disk_info(disk_info) {
                    let used_gb = used as f32 / 1e9;
                    self.disk_usage_plot.update_line(&disk_name, elapsed_time, used_gb);
                }
            }

            // Update network interfaces
            for interface in &sysinfo.network_interfaces {
                let rx_gb = interface.total_received as f32 / 1e9;
                let tx_gb = interface.total_transmitted as f32 / 1e9;
                self.network_interface_plot.update_line(&interface.interface_name, elapsed_time, rx_gb + tx_gb);
            }
            self.processes = sysinfo.processes;
        }
    }

    pub fn display(&mut self, ui: &mut Ui) {
        self.receive();

        ui.ctx().request_repaint_after_secs(2.);
        TopBottomPanel::top("Resource Monitor Top Panel").exact_height(25.).show_inside(ui, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui | {
                    let button_stroke = ui.style().visuals.window_stroke;
                    let button_size = Vec2::new(100.0, 15.0);

                    if Button::new("Cpu").min_size(button_size).frame(true).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Cpu
                    }

                    ui.add_space(5.);

                    if Button::new("Graphics").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Gpu
                    }

                    ui.add_space(5.);

                    if Button::new("Ram").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state =ResourceMonitorState::Ram
                    }

                    ui.add_space(5.);

                    if Button::new("Processes").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Processes
                    }

                    ui.add_space(5.);

                    if Button::new("Temperatures").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Temperatures
                    }

                    ui.add_space(5.);

                    if Button::new("Network").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Network
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui | {
                    ui.add_space(2.);
                    let button_stroke = ui.style().visuals.window_stroke;
                    let button_size = Vec2::new(60.0, 15.0);
                    if Button::new("Refresh").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        let tx = self.sysinfo_channel.0.clone();
                        spawn(async move {
                            let res = live_computer_stats(tx).await; 
                            log::info!("Getting live sys stats: {res:?}");
                        });
                    }

                    ui.add_space(ui.available_width()/1.5);

                    ui.label(
                        RichText::new("Resource monitor")
                            .color(Color32::LIGHT_RED)
                            .heading()
                            .font(FontId::proportional(20.))
                    );

                });
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {

                // New line charts
                let mut colors = HashMap::new();
                colors.insert("Component Temps".to_string(), Color32::from_rgb(235, 12, 38));
                colors.insert("Disk Usage".to_string(), Color32::from_rgb(12, 235, 97));
                colors.insert("Network Usage".to_string(), Color32::from_rgb(240, 141, 55));

                match self.state {
                    ResourceMonitorState::Cpu => {
                        ui.group(|ui| {
                            ui.vertical_centered(|ui| ui
                                .label(
                                    RichText::new(format!("CPU ()"))
                                    .underline()
                                    .color(
                                        ui.style().visuals.error_fg_color
                                    )
                                    .heading()
                                    .font(
                                        FontId::proportional(20.)
                                    )
                                )
                            );
        
                            ui.add_space(50.);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui | {
                                ui.add_space(50.);
                                ui.scope(|ui| {
                                    ui.set_width(ui.available_width()/2.);
                                    self.cpu_clock_chart.ui(ui, "CPU Clock Chart", Color32::from_rgb(7, 242, 176));
                                });
                                self.cpu_usage_chart.ui(ui, "CPU Usage Chart", Color32::from_rgb(62, 7, 242));
                            });
                        });
                    },
                    ResourceMonitorState::Ram => {
                        ui.group(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("RAM")
                                    .color(
                                        ui.style().visuals.error_fg_color
                                    )
                                    .heading()
                                    .underline()
                                    .font(
                                        FontId::proportional(20.)
                                    )
                                );
                                ui.add_space(50.);
                                self.ram_usage_chart.ui(ui, "RAM Usage Chart", Color32::from_rgb(242, 7, 179));
                            });
        
                        });
                    },
                    ResourceMonitorState::Gpu => {

                    },
                    ResourceMonitorState::Processes => {
                        ui.group(|ui| {
                            ui.horizontal_top(|ui| {
                                ui.label("PID");
                                ui.label("Process");
                                ui.label("CMD");
                                ui.label("Process CPU Usage");
                                ui.label("Process Memory Usage");
                                ui.label("Process Disk Usage");
                                ui.label("Process UID");
                            });

                            for process in self.processes.iter() {
                                ui.horizontal_top(|ui| {

                                    ui.label(process.id.to_string());
                                    
                                    ui.label(process.name.clone());
                                    
                                    ui.label(process.cmd.clone());
                                    ui.label(process.cpu_usage.to_string());
                                    
                                    ui.label(process.memory.clone());
                                    ui.label(process.process_disk_usage.total_written_bytes.to_string());

                                    if let Some(uid) = process.user_id.as_ref() {
                                        ui.label(uid);
                                    }
                                });
                            }
                        });
                    },
                    ResourceMonitorState::Network => {
                        ui.group(|ui| {
                            // self.disk_usage_plot.ui(ui, "Disk I/O", &colors);
                            self.network_interface_plot.ui(ui, "Network Usage", &colors);
                        });
                    },
                    ResourceMonitorState::Temperatures => {
                        ui.group(|ui| {
                            self.component_temp_plot.ui(ui, "Temps", &colors);
                        });
                    },
                }
            });
        });
    }
}

#[derive(Clone, PartialEq, Default)]
pub struct LinePlot {
    data: HashMap<String, VecDeque<(f32, f32)>>, // Map line names to VecDeque of (x, y) points
    max_points: usize,                          // Maximum number of points per line
}

impl LinePlot {
    pub fn new(max_points: usize) -> Self {
        Self {
            data: HashMap::new(),
            max_points,
        }
    }

    pub fn add_line(&mut self, name: &str) {
        self.data.entry(name.to_string()).or_insert_with(VecDeque::new);
    }

    pub fn update_line(&mut self, name: &str, x_value: f32, y_value: f32) {
        if let Some(points) = self.data.get_mut(name) {
            if points.len() >= self.max_points {
                points.pop_front();
            }
            points.push_back((x_value, y_value));
        }
    }

    pub fn lines(&self, colors: &HashMap<String, Color32>) -> Vec<Line> {
        self.data
            .iter()
            .filter_map(|(name, points)| {
                let color = colors.get(name)?;
                let plot_points: Vec<[f64; 2]> =
                    points.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
                Some(
                    Line::new(PlotPoints::new(plot_points))
                        .color(*color)
                        .name(name),
                )
            })
            .collect()
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str, colors: &HashMap<String, Color32>) -> Response {
        let plot = Plot::new(plot_name)
        .legend(
            Legend::default()
            .position(
                Corner::LeftTop
            )
            .text_style(
                TextStyle::Body
            )
            .background_alpha(0.90)
        )
        .width(ui.available_size_before_wrap().x/1.5)
        .height(ui.available_size_before_wrap().y/1.5)
        .allow_drag(false)
        .show_background(false);

        plot.show(ui, |plot_ui| {
            for line in self.lines(colors) {
                plot_ui.line(line);
            }
        })
        .response
    }
}


#[derive(Clone, PartialEq, Default)]
pub struct MetricBarChart {
    name: String,
    color: Color32,
    data: VecDeque<(f32, f32)>, // (x, y) pairs
    max_points: usize,          // Maximum number of bars
}

impl MetricBarChart {
    pub fn new(name: &str, color: Color32, max_points: usize) -> Self {
        Self {
            name: name.to_string(),
            color,
            data: VecDeque::new(),
            max_points,
        }
    }

    pub fn update(&mut self, x_value: f32, y_value: f32) {
        if self.data.len() >= self.max_points {
            self.data.pop_front();
        }
        self.data.push_back((x_value, y_value));
    }

    pub fn to_bar_chart(&self) -> BarChart {
        let bars: Vec<Bar> = self
            .data
            .iter()
            .map(|&(x, y)| Bar::new(x as f64, y as f64))
            .collect();

        BarChart::new(bars).name(self.name.clone()).color(self.color)
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str) -> Response {
        Plot::new(plot_name)
            .legend(
                Legend::default()
                .position(
                    Corner::LeftTop
                )
                .text_style(
                    TextStyle::Body
                )
                .background_alpha(0.90)
            )
            .width(ui.available_size_before_wrap().x/1.5)
            .height(ui.available_size_before_wrap().y/1.5)
            .allow_drag(false)
            .show_background(false)
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(self.to_bar_chart());
            })
            .response
    }
}

#[derive(Clone, PartialEq, Default)]
pub struct MetricPlot {
    data: VecDeque<(f32, f32)>, // (x, y) pairs for the chart
    x_label: String,            // Label for x-axis
    y_label: String,            // Label for y-axis
}

impl MetricPlot {
    pub fn new(x_label: &str, y_label: &str) -> Self {
        Self {
            data: VecDeque::new(),
            x_label: x_label.to_string(),
            y_label: y_label.to_string(),
        }
    }

    pub fn update(&mut self, x_value: f32, y_value: f32) {
        const MAX_BARS: usize = 50;
        if self.data.len() >= MAX_BARS {
            self.data.pop_front();
        }
        self.data.push_back((x_value, y_value));
    }

    pub fn line(&self, name: &str, color: Color32) -> Line {
        let update_interval = 2.0; // Time between updates in seconds

        let points: Vec<[f64; 2]> = self.data
            .iter()
            // .zip(&self.y_values)
            .map(|(x, y)| [*x as f64 * update_interval, *y as f64])
            .collect();
        Line::new(PlotPoints::new(points))
            .color(color)
            .style(LineStyle::Solid)
            .name(name)
    }

    pub fn ui(&self, ui: &mut Ui, plot_name: &str, color: Color32) -> Response {
        let x_label = RichText::new(&self.x_label).size(14.0).strong();
        let y_label = RichText::new(&self.y_label).size(14.0).strong();

        // let bars: Vec<Bar> = self
        //     .data
        //     .iter()
        //     .map(|&(x, y)| Bar::new(x as f64, y as f64))
        //     .collect();

        // let bar_chart = BarChart::new(bars).name(plot_name).color(color);
        let line_chart = self.line(plot_name, color);

        Plot::new(plot_name)
            .legend(
                Legend::default()
                .position(
                    Corner::LeftTop
                )
                .text_style(
                    TextStyle::Body
                )
                .background_alpha(0.90)
            )
            .width(ui.available_size_before_wrap().x/1.5)
            .height(ui.available_size_before_wrap().y/1.5)
            .allow_drag(false)
            .show_background(false)
            .x_axis_label(x_label)
            .y_axis_label(y_label)
            .show(ui, |plot_ui| {
                plot_ui.line(line_chart);
            })
            .response
    }
}

fn normalize(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}

fn parse_disk_info(disk_info: &str) -> Option<(String, u64, u64)> {
    let parts: Vec<&str> = disk_info.split_whitespace().collect();
    if parts.len() >= 4 {
        let name = parts[0].to_string();
        let used = parts[2].parse::<u64>().ok()?;
        let total = parts[3].parse::<u64>().ok()?;
        return Some((name, used, total));
    }
    None
}

async fn live_computer_stats(tx: Sender<SystemInformation>) -> anyhow::Result<(), anyhow::Error>{
    loop {
        tx.send(get_sysinfo().await?)?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    #[allow(unreachable_code)]
    Ok(())
}
