use eframe::egui::{RichText, Ui,};
use egui::{vec2, Color32, Layout, Margin, Rect, Sense, Slider, Stroke, Vec2};
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
            size: vec2(200.0, 200.0),
            rounding: 5.0,
            stroke_width: 2.0,
        }
    }
}


impl BoxPainting {
    pub fn ui(&mut self, ui: &mut Ui, tasks: &Vec<TaskPayload>) {
        ui.vertical(|ui| {
            
            for task_data in tasks {
                let (mut rect, response) = ui.allocate_exact_size(self.size, Sense::hover());
                rect.set_height(200.0);
                rect.set_width(400.0);

                ui.allocate_ui_at_rect(rect, |ui|{
                    
                    egui::Frame::default()
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .rounding(ui.visuals().widgets.noninteractive.rounding)
                        .fill(ui.visuals().extreme_bg_color)
                        .inner_margin(Margin::symmetric(4.0, 2.0))
                        .show(ui, |ui| 
                    {
                        
                        ui.expand_to_include_rect(rect);
                        ui.horizontal(|ui| {
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
                        ui.horizontal(|ui| {
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