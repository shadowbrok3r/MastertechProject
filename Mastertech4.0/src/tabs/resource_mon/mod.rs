use std::collections::VecDeque;

use eframe::egui::{Align, CentralPanel, Color32, FontFamily, FontId, Layout, RichText, ScrollArea, Ui};
use sysinfo::{Components, CpuRefreshKind, RefreshKind, System};

use crate::app_state::MastertechContext;

impl MastertechContext {
    pub fn resource_monitor(&mut self, ui: &mut Ui) {
        CentralPanel::default().show_inside(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {

                let sys = System::new_all();
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.heading(
                        RichText::new("Resource monitor")
                            .color(Color32::LIGHT_RED)
                            .font(FontId::monospace(28.5)),
                    );
                });

                ui.separator();

                ui.label(
                    RichText::new("CPU")
                        .color(Color32::GREEN)
                        .font(FontId::monospace(20.0)),
                );

                let mut live = VecDeque::new();
                let mut s = System::new_with_specifics(
                    RefreshKind::new().with_cpu(CpuRefreshKind::everything()),
                );
                // Wait a bit because CPU usage is based on diff.
                std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
                // Refresh CPUs again to get actual value.
                s.refresh_cpu_usage();
                live.push_front(s.global_cpu_usage());


                ui.label(
                    RichText::new(format!("printing values from cpu load\t{:?}", live))
                        .font(FontId::monospace(18.1)),
                );

                ui.separator();

                ui.label(
                    RichText::new("MEMORY")
                        .color(Color32::GREEN)
                        .font(FontId::monospace(20.0)),
                );


                ui.label(
                    RichText::new(format!(
                        "\nmemory {} used/{}",
                        sys.total_memory(),
                        sys.used_memory(),
                    ))
                    .font(FontId {
                        size: 18.1,
                        family: FontFamily::Monospace,
                    }),
                );


                ui.separator();

                ui.label(
                    RichText::new("AVERAGE_LOAD")
                        .color(Color32::GREEN)
                        .font(FontId::monospace(20.0)),
                );

                let load_avg = System::load_average();

                ui.label(
                    RichText::new(format!(
                        "\nload average {} {} {}",
                        load_avg.one, load_avg.five, load_avg.fifteen
                    ))
                    .font(FontId {
                        size: 18.1,
                        family: FontFamily::Monospace,
                    }),
                );


                ui.separator();

                ui.label(
                    RichText::new("TEMPERATURES")
                        .color(Color32::GREEN)
                        .font(FontId::monospace(20.0)),
                );

                let components = Components::new_with_refreshed_list();
                ui.horizontal(|ui| {
                    for component in components.iter() {
                        ui.label(
                            RichText::new(component.label()).font(
                                FontId {
                                    size: 18.1,
                                    family: FontFamily::Monospace,
                                },
                            ),
                        );
                        ui.label(
                            RichText::new(format!("{}°C", component.temperature())).font(
                                FontId {
                                    size: 18.1,
                                    family: FontFamily::Monospace,
                                },
                            ),
                        );
                    }
                });

                let processes = sys.processes();
                ui.horizontal(|ui| {
                    for (pid, process) in processes.iter() {
                        ui.label(
                            RichText::new(format!("{}", pid)).font(
                                FontId {
                                    size: 18.1,
                                    family: FontFamily::Monospace,
                                },
                            ),
                        );
                        ui.label(
                            RichText::new(format!("{:?}", process)).font(
                                FontId {
                                    size: 18.1,
                                    family: FontFamily::Monospace,
                                },
                            ),
                        );
                        ui.vertical(|ui| {
                            if let Some(process) = sys.process(*pid) {
                                if let Some(tasks) = process.tasks() {
                                    ui.label(
                                        RichText::new(format!("Listing tasks for process {:?}", process.pid())).font(
                                            FontId {
                                                size: 16.1,
                                                family: FontFamily::Monospace,
                                            },
                                        ),
                                    );
                                    for task_pid in tasks {
                                        if let Some(task) = s.process(*task_pid) {
                                            println!("Task {:?}: {:?}", task.pid(), task.name());
                                        }
                                    }
                                }
                            }
                        });
                    }
                });
            });
        });
    }
}