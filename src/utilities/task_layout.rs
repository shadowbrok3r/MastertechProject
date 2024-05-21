use eframe::egui::{RichText, Ui,};
use egui::{vec2, Color32, Layout, Margin, Sense, Slider, Stroke, Vec2};
use egui_extras::{Column,TableBuilder};

use crate::database::schema::TaskPayload;

pub struct TaskLayout{
    selected: bool
}

impl Default for TaskLayout{
    fn default() -> Self {
        Self {
            selected: false
        }
    }
}
struct BoxPainting {
    size: Vec2,
    rounding: f32,
    stroke_width: f32,
}


impl TaskLayout{
    pub fn new(selected: bool) -> Self { Self { selected } }

    pub fn task_card(&self, task_data: &Vec<TaskPayload>, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error> {
    
        // TableBuilder::new(ui)
        //     .column(Column::remainder().resizable(true))
        //     .column(Column::remainder().resizable(true))
        //     .header(20.0, |mut header| 
        // {
        //     header.col(|ui| {
        //         ui.heading("Task Name");
        //     });
        //     header.col(|ui| {
        //         ui.heading("Due");
        //     });
        // }).body(|mut body| 
        // {
        //     for task_data in task_data.iter(){
        //         body.row(30.0, |mut row| {
        //             row.col(|ui| {
        //                 let _x = ui.selectable_label(self.selected, 
        //                     RichText::new(&task_data.task_name).small().size(12.0)
        //                 );
        //             });
        //             row.col(|ui| {
        //                 ui.label(&task_data.due_date);
        //             });
        //         });
        //     }
        // });

        let mut boxes = BoxPainting::default();
        boxes.ui(ui, &task_data);
    
/* 
    let header_id = ui.make_persistent_id(&task_data.task_name);
    CollapsingState::load_with_default_open(ui.ctx(), header_id, false)
    .show_header(ui, |ui| {
        ui.toggle_value(&mut stuff, &task_data.task_name);
    })
    .body_unindented(|ui| {
        ui.label("The body is always custom");
    });
*/
        Ok(())
    }
}


impl Default for BoxPainting {
    fn default() -> Self {
        Self {
            size: vec2(100.0, 300.0),
            rounding: 5.0,
            stroke_width: 2.0,
        }
    }
}

// #[derive(PartialEq)]
// pub struct FrameDemo {
//     frame: egui::Frame,
// }

// impl Default for FrameDemo {
//     fn default() -> Self {
//         Self {
//             frame: egui::Frame {
//                 inner_margin: 12.0.into(),
//                 outer_margin: 24.0.into(),
//                 rounding: 14.0.into(),
//                 shadow: egui::Shadow {
//                     offset: [8.0, 12.0].into(),
//                     blur: 16.0,
//                     spread: 0.0,
//                     color: egui::Color32::from_black_alpha(180),
//                 },
//                 fill: egui::Color32::from_rgba_unmultiplied(97, 0, 255, 128),
//                 stroke: egui::Stroke::new(1.0, egui::Color32::GRAY),
//             },
//         }
//     }
// }


impl BoxPainting {
    pub fn ui(&mut self, ui: &mut Ui, tasks: &Vec<TaskPayload>) {
        ui.horizontal_wrapped(|ui| {
            for task_data in tasks {
                let (rect, _response) = ui.allocate_at_least(self.size, Sense::hover());
                // ui.painter()
                // .rect(
                    // rect,self.rounding,ui.visuals().faint_bg_color().gamma_multiply(0.5),Stroke::new(self.stroke_width, Color32::WHITE),
                // );ui.allocate_space(self.size);

                egui::Frame::default()
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .rounding(ui.visuals().widgets.noninteractive.rounding)
                    .fill(ui.visuals().extreme_bg_color)
                    .inner_margin(Margin::symmetric(4.0, 2.0))
                    .show(ui, |ui| {
                        ui.set_min_size(self.size);
                        ui.with_layout(Layout::top_down_justified(egui::Align::Center), |ui| {
                            ui.vertical_centered(|ui| {
                                ui.horizontal_top(|ui| {
                                    ui.label(egui::RichText::new(&task_data.assignee_initials.clone().unwrap_or("".to_string())).color(egui::Color32::WHITE));
                                    ui.label(egui::RichText::new(&task_data.task_name).color(egui::Color32::WHITE));
                                    if task_data.completed{
                                        let _ = ui.selectable_label(
                                            false,
                                            egui::RichText::new("✔️").color(egui::Color32::WHITE).background_color(Color32::LIGHT_GREEN)
                                        );
                                    }else{
                                        let _ = ui.selectable_label(
                                            false,
                                            egui::RichText::new("✖️").color(egui::Color32::WHITE).background_color(Color32::LIGHT_RED)
                                        );
                                    }
                                });
                            });
                            ui.horizontal_top(|ui| {
                                ui.label(egui::RichText::new(&task_data.due_date).color(egui::Color32::WHITE));
                                ui.label(egui::RichText::new(format!("{:?}", &task_data.status)).color(egui::Color32::WHITE));
                                ui.label(egui::RichText::new(format!("{:?}", &task_data.priority)).color(egui::Color32::WHITE));
                            });
                        });   
                    });
            }
        });
    }
}