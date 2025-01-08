use eframe::egui::{Align, Button, CentralPanel, Color32, FontId, Layout, RichText, ScrollArea, TopBottomPanel, Ui, Vec2, Widget};
use process_table::ProcessTableViewer;
use crate::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use line_plot::LinePlot;
use metric_plot::MetricPlot;
use std::{collections::HashMap, time::{Duration, Instant}};
use database::schema::SystemInformation;
pub mod process_table;
pub mod line_plot;
pub mod metric_plot;
pub mod bar_chart;

#[derive(Default, PartialEq, Eq)]
pub enum ResourceMonitorState {
    #[default]
    Cpu,
    Ram,
    Gpu,
    Processes,
    Network,
    Temperatures,
    RequestingData,
    Drives,
    Stop
}

pub struct ResourceMonitor {
    pub state: ResourceMonitorState,
    pub sysinfo_channel: (Sender<SystemInformation>, Receiver<SystemInformation>),
    cpu_usage_chart: MetricPlot,
    cpu_clock_chart: MetricPlot,
    ram_usage_chart: MetricPlot,
    gpu_temp_chart: MetricPlot,
    gpu_mem_chart: MetricPlot,
    component_temp_plot: LinePlot,
    disk_usage_plot: LinePlot,
    network_interface_plot: LinePlot,
    start_time: Instant,
    process_table_viewer: ProcessTableViewer,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        let sysinfo_channel = SystemInformation::create_unbounded_channel();
        Self {
            sysinfo_channel,
            cpu_usage_chart: MetricPlot::new("Time (s)", "CPU Usage (%)"),
            cpu_clock_chart: MetricPlot::new("Time (s)", "CPU Clock (MHz)"),
            ram_usage_chart: MetricPlot::new("Time (s)", "RAM Usage (GB)"),
            gpu_temp_chart: MetricPlot::new("Time (s)", "GPU Temp (C)"),
            gpu_mem_chart: MetricPlot::new("Time (s)", "GPU Memory"),
            component_temp_plot: LinePlot::new(50),
            disk_usage_plot: LinePlot::new(50),
            network_interface_plot: LinePlot::new(50),
            start_time: Instant::now(), // Initialize the timer
            state: ResourceMonitorState::default(),
            process_table_viewer: ProcessTableViewer::new(),
        }
    }
}

impl ResourceMonitor {
    fn receive(&mut self) {
        if let Ok(sysinfo) = self.sysinfo_channel.1.try_recv() {
            self.set_sysinfo(sysinfo);
        }

        // Clean up old data for MetricPlots
        self.cpu_usage_chart.clean_old_data(50);
        self.cpu_clock_chart.clean_old_data(50);
        self.ram_usage_chart.clean_old_data(50);
        self.gpu_temp_chart.clean_old_data(50);
        self.gpu_mem_chart.clean_old_data(50);
            // Clean up old data for LinePlots
        for points in self.disk_usage_plot.data.values_mut() {
            while points.len() > self.disk_usage_plot.max_points {
                points.pop_front();
            }
        }
        for points in self.network_interface_plot.data.values_mut() {
            while points.len() > self.network_interface_plot.max_points {
                points.pop_front();
            }
        }
    }

    pub fn set_sysinfo(&mut self, sysinfo: SystemInformation) {
        match self.state {
            ResourceMonitorState::Stop => {},
            _ => {
                let wrapped_time = self.start_time.elapsed().as_secs_f32();
                self.cpu_usage_chart.update(wrapped_time, sysinfo.cpu_percentage);
                // Normalize MHz
                self.cpu_clock_chart.update(wrapped_time, normalize(sysinfo.cpu_clock, 0.0, 100.0));
                // Convert MB to GB
                self.ram_usage_chart.update(wrapped_time, if sysinfo.total_memory > 0.0 { (sysinfo.used_memory / sysinfo.total_memory) * 100.0 } else { 0.0 }); 

                // Update component temperatures
                for (component, &temp) in &sysinfo.component_temps {
                    self.component_temp_plot.update_line(component, wrapped_time, temp);
                }

                for (gpu, gpu_usage) in sysinfo.gpu_info.card.iter().zip(sysinfo.gpu_info.usage.iter()) {
                    self.gpu_temp_chart.update(wrapped_time, gpu.temperature as f32);
                    self.gpu_mem_chart.update(wrapped_time, gpu_usage.memory_usage as f32);
                    // self.gpu_mem_chart.update(wrapped_time, normalize(sysinfo.cpu_clock, 0.0, 100.0)); 
                    // self.gpu_plot.update_line(&gpu.name, wrapped_time, gpu.temperature as f32);
                    // for x in &gpu_usage.processes {
                    //     self.gpu_plot.update_line(&gpu.name, wrapped_time, x.memory as f32);
                    // }
                }

                // Update disk usage
                for disk_info in sysinfo.disks.split("Disk").skip(1) {
                    if let Some((disk_name, used, _total)) = parse_disk_info(disk_info) {
                        let used_gb = used as f32 / 1e9;
                        self.disk_usage_plot.update_line(&disk_name, wrapped_time, used_gb);
                    }
                }

                // Update network interfaces
                for interface in &sysinfo.network_interfaces {
                    log::info!("interface: {interface:?}");
                    let rx_gb = interface.total_received as f32;
                    let tx_gb = interface.total_transmitted as f32;
                    self.network_interface_plot.update_line(&interface.interface_name, wrapped_time, rx_gb + tx_gb);
                }
            }
        }
            self.process_table_viewer.set_data(sysinfo.processes);
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

                    if Button::new("Drives").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                        self.state = ResourceMonitorState::Drives
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
                        self.state = ResourceMonitorState::RequestingData;
                        self.start_time = Instant::now();
                    }

                    ui.add_space(5.);
                    if let ResourceMonitorState::Stop = self.state {
                    } else {
                        if Button::new("Stop").min_size(button_size).stroke(button_stroke).ui(ui).clicked() {
                            self.state = ResourceMonitorState::Stop;
                        }
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
                        ui.group(|ui| {
                            ui.vertical_centered(|ui| ui
                                .label(
                                    RichText::new(format!("GPU"))
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
                                    self.gpu_temp_chart.ui(ui, "GPU Temps", Color32::from_rgb(7, 242, 176));
                                });
                                self.gpu_mem_chart.ui(ui, "GPU Memory Usage", Color32::from_rgb(62, 7, 242));
                            });
                        });
                    },
                    ResourceMonitorState::Processes => {
                        self.process_table_viewer.show(ui);
                    },
                    ResourceMonitorState::Network => {
                        ui.group(|ui| {
                            // self.disk_usage_plot.ui(ui, "Disk I/O", &colors);
                            self.network_interface_plot.ui(ui, "Network Usage", &mut colors);
                        });
                    },
                    ResourceMonitorState::Temperatures => {
                        ui.group(|ui| {
                            self.component_temp_plot.ui(ui, "Temps", &mut colors);
                        });
                    },
                    ResourceMonitorState::Drives => {
                        ui.group(|ui| {
                            self.disk_usage_plot.ui(ui, "Drives", &mut colors);
                        });
                    }
                    _ => {},
                }
            });
        });
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
