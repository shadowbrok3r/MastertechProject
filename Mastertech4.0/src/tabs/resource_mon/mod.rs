use eframe::egui::{Align, CentralPanel, Color32, Layout, NumExt, Response, RichText, ScrollArea, TextStyle, TopBottomPanel, Ui, Vec2b};
use egui_plot::{CoordinatesFormatter, Corner, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, VPlacement};
use displays::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use std::collections::{HashMap, VecDeque};
use database::schema::SystemInformation;
use tokio::spawn;
use log::info;

use crate::filesystem::system_info::get_sysinfo;

pub struct ResourceMonitor {
    sysinfo_channel: (Sender<SystemInformation>, Receiver<SystemInformation>),
    cpu_clock: VecDeque<f32>,
    temps: VecDeque<HashMap<String, f32>>,
    cpu_percentage: VecDeque<f32>,
    ram_usage: VecDeque<f32>,
    line_plot: LinePlot,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        let sysinfo_channel = SystemInformation::create_unbounded_channel();
        let mut line_plot = LinePlot::new();
        line_plot.add_line("CPU Usage");
        line_plot.add_line("CPU Clock");
        line_plot.add_line("RAM Usage");
        Self {
            sysinfo_channel,
            cpu_clock: Default::default(),
            temps: Default::default(),
            cpu_percentage: Default::default(),
            ram_usage: Default::default(),
            line_plot,
        }
    }
}

impl ResourceMonitor {
    fn receive(&mut self) {
        if let Ok(sysinfo) = self.sysinfo_channel.1.try_recv() {
            let normalized_cpu_clock = normalize(sysinfo.cpu_clock, 0.0, 100.0); // Example range for CPU clock
            // let normalized_cpu_percentage = normalize(sysinfo.cpu_percentage, 0.0, 100.0);
            let total_ram = if sysinfo.total_memory > 0.0 { (sysinfo.used_memory / sysinfo.total_memory)*100.0 } else { 0.0 };
            // let normalized_ram_usage = normalize(total_ram, 0.0, 100.0); // Example range for RAM usage
            self.cpu_percentage.push_back(sysinfo.cpu_percentage);
            self.cpu_clock.push_back(normalized_cpu_clock);
            self.ram_usage.push_back(total_ram);
                    // let normalized_temps: Vec<f32> = sysinfo.component_temps.values().map(|&temp| normalize(temp, 0.0, 100.0)).collect();

                if self.cpu_percentage.len() < 30
                    || self.cpu_clock.len() < 30
                    || self.ram_usage.len() < 30 
                {

                    // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                } else {
                    self.cpu_percentage.pop_front();
                    self.cpu_percentage.push_back(sysinfo.cpu_percentage);
                    self.cpu_clock.pop_front();
                    self.cpu_clock.push_back(sysinfo.cpu_clock);
                    self.ram_usage.pop_front();
                    self.ram_usage.push_back(sysinfo.used_memory);
                    // self.component_temps.pop_front();
                    // self.component_temps.push_back(normalized_temps.iter().sum::<f32>() / normalized_temps.len() as f32); // Average temperature
                }

                if self.cpu_percentage.len() > 50
                    || self.cpu_clock.len() > 50
                    || self.ram_usage.len() > 50 {
                    // || self.component_temps.len() < 30 
                    self.cpu_percentage.clear();
                    self.cpu_clock.clear();
                    self.ram_usage.clear();
                }
                // self.line_plot.update_data();

                self.line_plot.update_line("CPU Usage", self.cpu_percentage.len() as f32, sysinfo.cpu_percentage);
                self.line_plot.update_line("CPU Clock", self.cpu_clock.len() as f32, normalize(sysinfo.cpu_clock, 0.0, 100.0));
                self.line_plot.update_line(
                    "RAM Usage",
                    self.ram_usage.len() as f32,
                    if sysinfo.total_memory > 0.0 {
                        (sysinfo.used_memory / sysinfo.total_memory) * 100.0
                    } else {
                        0.0
                    },
                );
                // self.cpu_clock_plot.update_data(x_value, y_value);
                // self.ram_usage_plot.update_data(x_value, y_value);
                // info!("\nsysinfo: CPU %: {percentages:?}, \nCPU Clock: {clocks:?}, \nRAM usage: {ram:?}");
                // let temps_plot = LinePlot::new(&[0.0], &temps.as_slice());


        }
    }

    pub fn display(&mut self, ui: &mut Ui) {
        self.receive();
        TopBottomPanel::top("Resource Monitor Top Panel").exact_height(25.).show_inside(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(
                        RichText::new("Resource monitor")
                            .color(Color32::LIGHT_RED)
                    );
                });
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
                // self.timeout_counter.elapsed().as_secs()
                // let percentages = self.cpu_percentage.make_contiguous().to_owned();
                // let clocks = self.cpu_clock.make_contiguous().to_owned();
                // // let temps = self.component_temps.make_contiguous().to_owned();
                // let ram = self.ram_usage.make_contiguous().to_owned();
                let mut colors = HashMap::new();
                colors.insert("CPU Usage".to_string(), Color32::GREEN);
                colors.insert("CPU Clock".to_string(), Color32::BLUE);
                colors.insert("RAM Usage".to_string(), Color32::RED);
                self.line_plot.ui(ui, "System Metrics", &colors);
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

fn normalize(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}


async fn live_computer_stats(tx: Sender<SystemInformation>) -> anyhow::Result<(), anyhow::Error>{
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        tx.send(get_sysinfo().await?)?;
    }
    #[allow(unreachable_code)]
    Ok(())
}


const MINS_PER_DAY: f64 = 24.0 * 60.0;
const MINS_PER_H: f64 = 60.0;

#[derive(Clone, PartialEq, Default)]
pub struct LinePlot {
    animate: bool,
    time: f64,
    square: bool,
    proportional: bool,
    coordinates: bool,
    data: HashMap<String, VecDeque<(f32, f32)>>, // Map line names to VecDeque of (x, y) points
}

impl LinePlot {
    pub fn new() -> Self {
        Self {
            animate: true,
            time: 0.0,
            square: true,
            proportional: false,
            coordinates: true,
            data: HashMap::new(),
        }
    }

    pub fn add_line(&mut self, name: &str) {
        self.data.entry(name.to_string()).or_insert_with(VecDeque::new);
    }

    // pub fn line(&self, name: &str, color: Color32) -> Line {
    //     let points: Vec<[f64; 2]> = self.x_values
    //         .iter()
    //         .zip(&self.y_values)
    //         .map(|(&x, &y)| [x as f64, y as f64])
    //         .collect();
    //     Line::new(PlotPoints::new(points))
    //         .color(color)
    //         .style(LineStyle::Solid)
    //         .name(name)
    // }
    

    pub fn update_line(&mut self, name: &str, x_value: f32, y_value: f32) {
        const MAX_POINTS: usize = 50;
        if let Some(points) = self.data.get_mut(name) {
            info!("points: {points:?}");
            if points.len() >= MAX_POINTS {
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
                let plot_points: Vec<[f64; 2]> = points.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
                Some(
                    Line::new(PlotPoints::new(plot_points))
                        .color(*color)
                        .name(name),
                )
            })
            .collect()
    }

    pub fn ui(&mut self, ui: &mut Ui, plot_name: &str, colors: &HashMap<String, Color32>) -> Response {
        if self.animate {
            ui.ctx().request_repaint();
            self.time += 0.016;
            if self.time > 60.0 {
                self.time = 0.0;
            }
        }

        let mut plot = Plot::new(plot_name)
            .legend(Legend::default())
            .width(ui.available_width())
            .height(200.0)
            .show_axes(true)
            .show_grid(true);

        if self.square {
            plot = plot.view_aspect(1.0);
        }
        if self.proportional {
            plot = plot.data_aspect(1.0);
        }
        if self.coordinates {
            plot = plot.coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::default());
        }

        plot.show(ui, |plot_ui| {
            for line in self.lines(colors) {
                plot_ui.line(line);
            }
        })
        .response
    }
}

fn hour(x: f64) -> f64 {
    (x.rem_euclid(MINS_PER_DAY) / MINS_PER_H).floor()
}

fn minute(x: f64) -> f64 {
    x.rem_euclid(MINS_PER_H).floor()
}

fn percent(y: f64) -> f64 {
    100.0 * y
}