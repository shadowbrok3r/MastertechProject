use eframe::egui::{Align, CentralPanel, Color32, FontId, Layout, Response, RichText, ScrollArea, Separator, TextStyle, TopBottomPanel, Ui, Widget};
use egui_plot::{Bar, BarChart, Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use displays::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use std::{collections::{HashMap, VecDeque}, time::Instant};
use database::schema::SystemInformation;
use tokio::spawn;
use log::info;

use crate::filesystem::system_info::get_sysinfo;

pub struct ResourceMonitor {
    sysinfo_channel: (Sender<SystemInformation>, Receiver<SystemInformation>),
    cpu_usage_chart: MetricPlot,
    cpu_clock_chart: MetricPlot,
    ram_usage_chart: MetricPlot,
    component_temp_plot: LinePlot,
    disk_usage_plot: LinePlot,
    network_interface_plot: LinePlot,
    start_time: Instant,
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
        }
    }
}

impl ResourceMonitor {
    fn receive(&mut self) {
        if let Ok(sysinfo) = self.sysinfo_channel.1.try_recv() {
            // self.line_plot.update_metric("CPU Usage", self.cpu_percentage.len() as f32, sysinfo.cpu_percentage);
            // self.line_plot.update_metric("CPU Clock", self.cpu_clock.len() as f32, normalize(sysinfo.cpu_clock, 0.0, 100.0));
            // self.line_plot.update_metric("RAM Usage", self.ram_usage.len() as f32,
            //     if sysinfo.total_memory > 0.0 {
            //         (sysinfo.used_memory / sysinfo.total_memory) * 100.0
            //     } else {
            //         0.0
            //     },
            // );
            // self.cpu_clock_plot.update_data(x_value, y_value);
            // self.ram_usage_plot.update_data(x_value, y_value);
            // info!("\nsysinfo: CPU %: {percentages:?}, \nCPU Clock: {clocks:?}, \nRAM usage: {ram:?}");
            // let temps_plot = LinePlot::new(&[0.0], &temps.as_slice());
            // let x_value = self.cpu_usage_chart.data.len() as f32;
            // self.cpu_usage_chart.update(x_value, sysinfo.cpu_percentage);
            // self.cpu_clock_chart
            //     .update(x_value, normalize(sysinfo.cpu_clock, 0.0, 100.0));
            // self.ram_usage_chart.update(
            //     x_value,
            //     if sysinfo.total_memory > 0.0 {
            //         (sysinfo.used_memory / sysinfo.total_memory) * 100.0
            //     } else {
            //         0.0
            //     },
            // );
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
            for (interface, data) in &sysinfo.network_interfaces {
                if let Some((rx_bytes, tx_bytes)) = parse_network_data(data) {
                    let rx_gb = rx_bytes as f32 / 1e9;
                    let tx_gb = tx_bytes as f32 / 1e9;
                    self.network_interface_plot
                        .update_line(interface, elapsed_time, rx_gb + tx_gb);
                }
            }
            // let x_value = self.cpu_usage_chart.data.values().next().map_or(0.0, |v| v.len() as f32 * 4.0);
            // self.cpu_usage_chart.update_metric("CPU Usage (%)", x_value, sysinfo.cpu_percentage);
            // self.cpu_clock_chart.update_metric("CPU Clock (MHz)", x_value, normalize(sysinfo.cpu_clock, 0.0, 4000.0)); // Normalize MHz
            // self.ram_usage_chart.update_metric("Ram Usage (Gb)",x_value,sysinfo.used_memory / 1024.0); // Convert MB to GB
        }
    }

    pub fn display(&mut self, ui: &mut Ui) {
        self.receive();

        ui.ctx().request_repaint_after_secs(2.);
        TopBottomPanel::top("Resource Monitor Top Panel").exact_height(25.).show_inside(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.heading(
                    RichText::new("Resource monitor")
                        .color(Color32::LIGHT_RED)
                );
                ui.add_space(5.);
                if ui.button("Refresh").clicked() {
                    let tx = self.sysinfo_channel.0.clone();
                    spawn(async move {
                        let res = live_computer_stats(tx).await; 
                        log::info!("Getting live sys stats: {res:?}");
                    });
                }
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(10.);
                ui.vertical_centered(|ui| ui
                    .label(
                        RichText::new(format!("CPU ()"))
                        .color(
                            ui.style().visuals.error_fg_color
                        )
                        .heading()
                        .font(
                            FontId::proportional(15.)
                        )
                    )
                );

                ui.add_space(10.);
                ui.horizontal_top(|ui| {
                    ui.group(|ui| {
                        self.cpu_clock_chart.ui(ui, "CPU Clock Chart", Color32::from_rgb(7, 242, 176));
                        self.cpu_usage_chart.ui(ui, "CPU Usage Chart", Color32::from_rgb(62, 7, 242));
                    });
                });

                ui.add_space(10.);
                ui.vertical_centered(|ui| ui
                    .label(
                        RichText::new("RAM")
                        .color(
                            ui.style().visuals.error_fg_color
                        )
                        .heading()
                        .font(
                            FontId::proportional(15.)
                        )
                    )
                );

                ui.add_space(10.);
                ui.horizontal_top(|ui| {
                    ui.group(|ui| {
                        self.ram_usage_chart.ui(ui, "RAM Usage Chart", Color32::from_rgb(242, 7, 179));
                    });
                });

                ui.add_space(10.);
                ui.horizontal_top(|ui| {
                    ui.group(|ui| {
                                    // New line charts
                        let mut colors = HashMap::new();
                        colors.insert("Component Temps".to_string(), Color32::from_rgb(235, 12, 38));
                        colors.insert("Disk Usage".to_string(), Color32::from_rgb(12, 235, 97));
                        colors.insert("Network Usage".to_string(), Color32::from_rgb(240, 141, 55));
                        self.component_temp_plot.ui(ui, "Temps", &colors);
                        self.disk_usage_plot.ui(ui, "Disk I/O", &colors);
                        self.network_interface_plot.ui(ui, "Network Usage", &colors);
                    });
                });



                // let processes = sys.processes();
                // ui.horizontal(|ui| {
                //     for (pid, process) in processes.iter() {
                //         ui.label(
                //             RichText::new(format!("{}", pid)).font(
                //                 FontId {
                //                     size: 18.1,
                //                     family: FontFamily::Monospace,
                //                 },
                //             ),
                //         );
                //         ui.label(
                //             RichText::new(format!("{:?}", process)).font(
                //                 FontId {
                //                     size: 18.1,
                //                     family: FontFamily::Monospace,
                //                 },
                //             ),
                //         );
                //         ui.vertical(|ui| {
                //             if let Some(process) = sys.process(*pid) {
                //                 if let Some(tasks) = process.tasks() {
                //                     ui.label(
                //                         RichText::new(format!("Listing tasks for process {:?}", process.pid())).font(
                //                             FontId {
                //                                 size: 16.1,
                //                                 family: FontFamily::Monospace,
                //                             },
                //                         ),
                //                     );
                //                     for task_pid in tasks {
                //                         if let Some(task) = s.process(*task_pid) {
                //                             println!("Task {:?}: {:?}", task.pid(), task.name());
                //                         }
                //                     }
                //                 }
                //             }
                //         });
                //     }
                // });
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
            .legend(Legend::default())
            .width(400.)
            .height(200.0)
            .show_axes(true)
            .show_grid(true);

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
            .width(300.)
            .legend(Legend::default())
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
            .width(500.)
            .height(250.)
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

fn parse_network_data(data: &str) -> Option<(u64, u64)> {
    let parts: Vec<&str> = data.split('/').collect();
    if parts.len() == 2 {
        let rx = parts[0].parse::<u64>().ok()?;
        let tx = parts[1].parse::<u64>().ok()?;
        return Some((rx, tx));
    }
    None
}


async fn live_computer_stats(tx: Sender<SystemInformation>) -> anyhow::Result<(), anyhow::Error>{
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        tx.send(get_sysinfo().await?)?;
    }
    #[allow(unreachable_code)]
    Ok(())
}
