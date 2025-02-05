use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Layout, Margin, Rect, RichText, CornerRadius, ScrollArea, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Widget};
use database::{live_data::handle_live_delete, schema::{helper_traits::TaskNotePayloadHelper, TaskNotePayload, User}};
use surrealdb::RecordId;
use super::markdown_editor::{viewer, EasyMarkEditor, SHORTCUT_ENTER};
use std::collections::{BTreeSet, HashMap, HashSet};
use crate::{PlatformSpawner, Spawner};
use chrono::{DateTime, Local, Utc};
use structdiff::StructDiff;
use eframe::emath::Vec2;
use log::{error, info};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ChatView{
    pub title: String,
    pub messages: Vec<TaskNotePayload>,
    pub current_user: Option<User>,
    #[serde(skip)]
    pub markdown_editor: EasyMarkEditor,
    pub delete: Option<TaskNotePayload>,
    pub users: BTreeSet<String>,
    pub edit_text: HashMap<String, TaskNotePayload>,
    pub allow_edit: HashSet<String>,
    pub task_id: Option<RecordId>
}

impl Default for ChatView{
    fn default() -> Self {
        Self { 
            title: "Chat".to_string(), 
            messages: Vec::new(), 
            current_user: None, 
            markdown_editor: EasyMarkEditor::default(),
            delete: None,
            users: BTreeSet::new(),
            edit_text: HashMap::new(),
            allow_edit: HashSet::new(),
            task_id: None
        }
    }
}

impl ChatView {
    pub fn new(messages: Vec<TaskNotePayload>, current_user: User, users: Vec<User>, task_id: Option<RecordId>) -> Self {
        // info!("chats/mod.rs -> Before messages: {messages:?}");
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
            title: "Chat".to_string(),
            markdown_editor: EasyMarkEditor::new(),
            delete: None,
            users: users_set,
            edit_text: note_ids,
            allow_edit: HashSet::new(),
            task_id
        }
    }


    pub fn insert_note(&mut self, new_note: &mut TaskNotePayload){
        if let Some(existing_note) = self.messages.iter_mut().find(|n| n.id == new_note.id .clone()) {
            // Apply diffs to the existing note
            let diffs = existing_note.diff(&new_note);
            existing_note.apply_mut(diffs);
            info!("chats/mod.rs -> Updated existing note: {:?}", existing_note);
        } else {
            self.messages.push(new_note.clone());
        }
    }

    pub fn delete_note(&mut self, note_to_delete: &TaskNotePayload){
        let index = self.messages.iter().position(|n| n == note_to_delete);
        if let Some(idx) = index {
            info!("chats/mod.rs -> Deleting Note @ {idx}");
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

        let b_panel_marg = Margin::symmetric(5, 10);

        let markdown_editor = &mut self.markdown_editor;
        markdown_editor.inputs = self.users.clone();
        let central_panel_frame = Frame::new().fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6));

        let task_id = self.task_id.clone();

        let id = ui.auto_id_with(format!("Chat {:?}", task_id));

        TopBottomPanel::bottom(id)
            .default_height(ui.available_height()/1.2)
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
                    info!("chats/mod.rs -> Txt: {txt}");
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
                            task_id, 
                            username,
                            user: Some(usr.id),
                            id_employee,
                            id_customer_thread,
                            ..Default::default() 
                        };

                        // We only need a single thread ID
                        new_note.id_customer_thread = self.messages.first().cloned().unwrap_or_default().id_customer_thread;

                        // If there is a thread, its definitely associated to a Service
                        // otherwise, it could be a regular task with no associated Service
                        // if let Some(thread) = &new_note.id_customer_thread {
                        //     if !thread.is_empty() {

                        //     }
                        // } else {

                        // }

                        info!("chats/mod.rs -> new_note: {new_note:?}");

                        PlatformSpawner::spawn(async move {
                            if let Err(e) = new_note.handle_note_creation().await {
                                error!("Failed to create task note: {:?}", e);
                            } else {
                                info!("chats/mod.rs -> Task note successfully created.");
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
                .max_width(255.)
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {

                let max_msg_width = ui.available_width() / 2.5;
                let fixed_height = 50.;
                let min_width = 200.;
                let other = min_width - 30.;
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

                        let rounding = 8;
                        let margin = 8;
                        
                        // ui.set_min_width(min_width);
                        let rnding = eframe::egui::CornerRadius {
                            ne: if is_message_from_myself { 0 } else { rounding },
                            nw: if is_message_from_myself { rounding } else { 0 },
                            se: rounding,
                            sw: rounding,
                        };

                        let response = Frame::new()
                            .corner_radius(rnding)
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .show(ui, |ui| {
                                ui.set_min_height(fixed_height);  // Set the fixed height for the message box
                                ui.set_max_width(max_msg_width);
                                // Use a vertical layout to stack the name and message content
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {

                                    let mut shadow = Shadow::default();
                                    shadow.blur = 3;
                                    shadow.spread = 3;
                                    shadow.color = Color32::from_rgb(40,36,40);
                                    
                                    let mut b_panel_marg = Margin::default();
                                    b_panel_marg.top = 3;

                                    let color = Color32::from_rgb(10,10,12);

                                    let note_frame = Frame::new().fill(color)
                                        .shadow(shadow).stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
                                        .inner_margin(Margin::symmetric(6, 10)).corner_radius(rnding);

                                    let from = RichText::new(&item.username).strong().monospace().color(Color32::LIGHT_BLUE);

                                    if is_message_from_myself {
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::RightToLeft,
                                            Align::Min,
                                        ), |ui| {
                                            // ui.set_max_width(max_msg_width);
                                            ui.add_space(8.);
                                            Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30., 20.)).sense(Sense::hover()).ui(ui);
                                            
                                            ui.add_space(20.);
                                            let parsed_date = DateTime::parse_from_rfc3339(&item.created_at.clone())
                                                .unwrap_or_default()
                                                .with_timezone(&Local);
                                        
                                            let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                                            ui.label(RichText::new(formatted_date).weak());
                                            ui.add_space(20.);
                                            ui.add_space(other);

                                            let id = &item.id;

                                            if self.allow_edit.contains(&id.to_string()) {
                                                let save_btn = Button::new(RichText::new("Save").weak().color(Color32::LIGHT_RED))
                                                    .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui);

                                                if save_btn.clicked(){
                                                    if self.allow_edit.contains(&id.to_string()) {
                                                        if let Some(msg) = self.edit_text.get_mut(&id.to_string()){
                                                            let mut task_note = msg.clone();
                                                            // if note_pre_edit.ne(&msg.note) {
                                                            PlatformSpawner::spawn(async move {
                                                                match task_note.modify_prestashop_note().await {
                                                                    Ok(res) => info!("chats/mod.rs -> Modify note response:: {res:?}"),
                                                                    Err(e) => error!("Error modifying note: {e:?}"),
                                                                }
                                                            });
                                                            item.note = msg.note.clone();
                                                        }
                                                    }
                                                    self.allow_edit.remove(&id.to_string());
                                                }
                                                let cancel_btn = Button::new(RichText::new("Cancel").weak().color(Color32::LIGHT_RED))
                                                    .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui);

                                                if cancel_btn.clicked(){
                                                    self.allow_edit.remove(&id.to_string());
                                                }
                                            } else {
                                                let edit_btn = Button::new(RichText::new("🖊").weak().color(Color32::LIGHT_RED))
                                                    .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui)
                                                    .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"));

                                                if edit_btn.clicked(){
                                                    self.allow_edit.insert(item.id.to_string()); 
                                                }
                                            }

                                            let copy_btn = Button::new(RichText::new("🗐").weak().color(Color32::LIGHT_RED))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui)
                                                .on_hover_text(RichText::new("Copy Task Note"));

                                            if copy_btn.clicked(){
                                                ui.ctx().copy_text(item.note.clone());
                                            }

                                            ui.add_space(6.);

                                            let btn = Button::new(RichText::new("🗙").weak().color(Color32::LIGHT_RED))
                                                .corner_radius(eframe::egui::CornerRadius::same(255))
                                                .small()
                                                .min_size(Vec2::new(30., 14.))
                                                .ui(ui)
                                                .on_hover_text(
                                                    RichText::new("WARNING, this will delete the note from prestashop AND Master-tech.app\nIf this is what you want, DOUBLE CLICK to delete")
                                                        .strong()
                                                        .color(Color32::LIGHT_RED)
                                                );

                                            if btn.double_clicked(){
                                                self.delete = Some(item.clone());
                                                let mut item = item.clone();
                                                PlatformSpawner::spawn(async move {
                                                    match item.delete_note().await{
                                                        Ok(_) => info!("chats/mod.rs -> Deleted Note"),
                                                        Err(e) => error!("chats/mod.rs -> Error deleting note: {e:?}"),
                                                    }
                                                })
                                            }
                                        });
                                        
                                    } else{
                                        ui.with_layout(Layout::from_main_dir_and_cross_align(
                                            Direction::LeftToRight,
                                            Align::Min,
                                        ), |ui| {
                                            // ui.set_max_width(max_msg_width);
                                            ui.add_space(8.);
                                            Button::new(from).fill(Color32::TRANSPARENT).min_size(Vec2::new(30., 20.)).sense(Sense::hover()).ui(ui);
                                            ui.add_space(35.);
                                            let parsed_date = DateTime::parse_from_rfc3339(&item.created_at.clone())
                                                .unwrap_or_default()
                                                .with_timezone(&Local);
                                        
                                            let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                                            ui.label(RichText::new(formatted_date).weak());
                                            
                                            ui.add_space(10.);
                                            let copy_btn = Button::new(RichText::new("🗐").small().weak().color(Color32::LIGHT_RED))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui);

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
                                                    TextEdit::multiline(&mut msg.note)
                                                        .margin(Margin::symmetric(10, 3))
                                                        .desired_width(255.)
                                                        .show(ui);
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



