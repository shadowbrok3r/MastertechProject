use eframe::egui::{collapsing_header::CollapsingState, Align, Button, CollapsingHeader, Color32, Grid, Layout, RadioButton, Ui, Vec2, Widget};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};

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

impl TaskLayout{
    pub fn task_card(task_data: Vec<TaskPayload>, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error> {

        let mut selected = false;
    
        TableBuilder::new(ui)
            .column(Column::remainder().resizable(true))
            .column(Column::remainder().resizable(true))
            .header(20.0, |mut header| 
        {
            header.col(|ui| {
                ui.heading("Task Name");
            });
            header.col(|ui| {
                ui.heading("Due");
            });
        }).body(|mut body| 
        {
            for task_data in task_data.iter(){
                body.row(30.0, |mut row| {
                    row.col(|ui| {
                        ui.toggle_value(&mut selected, &task_data.task_name);
                    });
                    row.col(|ui| {
                        ui.label(&task_data.due_date);
                    });
                });
            }
        });
    
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