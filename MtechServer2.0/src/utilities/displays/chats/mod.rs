use std::collections::{BTreeSet, HashMap, HashSet};

use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Widget
};
use database::{live_data::handle_live_delete, schema::{get_data::TaskNoteMod, helper_traits::TaskNotePayloadHelper, Record, TaskNotePayload, User}, DATABASE};
use displays::markdown_editor::{viewer, EasyMarkEditor, SHORTCUT_ENTER};
use regex::Regex;
use surrealdb::RecordId;
use wasm_bindgen_futures::spawn_local;
use chrono::{DateTime, Local, Utc};
use eframe::emath::Vec2;
use serde::Serialize;
use log::{error, info};
use structdiff::StructDiff;
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
    pub delete: Option<TaskNotePayload>,
    pub users: BTreeSet<String>,
    pub edit_text: HashMap<String, TaskNotePayload>,
    pub allow_edit: HashSet<String>
}

impl Default for ChatView{
    fn default() -> Self {
        Self { 
            state: ModalState::default(), 
            title: "Chat".to_string(), 
            messages: Vec::new(), 
            current_user: None, 
            markdown_editor: EasyMarkEditor::default(),
            task_id: None,
            delete: None,
            users: BTreeSet::new(),
            edit_text: HashMap::new(),
            allow_edit: HashSet::new(),
            // save_edit: false,
        }
    }
}

impl ChatView {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: User, task_id: RecordId, users: Vec<User>) -> Self {
        // info!("Before messages: {messages:?}");
        let mut users_set = BTreeSet::new();
        for user in users {
            let parsed_email = user.email.split_once('@');
            if let Some(email) = parsed_email {
                users_set.insert(format!("@{}", email.0));
            }
        }
        let mut note_ids = HashMap::new();

        for message in messages.iter() {
            note_ids.insert(message.id.to_string(), message.clone());
        }

        ChatView {
            current_user: Some(current_user),
            messages,
            state: ModalState::default(),
            title: "Chat".to_string(),
            markdown_editor: EasyMarkEditor::new(),
            task_id: Some(task_id),
            delete: None,
            users: users_set,
            edit_text: note_ids,
            allow_edit: HashSet::new()
        }
    }

    pub fn insert_note(&mut self, new_note: &mut TaskNotePayload){
        if let Some(existing_note) = self.messages.iter_mut().find(|n| n.id == new_note.id .clone()) {
            // Apply diffs to the existing note
            let diffs = existing_note.diff(&new_note);
            existing_note.apply_mut(diffs);
            info!("Updated existing note: {:?}", existing_note);
        } else {
            self.messages.push(new_note.clone());
        }
    }

    pub fn delete_note(&mut self, note_to_delete: &TaskNotePayload){
        let index = self.messages.iter().position(|n| n == note_to_delete);
        if let Some(idx) = index {
            info!("Deleting Note @ {idx}");
            self.messages.remove(idx);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<String>{
        let mut new_msg: Option<String> = None;

        if let Some(note) = std::mem::take(&mut self.delete) {
            let deletion = handle_live_delete(&mut self.messages, note.clone());
            if let Err(e) = deletion {
                error!("Error deleting note: {e:?}");
            }
        }
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
        markdown_editor.inputs = self.users.clone();
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
                        let username = usr.email.split_once('@').map_or_else(String::new, |(name, _)| name.to_string());

                        // Extract the first customer thread ID if available
                        let id_customer_thread = self
                            .messages
                            .iter()
                            .filter_map(|m| m.id_customer_thread.clone())
                            .next();

                        let employee_id = usr.id_prestashop.clone().unwrap_or_default();
                        let id_employee = Some(employee_id.to_string());
                        let mut new_note = TaskNotePayload {
                            everest_initials: usr.everest_initials, 
                            note: txt, 
                            task_id: self.task_id.clone(), 
                            username,
                            user: Some(usr.id),
                            id_employee,
                            id_customer_thread,
                            ..Default::default() 
                        };

                        // If there are multiple threads, assign each as needed (retain only the last)
                        for thread in self.messages.iter().filter_map(|m| m.id_customer_thread.clone()) {
                            new_note.id_customer_thread = Some(thread);
                        }
                        info!("new_note: {new_note:?}");
                        spawn_local(async move {
                            if let Err(e) = new_note.create_task_note().await {
                                error!("Failed to create task note: {:?}", e);
                            } else {
                                info!("Task note successfully created.");
                            }
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
                self.messages.sort_by_key(|message| 
                    DateTime::parse_from_rfc3339(&message.created_at.clone())
                        .unwrap_or_default()
                        .with_timezone(&Utc)
                    );
                for item in self.messages.iter_mut(){
                    let mut is_message_from_myself = false;
                    if let Some(user) = &self.current_user{
                        let email = user.email.split_once('@').clone();
                        let username = email.unwrap_or_default().0.to_string();
                        is_message_from_myself = if item.username == username {
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

                                    let from = RichText::new(&item.username).strong().monospace().color(Color32::LIGHT_BLUE);

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
                                            ui.add_space(20.0);
                                            ui.add_space(other);

                                            let id = &item.id;

                                            if self.allow_edit.contains(&id.to_string()) {
                                                let save_btn = Button::new(RichText::new("Save").weak().color(Color32::LIGHT_RED))
                                                    .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                                if save_btn.clicked(){
                                                    if self.allow_edit.contains(&id.to_string()) {
                                                        if let Some(msg) = self.edit_text.get_mut(&id.to_string()){
                                                            let note = msg.note.clone();
                                                            let id = id.clone();
                                                            spawn_local(async move {
                                                                DATABASE.set("note", note).await.unwrap();
                                                                DATABASE.set("id", id).await.unwrap();
                                                                let update_task_note: Vec<Record> = DATABASE.query("UPDATE task_note SET note = $note WHERE id == $id").await.unwrap().take(0).unwrap();
                                                                info!("Update_note: {:?}", update_task_note);
                                                            });
                                                            item.note = msg.note.clone();
                                                        }
                                                    }
                                                    self.allow_edit.remove(&id.to_string());
                                                }
                                            } else {
                                                let edit_btn = Button::new(RichText::new("🖊").weak().color(Color32::LIGHT_RED))
                                                    .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                                if edit_btn.clicked(){
                                                    self.allow_edit.insert(item.id.to_string()); 
                                                }

                                            }


                                            let copy_btn = Button::new(RichText::new("🗐").weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if copy_btn.clicked(){
                                                ui.ctx().copy_text(item.note.clone());
                                            }

                                            ui.add_space(6.0);

                                            let btn = Button::new(RichText::new("🗙").weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if btn.clicked(){
                                                self.delete = Some(item.clone());
                                                let mut item = item.clone();
                                                spawn_local(async move {
                                                    match item.delete_note().await{
                                                        Ok(_) => info!("Deleted Note"),
                                                        Err(e) => error!("Error deleting note: {e:?}"),
                                                    }
                                                })
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
                                            ui.label(RichText::new(formatted_date).weak());
                                            
                                            ui.add_space(10.0);
                                            let copy_btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .rounding(Rounding::same(f32::INFINITY)).small().min_size(Vec2::new(30.0, 14.0)).ui(ui);

                                            if copy_btn.clicked(){
                                                ui.ctx().copy_text(item.note.clone());
                                            }
                                        });
                                    }
                                    note_frame.show(ui, |ui| {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::TopDown,
                                            Align::Center,
                                        ), |ui| {
                                            ui.set_width(ui.available_width());
                                            
                                            if self.allow_edit.contains(&item.id.to_string()) {
                                                if let Some(msg) = self.edit_text.get_mut(&item.id.to_string()){
                                                    TextEdit::multiline(&mut msg.note).show(ui);
                                                }
                                            } else {
                                                viewer::easy_mark(ui, &item.note);
                                            }
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



