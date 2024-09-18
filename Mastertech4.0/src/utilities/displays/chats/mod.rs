use eframe::egui::{
    epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TopBottomPanel, Ui, Widget
};
use surrealdb::RecordId;
use tokio::spawn;
use database::{DATABASE, schema::{Record, TaskNotePayload, User}};
use displays::markdown_editor::{EasyMarkEditor, SHORTCUT_ENTER};
// use crate::utilities::get_data::TaskNoteMod;
use chrono::{DateTime, Local};
use eframe::emath::Vec2;
use log::info;
use serde::Serialize;

use super::modals::ModalState;

#[derive(Debug, Clone, Serialize)]
pub struct ChatView{
    pub state: ModalState,
    pub title: String,
    pub messages: Vec<TaskNotePayload>,
    pub current_user: Option<User>,
    pub task_id: Option<RecordId>,
    #[serde(skip)]
    pub markdown_editor: EasyMarkEditor,
}

impl Default for ChatView{
    fn default() -> Self {
        Self { 
            state: ModalState::default(), 
            title: "Chat".to_string(), 
            messages: Vec::new(), 
            current_user: None, 
            markdown_editor: EasyMarkEditor::default(),
            task_id: None
        }
    }
}

impl ChatView {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: User, task_id: RecordId) -> Self {
        // info!("Before messages: {messages:?}");
        ChatView {
            current_user: Some(current_user),
            messages,
            state: ModalState::default(),
            title: "Chat".to_string(),
            markdown_editor: EasyMarkEditor::default(),
            task_id: Some(task_id)
        }
    }

    pub fn insert_note(&mut self, new_note: TaskNotePayload){
        let x = new_note.note.is_empty();
        let y = new_note.created_at.is_empty();
        let z = new_note.everest_initials.is_empty();
        info!("X {x} // Y {y} // Z {z}");
        if self.messages.iter().any(|note| {
            if let (Some(new_id), Some(existing_id)) = (new_note.id.as_ref(), note.id.as_ref()) {
                new_id.key().to_string() != existing_id.key().to_string() && !x && !y && !z
            } else { false }
        }) {
            info!("new_note {:?} // {:?}", new_note.everest_initials, new_note.created_at);
            self.messages.push(new_note);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<String>{
        
        let mut new_msg: Option<String> = None;

        let mut shadow = Shadow::default();
        shadow.blur = 10.0;
        shadow.spread = 5.0;
        shadow.color = Color32::from_rgb(40,36,40);

        let mut b_panel_marg = Margin::default();
        let mut c_panel_marg = Margin::default();
        c_panel_marg.top = 10.0;
        b_panel_marg.bottom = 10.0;
        let color = Color32::from_rgb(6,6,10);

        let markdown_editor = &mut self.markdown_editor;

        let central_panel_frame = Frame::none().fill(color)
            .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6.0)).rounding(Rounding::same(10.0));

        let bottom_panel_frame = Frame::none().fill(color)
            .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(c_panel_marg)
            .inner_margin(Margin::same(6.0)).rounding(Rounding::same(10.0));

        TopBottomPanel::bottom("ChatPageBottomPanel")
            .frame(bottom_panel_frame)
            .default_height(300.0)
            .resizable(true)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            let enter_pressed = ui.input_mut(|i| i.consume_shortcut(&SHORTCUT_ENTER));
            
            if let Some(response) = markdown_editor.ui(ui)
            {
                if response.clicked() || enter_pressed {
                    let txt = markdown_editor.submit();
                    info!("Txt: {txt}");
                    markdown_editor.clear();
                    new_msg = Some(txt.clone());

                    if let Some(usr) = self.current_user.clone(){
                        
                        let new_note = TaskNotePayload { everest_initials: usr.everest_initials, note: txt, task_id: self.task_id.clone(), ..Default::default() };

                        spawn(async move {
                            let query = format!("CREATE task_note CONTENT $note");
                            DATABASE.set("note", new_note).await.unwrap();
                            let update_task_note: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
                            info!("Update_note: {:?}", update_task_note);
                        });
                    }
                }
            }
        });

        CentralPanel::default()
            .frame(central_panel_frame)
            .show_inside(ui, |ui| 
        {
            ScrollArea::vertical()
                .animated(true)
                .max_height(ui.available_height())
                .max_width(f32::INFINITY)
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {

                let max_msg_width = ui.available_width() / 2.5;
                let fixed_height = 50.0;
                let min_width = 200.0;
                let other = min_width - 30.0;
                for item in self.messages.iter_mut(){
                    let mut is_message_from_myself = false;
                    if let Some(user) = &self.current_user{
                        is_message_from_myself = if item.everest_initials == user.everest_initials{
                            true
                        } else { false };
                    }

                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };

                    let msg_color = if is_message_from_myself {
                        ui.style().visuals.widgets.inactive.bg_fill
                    } else {
                        ui.style().visuals.widgets.active.weak_bg_fill
                    };

                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);

                        let rounding = 8.0;
                        let margin = 8.0;
                        
                        // ui.set_min_width(min_width);
                        let rnding = Rounding {
                            ne: if is_message_from_myself { 0.0 } else { rounding },
                            nw: if is_message_from_myself { rounding } else { 0.0 },
                            se: rounding,
                            sw: rounding,
                        };

                        let response = Frame::none()
                            .rounding(rnding)
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .show(ui, |ui| {
                                ui.set_min_height(fixed_height);  // Set the fixed height for the message box
                                ui.set_min_width(min_width / 2.5);
                                // Use a vertical layout to stack the name and message content
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {

                                    let mut shadow = Shadow::default();
                                    shadow.blur = 3.0;
                                    shadow.spread = 3.0;
                                    shadow.color = Color32::from_rgb(40,36,40);
                                    
                                    let mut b_panel_marg = Margin::default();
                                    b_panel_marg.top = 3.0;

                                    let color = Color32::from_rgb(10,10,12);

                                    let note_frame = Frame::none().fill(color)
                                        .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
                                        .inner_margin(Margin::symmetric(6.0, 10.0)).rounding(rnding);

                                    let from = RichText::new(&item.everest_initials).strong().monospace().color(Color32::LIGHT_BLUE);

                                    if is_message_from_myself {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::RightToLeft,
                                            Align::Min,
                                        ), |ui| {
                                            ui.add_space(8.0);
                                            Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30.0, 20.0)).sense(Sense::hover()).ui(ui);
                                            
                                            ui.add_space(20.0);
                                            let parsed_date = DateTime::parse_from_rfc3339(&item.created_at.clone())
                                                .unwrap_or_default()
                                                .with_timezone(&Local);
                                        
                                            let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                                            ui.label(RichText::new(formatted_date).weak());
                                            ui.add_space(15.0);
                                            ui.add_space(other);
                                            let btn = Button::new(RichText::new("X").small().weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                let _item = item.clone();
                                                spawn(async move {
                                                    // match item.delete_note().await{
                                                    //     Ok(_) => info!("Deleted Note"),
                                                    //     Err(e) =>  deleting note: {e:?}"),
                                                    // }
                                                });
                                            }
                                        });
                                        
                                    } else{
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::LeftToRight,
                                            Align::Min,
                                        ), |ui| {
                                            ui.add_space(8.0);
                                            Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30.0, 20.0)).sense(Sense::hover()).ui(ui);
                                            ui.add_space(35.0);
                                            let parsed_date = DateTime::parse_from_rfc3339(&item.created_at.clone())
                                                .unwrap_or_default()
                                                .with_timezone(&Local);
                                        
                                            let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                                            ui.label(RichText::new(formatted_date).small().weak());
                                            // ui.add_space(15.0);
                                        });
                                    }
                                    note_frame.show(ui, |ui| {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::TopDown,
                                            Align::Center,
                                        ), |ui| {
                                            ui.set_width(ui.available_width());
                                            displays::markdown_editor::viewer::easy_mark(ui, &item.note);
                                        });
                                    });
                                });
                            })
                            .response;

                        let points = if !is_message_from_myself {
                            let top = response.rect.left_top() + Vec2::splat(margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(-rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.right_bottom(),
                            ]
                        } else {
                            let top = response.rect.right_top() + Vec2::new(-margin, margin);
                            let arrow_rect =
                                Rect::from_two_pos(top, top + Vec2::new(rounding, rounding));

                            vec![
                                arrow_rect.left_top(),
                                arrow_rect.right_top(),
                                arrow_rect.left_bottom(),
                            ]
                        };

                        ui.painter()
                            .add(Shape::convex_polygon(points, msg_color, Stroke::NONE));

                    });
                };
            });
        });

        new_msg
    }
}



