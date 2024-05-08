use eframe::egui::{collapsing_header::CollapsingState, Align, CollapsingHeader, Layout, Ui};
use egui_extras::{Column, TableBuilder};

use crate::database::schema::TaskPayload;



pub fn task_card(task_data: Vec<TaskPayload>, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error>{

    let header_id = ui.make_persistent_id("my_collapsing_header");

    let mut stuff = false;
    CollapsingState::load_with_default_open(ui.ctx(), header_id, true)
    .show_header(ui, |ui| {
        ui.toggle_value(&mut stuff, "Click to select/unselect");
        ui.radio_value(&mut stuff, false, "");
        ui.radio_value(&mut stuff, true, "");
    })
    .body(|ui| {
        ui.label("The body is always custom");
    });

    CollapsingHeader::new("Normal collapsing header for comparison").show(ui, |ui| {
        ui.label("Nothing exciting here");
    });

    ui.push_id("tasks",|ui|{
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(100.0).range(50.0..=300.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        table
        .header(20.0, |mut header|{
            header.col(|ui| {
                ui.strong("Task ID");
            });
            header.col(|ui| {
                ui.strong("Task Name");
            });
        }).body(|mut body| {
            for task_data in task_data.iter(){
                
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label(format!("{}", &task_data.id.clone().unwrap().0.id));
                    });
                    row.col(|ui|{
                        ui.label(format!("{}", &task_data.task_name));
                    });
                });
            }
            
        });
    });
    Ok(())
}