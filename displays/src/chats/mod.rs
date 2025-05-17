use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Layout, Margin, Rect, RichText, ScrollArea, Shape, Stroke, TextEdit, TopBottomPanel, Ui, Widget};
use database::{live_data::handle_live_delete, schema::{TaskNotePayload, User}};
use super::markdown_editor::{viewer, EasyMarkEditor, SHORTCUT_ENTER};
use crate::{get_current_user_from_auth, PlatformSpawner, Spawner};
use std::collections::{BTreeSet, HashMap, HashSet};
use crossbeam::channel::{Receiver, Sender};
use structdiff::StructDiff;
use eframe::emath::Vec2;
use surrealdb::RecordId;
use itertools::Itertools;
use log::{error, info};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ChatView{
    pub title: String,
    pub messages: Vec<TaskNotePayload>,
    pub current_user: User,
    #[serde(skip)]
    pub markdown_editor: EasyMarkEditor,
    pub delete: Option<TaskNotePayload>,
    pub users: BTreeSet<String>,
    pub edit_text: HashMap<String, TaskNotePayload>,
    pub allow_edit: HashSet<String>,
    pub task_id: Option<RecordId>,
    pub service_number: Option<String>,
    #[serde(skip)]
    pub new_notes_tx: Sender<Vec<TaskNotePayload>>, 
    #[serde(skip)]
    pub new_notes_rx: Receiver<Vec<TaskNotePayload>>,
}

impl Default for ChatView {
    fn default() -> Self {
        let (new_notes_tx, new_notes_rx) = crossbeam::channel::unbounded();
        let current_user = if let Some(user) = get_current_user_from_auth() {
            user
        } else {
            User::default()
        };

        Self { 
            title: "Chat".to_string(), 
            messages: Vec::new(),
            current_user,
            markdown_editor: EasyMarkEditor::default(),
            delete: None,
            users: BTreeSet::new(),
            edit_text: HashMap::new(),
            allow_edit: HashSet::new(),
            task_id: None,
            service_number: None,
            new_notes_tx, new_notes_rx
        }
    }
}

impl ChatView {
    pub fn new(
        messages: Vec<TaskNotePayload>,
        users: Vec<User>, 
        task_id: Option<RecordId>,
        service_number: Option<String>
    ) -> Self {
        let (new_notes_tx, new_notes_rx) = crossbeam::channel::unbounded();
        let mut users_set = BTreeSet::new();
        let mut note_ids = HashMap::new();
        for user in users {
            let parsed_email = user.get_username();
            users_set.insert(format!("@{parsed_email}"));   
        }

        for message in messages.iter() {
            note_ids.insert(message.id.to_string(), message.clone());
        }

        log::info!("Note ID's: {}", note_ids.len());

        Self {
            current_user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            messages,
            title: "Chat".to_string(),
            markdown_editor: EasyMarkEditor::new(),
            delete: None,
            users: users_set,
            edit_text: note_ids,
            allow_edit: HashSet::new(),
            task_id,
            service_number,
            new_notes_tx, new_notes_rx
        }
    }

    pub fn insert_note(&mut self, new_note: &mut TaskNotePayload){
        // todo!("I need to check that ALL ID's MATCH, not just one, including id_customer_thread, task_id");
        let all_notes_match = self
            .messages
            .iter()
            .all(|n| 
                n.id_customer_thread == new_note.id_customer_thread.clone()
                && n.service_number == new_note.service_number.clone()
                && n.task_id == new_note.task_id.clone()
            );

        if all_notes_match {
            log::info!("chats/mod.rs -> id_customer_thread, service_number, task_id all match");
            if let Some(existing_note) = self.messages.iter_mut().find(|n| n.id == new_note.id .clone()) {
                // Apply diffs to the existing note
                let diffs = existing_note.diff(&new_note);
                existing_note.apply_mut(diffs);
                info!("chats/mod.rs -> Updated existing note: {:?}", existing_note.id);
            } else {
                info!("chats/mod.rs -> Inserting new note: {:#?}", new_note.id);
                self.messages.push(new_note.clone());
            }
            self.edit_text.insert(new_note.id.to_string(), new_note.clone());
        } else {
            log::warn!("chats/mod.rs -> id_customer_thread, service_number, or task_id do not match\nOr self.messages is empty");
            log::warn!("chats/mod.rs -> self.messages: {:#?}\nnew_note: {:#?}", self.messages.clone(), new_note.clone());
        }
    }

    pub fn delete_note(&mut self, note_to_delete: &TaskNotePayload){
        let index = self.messages.iter().position(|n| n == note_to_delete);
        if let Some(idx) = index {
            info!("chats/mod.rs -> Deleting Note @ {idx}");
            self.messages.remove(idx);
            self.edit_text.remove(&note_to_delete.id.to_string());
        }
    }

    pub fn refresh_notes(&self) {
        let service_number = self.service_number.clone().unwrap_or_default();
        let task_id = self.task_id.clone();
        let user = self.current_user.clone();
        let tx = self.new_notes_tx.clone();
        let id_customer_thread = self
            .messages
            .iter()
            .filter_map(|m| m.id_customer_thread.clone())
            .all_equal_value()
            .iter()
            .next()
            .cloned()
            .unwrap_or_default();

        log::info!(r#"Chats/mod.rs -> refresh_notes
            -> service_number: {}
            -> task_id: {:?}
            -> user: {}
            -> id_customer_thread: {}
        "#, service_number, task_id, user.get_username(), id_customer_thread);

        PlatformSpawner::spawn(async move {
            let mut task_note = TaskNotePayload {
                service_number: Some(service_number.clone()),
                task_id: task_id.clone(),
                user: Some(user.get_id().clone()),
                username: user.get_username().to_string(),
                ..Default::default()
            };

            if id_customer_thread.is_empty() && !service_number.is_empty() {
                log::info!("Chats/mod.rs -> refresh_notes -> task_note.get_notes_from_service_number");
                match task_note.get_notes_from_service_number(&service_number).await {
                    Ok(notes) => {let _ = tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from service number: {e:?}"),
                };
            } else if !id_customer_thread.is_empty() && service_number.is_empty() {
                log::info!("Chats/mod.rs -> refresh_notes -> TaskNotePayload::get_db_notes_from_task_id");
                if let Some(id) = task_id.clone() {
                    match TaskNotePayload::get_db_notes_from_task_id(id).await {
                        Ok(notes) => {let _ = tx.try_send(notes); },
                        Err(e) => log::error!("Error getting notes from service number: {e:?}"),
                    };
                }
            } else {
                match task_note.get_notes_from_service_number(&service_number).await {
                    Ok(notes) => {let _ = tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from service number: {e:?}"),
                };
            }
        });
    }

    pub fn receive(&mut self) {
        if let Ok(notes) = self.new_notes_rx.try_recv() {
            log::info!("Chats/mod.rs -> refresh_notes -> self.new_notes_rx.try_recv(): {notes:?}");
            let mut new_messages = vec![];

            // Collect IDs of existing messages for efficient lookup
            let existing_ids: std::collections::HashSet<_> = self
                .messages
                .iter()
                .map(|msg| msg.id.clone())
                .collect();

            log::info!("Chats/mod.rs -> refresh_notes -> Existing ID's: {existing_ids:?}");

            // Add notes that don't exist in current messages
            for note in notes {
                if !existing_ids.contains(&note.id) {
                    log::info!("Adding new note with ID: {:?}", note.id);
                    new_messages.push(note);
                }
            }

            // Append new messages to the existing list
            if !new_messages.is_empty() {
                self.messages.extend(new_messages);
            }
        }
    }
    
    pub fn ui(&mut self, ui: &mut Ui) {
        self.receive();
        let task_id = self.task_id.clone();
        let id = ui.auto_id_with(format!("Chat {:?}", task_id));
        if let Some(note) = std::mem::take(&mut self.delete) {
            if let Err(e) = handle_live_delete(&mut self.messages, note.clone()) {
                error!("Error deleting note: {e:?}");
            }
        }

        TopBottomPanel::top(format!("Top panel header {:?}", task_id)).exact_height(28.).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(""); 
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    if Button::new("Refresh").ui(ui).clicked() {
                        self.refresh_notes();
                    }
                });
            });
        });

        TopBottomPanel::bottom(id)
            .default_height(ui.available_height()/1.2)
            .resizable(true)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            let enter_pressed = ui.input_mut(|i| i.consume_shortcut(&SHORTCUT_ENTER));
            
            let markdown_editor = &mut self.markdown_editor;
            markdown_editor.inputs = self.users.clone();

            if let Some(response) = markdown_editor.ui(ui)
            {
                if response.clicked() || enter_pressed {
                    let txt = markdown_editor.submit();
                    info!("chats/mod.rs -> Txt: {txt}");
                    markdown_editor.clear();

                    let usr = &mut self.current_user.clone();
                    log::warn!("USER: {usr:?}");
                    if usr.get_email().is_empty() {
                        if let Some(user) = get_current_user_from_auth() {
                            log::warn!("USER FROM AUTH: {user:?}");
                            *usr = user;
                        }
                    }

                    // Extract the first customer thread ID if available
                    let id_customer_thread = self
                        .messages
                        .iter()
                        .filter_map(|m| m.id_customer_thread.clone())
                        .all_equal_value()
                        .iter()
                        .next()
                        .cloned();

                    match usr.get_employee_id().clone() {
                        Some(id_employee) => {
                            let mut new_note = TaskNotePayload {
                                note: txt, 
                                task_id, 
                                username: usr.get_username().to_string(),
                                user: Some(usr.get_id()),
                                id_employee: Some(id_employee.to_string()),
                                id_customer_thread,
                                service_number: self.service_number.clone(),
                                private: markdown_editor.private_note.clone(),
                                ..Default::default() 
                            };

                            info!("chats/mod.rs -> new_note: {new_note:?}");
                            
                            PlatformSpawner::spawn(async move {
                                if let Err(e) = new_note.handle_note_creation().await {
                                    error!("Failed to create task note: {:?}", e);
                                } else {
                                    info!("chats/mod.rs -> Task note successfully created.");
                                }
                            });
                        },
                        None => log::info!("No employee ID found"),
                    }
                }
            }
        });

        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
                    .stroke(ui.style().visuals.widgets.inactive.bg_stroke)
                    .outer_margin(Margin::symmetric(5, 10))
                    .inner_margin(Margin::same(6))
            )
            .show_inside(ui, |ui| 
        {
            ScrollArea::vertical()
                .animated(true)
                .max_height(ui.available_height())
                .max_width(ui.available_width())
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(ui, |ui| 
            {

                let max_msg_width = ui.available_width()/2.5;
                let fixed_height = 50.;
                let min_width = 200.;
                let other = min_width - 30.;
                
                self.messages.sort_by_key(|message| message.created_at.clone() );

                for item in self.messages.iter_mut(){
                    let user = self.current_user.clone();
                    let is_message_from_myself = if item.username == user.get_username() {
                        true
                    } else { false };

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
                        ui.set_width(max_msg_width);

                        let rounding = 8.;
                        let margin = 8.;
                        
                        // ui.set_min_width(min_width);
                        let rnding = eframe::egui::CornerRadius {
                            ne: if is_message_from_myself { 0 } else { rounding as u8 },
                            nw: if is_message_from_myself { rounding as u8 } else { 0 },
                            se: rounding as u8,
                            sw: rounding as u8,
                        };

                        let mut main_note_frame = Frame::new()
                            .corner_radius(rnding)
                            .inner_margin(margin)
                            .outer_margin(margin)
                            .fill(msg_color)
                            .begin(ui);

                        let style = ui.style().clone();
                        main_note_frame.frame.stroke = style.visuals.widgets.open.bg_stroke;

                        { // NOTE FRAME SCOPED UI
                            let ui = &mut main_note_frame.content_ui;
                            ui.set_min_height(fixed_height);  // Set the fixed height for the message box
                            ui.set_max_width(max_msg_width);
                            // Use a vertical layout to stack the name and message content
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                let btn_txt_color = ui.style().visuals.error_fg_color;
                                let username_txt_color = ui.style().visuals.hyperlink_color;
                                let from = RichText::new(&item.username).strong().monospace().color(username_txt_color);
                                if is_message_from_myself {
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::RightToLeft,
                                        Align::Min,
                                    ), |ui| {
                                        // ui.set_max_width(max_msg_width);
                                        ui.add_space(8.);
                                        Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 20.)).ui(ui);
                                        
                                        ui.add_space(20.);
                                        // let parsed_date = item.created_at.clone();
                                    
                                        ui.label(RichText::new(item.created_at.format("%Y/%m/%d @ %I:%M%p").to_string()).weak());
                                        ui.add_space(20.);
                                        ui.add_space(other);

                                        let id = &item.id;

                                        if self.allow_edit.contains(&id.to_string()) {
                                            let save_btn = Button::new(RichText::new("Save").weak().color(btn_txt_color))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui);

                                            if save_btn.clicked(){
                                                if self.allow_edit.contains(&id.to_string()) {
                                                    if let Some(msg) = self.edit_text.get_mut(&id.to_string()){
                                                        let mut task_note = msg.clone();
                                                        item.note = msg.note.clone();
                                                        // if note_pre_edit.ne(&msg.note) {
                                                        PlatformSpawner::spawn(async move {
                                                            match task_note.update_note().await {
                                                                Ok(res) => info!("chats/mod.rs -> Modify note response:: {res:?}"),
                                                                Err(e) => error!("Error modifying note: {e:?}"),
                                                            }
                                                        });
                                                        
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
                                            let edit_btn = Button::new(RichText::new("🖊").weak().color(btn_txt_color))
                                                .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui)
                                                .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"));

                                            if edit_btn.clicked(){
                                                self.allow_edit.insert(item.id.to_string()); 
                                            }
                                        }

                                        let copy_btn = Button::new(RichText::new("🗐").weak().color(btn_txt_color))
                                            .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui)
                                            .on_hover_text(RichText::new("Copy Task Note"));

                                        if copy_btn.clicked(){
                                            ui.ctx().copy_text(item.note.clone());
                                        }

                                        let btn = Button::new(RichText::new("🗙").weak().color(btn_txt_color))
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
                                        ui.add_space(4.);
                                        Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 20.)).ui(ui);
                                        ui.add_space(10.);
                                    
                                        ui.label(RichText::new(item.created_at.format("%Y/%m/%d @ %I:%M%p").to_string()).weak());
                                        
                                        ui.add_space(10.);

                                        ui.label(if item.private { "🕶" } else { "✔" });

                                        ui.add_space(10.);

                                        let copy_btn = Button::new(RichText::new("🗐").small().weak().color(btn_txt_color))
                                            .corner_radius(eframe::egui::CornerRadius::same(255)).small().min_size(Vec2::new(30., 14.)).ui(ui);

                                        if copy_btn.clicked(){
                                            ui.ctx().copy_text(item.note.clone());
                                        }
                                    });
                                }

                                Frame::new()
                                    .fill(Color32::from_rgb(10,10,12))
                                    .shadow(Shadow {
                                        blur: 3,
                                        spread: 3,
                                        color: Color32::from_rgb(40,36,40),
                                        ..Default::default()
                                    })
                                    .stroke(style.visuals.widgets.inactive.bg_stroke)
                                    .outer_margin(Margin {
                                        top: 3,
                                        ..Default::default()
                                    })
                                    .inner_margin(Margin::symmetric(6, 10))
                                    .corner_radius(rnding)
                                    .show(ui, |ui| 
                                {
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::TopDown,
                                        Align::Center,
                                    ), |ui| {
                                        ui.set_width(ui.available_width());
                                        if self.allow_edit.contains(&item.id.to_string()) {
                                            if let Some(msg) = self.edit_text.get_mut(&item.id.to_string()){
                                                TextEdit::multiline(&mut msg.note)
                                                    .margin(Margin::symmetric(10, 3))
                                                    .desired_width(max_msg_width)
                                                    .show(ui);
                                            }
                                        } else {
                                            viewer::easy_mark(ui, &item.note);
                                        }
                                    });
                                });

                            });
                        };
                        let response = main_note_frame.allocate_space(ui);
                        if response.hovered() {
                            // style.visuals.widgets.inactive.bg_fill
                            main_note_frame.frame.fill =  style.visuals.widgets.inactive.bg_fill + Color32::from_rgb(1, 1, 3);
                            main_note_frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
                            main_note_frame.frame.shadow = style.visuals.window_shadow;
                        }
                        main_note_frame.paint(ui);

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

                        ui.painter().add(Shape::convex_polygon(points, msg_color, Stroke::NONE));

                    });
                };
            });
        });
    }
}



