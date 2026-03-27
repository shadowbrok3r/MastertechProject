use eframe::egui::{Align, Button, CentralPanel, Color32, Context, Direction, Frame, Layout, Margin, Popup, PopupCloseBehavior, RectAlign, Response, RichText, ScrollArea, Shadow, Style, TextEdit, TopBottomPanel, Ui, Widget};
use database::{live_data::handle_live_delete, schema::{random_record_id, RecordIdExt, TaskNotePayload, User, TASK_NOTE_TABLE}};
use super::markdown_editor::{viewer, EasyMarkEditor, SHORTCUT_ENTER};
use std::{collections::{BTreeSet, HashMap, HashSet}, f32, sync::Arc};
use crate::{get_current_user_from_auth, get_toast_sender, PlatformSpawner, Spawner, ToastMessage};
use crossbeam::channel::{Receiver, Sender};
use structdiff::StructDiff;
use eframe::emath::Vec2;
use database::schema::RecordId;
use itertools::Itertools;
use log::{error, info};
use serde::Serialize;
use chrono::Utc;

#[derive(Debug, Clone, Serialize)]
pub struct ChatView{
    pub title: String,
    pub messages: Vec<TaskNotePayload>,
    pub current_user: User,
    #[serde(skip)]
    markdown_editor: EasyMarkEditor,
    delete: Option<TaskNotePayload>,
    users: BTreeSet<String>,
    edit_text: HashMap<String, TaskNotePayload>,
    allow_edit: HashSet<String>,
    pub task_id: RecordId,
    service_number: Option<String>,
    hovered: HashSet<RecordId>,
    remove_hovered: Option<RecordId>,
    #[serde(skip)]
    new_notes_tx: Sender<Vec<TaskNotePayload>>, 
    #[serde(skip)]
    new_notes_rx: Receiver<Vec<TaskNotePayload>>,
    #[serde(skip)]
    ui_event_tx: Sender<ChatEvent>, 
    #[serde(skip)]
    ui_event_rx: Receiver<ChatEvent>,
}

impl Default for ChatView {
    fn default() -> Self {
        let (new_notes_tx, new_notes_rx) = crossbeam::channel::unbounded();
        let (ui_event_tx, ui_event_rx) = crossbeam::channel::unbounded();

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
            // THIS IS ON PURPOSE SO WE CANT ACCIDENTALLY TRY AND LEAVE A NOTE WITHOUT A TASK
            task_id: random_record_id(TASK_NOTE_TABLE),
            service_number: None,
            hovered: HashSet::new(),
            remove_hovered: None,
            new_notes_tx, new_notes_rx,
            ui_event_tx, ui_event_rx,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum ChatEvent {
    Edit(RecordId),
    SaveNote(TaskNotePayload),
    CancelEdit(RecordId),
    DeleteNote(TaskNotePayload),
}

impl ChatView {
    pub fn new(
        users: Vec<User>, 
        task_id: RecordId,
        service_number: Option<String>
    ) -> Self {
        let (new_notes_tx, new_notes_rx) = crossbeam::channel::unbounded();
         let (ui_event_tx, ui_event_rx) = crossbeam::channel::unbounded();
        let mut users_set = BTreeSet::new();

        for user in users {
            let parsed_email = user.get_username();
            users_set.insert(format!("@{parsed_email}"));   
        }

        let service_number = if let Some(service_number) = service_number {
            if service_number.is_empty() {
                None
            } else {
                Some(service_number.clone())
            }
        } else {
            None
        };


        Self {
            current_user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            messages: vec![],
            title: "Chat".to_string(),
            markdown_editor: EasyMarkEditor::new(),
            delete: None,
            users: users_set,
            edit_text: HashMap::new(),
            allow_edit: HashSet::new(),
            task_id,
            service_number,
            hovered: HashSet::new(),
            remove_hovered: None,
            new_notes_tx, new_notes_rx,
            ui_event_tx, ui_event_rx,
        }
    }

    pub fn set_notes(&mut self, notes: Vec<TaskNotePayload>) -> &mut Self {
        // Collect existing IDs from self.messages to check for duplicates
        let mut seen_ids: HashSet<String> = self.messages
            .iter()
            .map(|note| note.id.key_string())
            .collect();

        // Process new notes, adding only non-duplicates
        for mut note in notes {
            // Make sure seeded notes are tied to this chat's task
            note.set_task_id(&self.task_id);
            if seen_ids.insert(note.id.key_string()) {
                // Add to edit_text (overwrites if ID exists, but seen_ids ensures it’s new)
                self.edit_text.insert(note.id.key_string(), note.clone());
                // Append to messages
                self.messages.push(note);
            }
        }

        self
    }

    pub fn set_service_number(&mut self, service_number: String) -> &mut Self {
        self.service_number = Some(service_number.clone());
        self
    }

    pub fn set_users(&mut self, users: Vec<User>) -> &mut Self {
        for user in users.iter() {
            let parsed_email = user.get_username();
            self.users.insert(format!("@{parsed_email}"));  
        }
        self
    }

    pub fn insert_note(&mut self, new_note: &mut TaskNotePayload){
        // Check that the note belongs to this chat view by matching task_id and service_number.
        // For id_customer_thread, we need to handle private notes specially:
        // - Private notes have id_customer_thread: None even when the task has a thread
        // - Non-private notes should match id_customer_thread with existing notes
        let note_belongs_to_chat = if self.messages.is_empty() {
            // If no messages yet, check if note matches our task_id and service_number
            new_note.task_id.as_ref() == Some(&self.task_id)
                && new_note.service_number == self.service_number
        } else {
            self.messages.iter().all(|n| {
                // Task ID must match
                let task_matches = n.task_id == new_note.task_id;
                // Service number must match
                let service_matches = n.service_number == new_note.service_number;
                // For id_customer_thread: 
                // - If new note is private, it won't have a thread, so skip this check
                // - If existing notes are private (no thread), also skip
                // - Otherwise, threads should match
                let thread_matches = new_note.private 
                    || n.private 
                    || n.id_customer_thread.is_none() 
                    || new_note.id_customer_thread.is_none()
                    || n.id_customer_thread == new_note.id_customer_thread;
                
                task_matches && service_matches && thread_matches
            })
        };

        if note_belongs_to_chat {
            log::info!("chats/mod.rs -> Note belongs to this chat (task_id, service_number match)");
            if let Some(existing_note) = self.messages.iter_mut().find(|n| n.id == new_note.id.clone()) {
                // Apply diffs to the existing note
                let diffs = existing_note.diff(&new_note);
                existing_note.apply_mut(diffs);
                info!("chats/mod.rs -> Updated existing note: {:?}", existing_note.id);
            } else {
                info!("chats/mod.rs -> Inserting new note: {:#?}", new_note.id);
                self.messages.push(new_note.clone());
            }
            self.edit_text.insert(new_note.id.key_string(), new_note.clone());
        } else {
            log::warn!(
                "chats/mod.rs -> Note does not belong to this chat.\n\
                Note task_id: {:?}, chat task_id: {:?}\n\
                Note service_number: {:?}, chat service_number: {:?}\n\
                Note is private: {}", 
                new_note.task_id, self.task_id,
                new_note.service_number, self.service_number,
                new_note.private
            );
        }
    }

    pub fn delete_note(&mut self, note_to_delete: &TaskNotePayload){
        let index = self.messages.iter().position(|n| n == note_to_delete);
        if let Some(idx) = index {
            info!("chats/mod.rs -> Deleting Note @ {idx}");
            self.messages.remove(idx);
            self.edit_text.remove(&note_to_delete.id.key_string());
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
            if !service_number.is_empty() {
                let notes_res = TaskNotePayload::get_prestashop_notes_from_service(&service_number, Some(task_id.clone())).await;
                match notes_res {
                    Ok(notes) => {let _ = tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from service number: {e:?}"),
                };
            } else {
                let notes_res = TaskNotePayload::get_db_notes_from_task_id(task_id.clone()).await;
                match notes_res {
                    Ok(notes) => { let _ = tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from service number: {e:?}"),
                };
            }
        });
    }

    pub fn receive(&mut self) {
        if let Ok(mut notes) = self.new_notes_rx.try_recv() {

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
            for note in notes.iter_mut() {
                note.set_task_id(&self.task_id);

                if !existing_ids.contains(&note.id) {
                    log::info!("Adding new note with ID: {:?}", note.id);
                    new_messages.push(note.clone());
                }
            }

            // Append new messages to the existing list
            if !new_messages.is_empty() {
                self.messages.extend(new_messages);
            }
        }
    
        if let Ok(event) = self.ui_event_rx.try_recv() {
            match event {
                ChatEvent::SaveNote(task_note) => {
                    if self.allow_edit.contains(&task_note.id.key_string()) {
                        if let Some(msg) = self.edit_text.get_mut(&task_note.id.key_string()){
                            let mut task_note = msg.clone();
                            task_note.note = msg.note.clone();
                            PlatformSpawner::spawn(async move {
                                match task_note.update_note().await {
                                    Ok(res) => info!("chats/mod.rs -> Modify note response:: {res:?}"),
                                    Err(e) => {
                                        error!("Error modifying note: {e:?}");
                                        let tx = get_toast_sender();
                                        let _ = tx.try_send(ToastMessage::Error(
                                            format!("Failed to update note: {:?}", e)
                                        ));
                                    },
                                }
                            });
                        }
                    }
                    self.allow_edit.remove(&task_note.id.key_string());
                },
                ChatEvent::Edit(id) => { self.allow_edit.insert(id.key_string()); },
                ChatEvent::CancelEdit(id) => { self.allow_edit.remove(&id.key_string()); },
                ChatEvent::DeleteNote(note) => {
                    self.delete = Some(note.clone());
                    let mut item = note.clone();
                    PlatformSpawner::spawn(async move {
                        match item.delete_note().await{
                            Ok(_) => info!("chats/mod.rs -> Deleted Note"),
                            Err(e) => {
                                error!("chats/mod.rs -> Error deleting note: {e:?}");
                                let tx = get_toast_sender();
                                let _ = tx.try_send(ToastMessage::Error(
                                    format!("Failed to delete note: {:?}", e)
                                ));
                            },
                        }
                    })
                },
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

        eframe::egui::Panel::top(format!("Top panel header {:?}", task_id)).exact_height(24.).show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                if Button::new(RichText::new("Refresh").strong().heading()).ui(ui).clicked() {
                    self.refresh_notes();
                }
            });
        });

        eframe::egui::Panel::bottom(id)
            .default_height(300.)
            // .max_height(500.)
            .resizable(false)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            let enter_pressed = ui.input_mut(|i| i.consume_shortcut(&SHORTCUT_ENTER));
            
            let markdown_editor = &mut self.markdown_editor;
            markdown_editor.inputs = self.users.clone();

            if let Some(response) = markdown_editor.ui(ui) {
                if response.clicked() || enter_pressed {
                    let txt = markdown_editor.submit();
                    info!("chats/mod.rs -> Txt: {txt}");
                    markdown_editor.clear();

                    let usr = &mut self.current_user.clone();
                    if usr.get_email().is_empty() {
                        if let Some(user) = get_current_user_from_auth() {
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
                                task_id: Some(task_id.clone()), 
                                username: usr.get_username().to_string(),
                                user: usr.get_id(),
                                id_employee: Some(id_employee.to_string()),
                                id_customer_thread,
                                service_number: self.service_number.clone(),
                                private: markdown_editor.private_note.clone(),
                                id: random_record_id(TASK_NOTE_TABLE),
                                created_at: Utc::now().into(),
                                id_customer_message: None,
                            };

                            error!("chats/mod.rs -> new_note: {new_note:#?}");
                            
                            // Copy note to clipboard regardless of result
                            let note_text = new_note.note.clone();
                            ui.ctx().copy_text(note_text.clone());
                            
                            PlatformSpawner::spawn(async move {
                                if let Err(e) = new_note.handle_note_creation().await {
                                    error!("Failed to create task note: {:?}", e);
                                    // Send error toast
                                    let tx = get_toast_sender();
                                    let _ = tx.try_send(ToastMessage::Error(
                                        format!("Failed to send note: {:?}. Note copied to clipboard.", e)
                                    ));
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
            .frame(Frame::new().inner_margin(Margin::same(2)).fill(Color32::from_rgb(12, 12, 16)))
            .show_inside(ui, |ui| 
        {
            
            ScrollArea::vertical()
                .max_height(f32::INFINITY)
                .max_width(f32::INFINITY)
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_min_height(ui.available_height()/1.1);
                    self.chat(ui) 
            });
            ui.set_max_height(750.);
        });
    }

    fn chat(&mut self, ui: &mut Ui) {
        let max_msg_width = 376.;

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
                ui.style().visuals.widgets.active.bg_fill
            } else {
                ui.style().visuals.widgets.active.weak_bg_fill
            };

            ui.with_layout(layout, |ui| {
                ui.set_max_width(max_msg_width);
                let rounding = 8.;
                let outer_margin = Margin { left: 1, right: 1, top: 4, bottom: 1 };
                let inner_margin = Margin { left: 0, right: 0, top: 4, bottom: 0 };

                let rnding = eframe::egui::CornerRadius {
                    ne: if is_message_from_myself { 0 } else { rounding as u8 },
                    nw: if is_message_from_myself { rounding as u8 } else { 0 },
                    se: rounding as u8,
                    sw: rounding as u8,
                };

                let style = ui.style().clone();

                let (fill, stroke, shadow) = if self.hovered.contains(&item.id) {
                    (
                        style.visuals.widgets.inactive.bg_fill + Color32::from_rgb(1, 1, 4),
                        style.visuals.widgets.hovered.fg_stroke,
                        style.visuals.window_shadow
                    )
                } else {
                    (
                        msg_color,
                        style.visuals.widgets.open.bg_stroke,
                        Shadow::default()
                    )
                };

                if Frame::new()
                .corner_radius(rnding)
                .inner_margin(inner_margin)
                .outer_margin(outer_margin)
                .fill(fill)
                .shadow(shadow)
                .stroke(stroke)
                .show(ui, |ui| { // NOTE FRAME SCOPED UI
                    ui.set_width(max_msg_width);
                    // Use a vertical layout to stack the name and message content
                    ui.vertical_centered(|ui| {
                        if is_message_from_myself {
                            ui.horizontal(|ui| {
                                let btn_txt_color = ui.style().visuals.error_fg_color;
                                let username_txt_color = ui.style().visuals.hyperlink_color;
                                let from = RichText::new(&item.username).strong().monospace().color(username_txt_color);
                                ui.add_space(5.);
                                if Button::new(RichText::new("🗙").color(btn_txt_color))
                                .min_size(Vec2::new(15., 14.))
                                .ui(ui)
                                .on_hover_text(
                                    RichText::new("WARNING, this will delete the note from prestashop AND Master-tech.app\nIf this is what you want, DOUBLE CLICK to delete")
                                        .strong()
                                        .color(Color32::LIGHT_RED)
                                )
                                .double_clicked() {
                                    let _ = self.ui_event_tx.try_send(ChatEvent::DeleteNote(item.clone()));
                                }

                                if Button::new(RichText::new("🗐").color(btn_txt_color))
                                .ui(ui)
                                .on_hover_text(RichText::new("Copy Task Note"))
                                .clicked() {
                                    ui.ctx().copy_text(item.note.clone());
                                }
                                
                                let id = &item.id;

                                if self.allow_edit.contains(&id.key_string()) {
                                    if Button::new(RichText::new("Save").color(btn_txt_color))
                                    .ui(ui)
                                    .clicked() {
                                        let _ = self.ui_event_tx.try_send(ChatEvent::SaveNote(item.clone()));
                                    }

                                    if Button::new(RichText::new("Cancel").color(Color32::LIGHT_RED))
                                    .ui(ui)
                                    .clicked() {
                                        let _ = self.ui_event_tx.try_send(ChatEvent::CancelEdit(id.clone()));
                                    }
                                } else {
                                        if Button::new(RichText::new("🖊").color(btn_txt_color)).ui(ui)
                                        .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"))
                                        .clicked() {
                                            let _ = self.ui_event_tx.try_send(ChatEvent::Edit(id.clone()));
                                        }
                                }

                                ui.add_space(5.);
                                ui.label(if item.private { "🕶" } else { "✔" });

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui|{
                                    ui.add_space(2.);
                                    let from_btn = Button::new(from)
                                        .fill(Color32::from_rgb(7, 7, 9))
                                        .min_size(Vec2::new(30., 20.))
                                        .ui(ui);

                                    if from_btn.clicked(){
                                        Popup::open_id(ui.ctx(), format!("sub_menu-from-{:?}", item.id).into());
                                    }

                                    popup_widget(from_btn, style.clone(), &item);

                                    ui.add_space(5.);

                                    ui.label(RichText::new(item.created_at.format("%m/%d @ %I:%M%p").to_string()).weak());
                                });
                            });
                        } else {
                            ui.horizontal(|ui| {
                                let btn_txt_color = ui.style().visuals.error_fg_color;
                                let username_txt_color = ui.style().visuals.hyperlink_color;
                                let from = RichText::new(&item.username).strong().monospace().color(username_txt_color);
                                // ui.set_max_width(max_msg_width);
                                let id = &item.id;
                                ui.add_space(2.);
                                let from_btn = Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 20.)).ui(ui);
                                if from_btn.clicked(){
                                    Popup::open_id(ui.ctx(), format!("sub_menu-from-{:?}", item.id).into());
                                }

                                popup_widget(from_btn, style.clone(), &item);

                                ui.add_space(5.);
                            
                                ui.label(RichText::new(item.created_at.format("%m/%d @ %I:%M%p").to_string()).weak());

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(5.);
                                    if self.current_user.is_admin() {
                                        if self.allow_edit.contains(&id.key_string()) {
                                            if Button::new(RichText::new("Cancel").color(Color32::LIGHT_RED))
                                            .ui(ui)
                                            .clicked() {
                                                let _ = self.ui_event_tx.try_send(ChatEvent::CancelEdit(id.clone()));
                                            }
                                            if Button::new(RichText::new("Save").color(btn_txt_color))
                                            .ui(ui)
                                            .clicked() {
                                                let _ = self.ui_event_tx.try_send(ChatEvent::SaveNote(item.clone()));
                                            }
                                        } else {
                                            if Button::new(RichText::new("🖊").color(btn_txt_color)).ui(ui)
                                            .on_hover_text(RichText::new("Edit Task Note\nWARNING: This will modify the note in Prestashop AND Master-tech.app"))
                                            .clicked() {
                                                let _ = self.ui_event_tx.try_send(ChatEvent::Edit(id.clone()));
                                            }
                                        }
                                    }
                                    
                                    if Button::new(RichText::new("🗐").color(btn_txt_color))
                                    .ui(ui)
                                    .clicked(){
                                        ui.ctx().copy_text(item.note.clone());
                                    }
                                    
                                    if self.current_user.is_admin() {
                                        if  Button::new(RichText::new("🗙").color(btn_txt_color))
                                        .ui(ui)
                                        .on_hover_text(
                                            RichText::new("WARNING, this will delete the note from prestashop AND Master-tech.app\nIf this is what you want, DOUBLE CLICK to delete")
                                                .strong()
                                                .color(Color32::LIGHT_RED)
                                        )
                                        .double_clicked() {
                                            let _ = self.ui_event_tx.try_send(ChatEvent::DeleteNote(item.clone()));
                                        }
                                    }

                                    ui.add_space(5.);
                                    ui.label(if item.private { "🕶" } else { "✔" });
                                });
                            });
                        }

                        Frame::new() // Frame for the actual note text itself // or for modifying the note
                            .fill(Color32::from_rgb(10,10,12))
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
                                if self.allow_edit.contains(&item.id.key_string()) {
                                    if let Some(msg) = self.edit_text.get_mut(&item.id.key_string()){
                                        TextEdit::multiline(&mut msg.note)
                                            .margin(Margin::symmetric(10, 3))
                                            .desired_width(max_msg_width)
                                            .show(ui);
                                    }
                                } else {
                                    viewer::easy_mark(ui, &item.note);
                                }
                            });
                        }).response;
                    });

                    let rm = &mut self.remove_hovered;
                    if rm.is_some() {
                        *rm = None;
                        self.hovered.remove(&item.id);
                    }
                })
                .response
                .hovered() {
                    self.hovered.insert(item.id.clone());
                } else {
                    self.remove_hovered = Some(item.id.clone());
                }
            });
        };
    }
}


pub fn popup_widget(btn_response: Response, style: Arc<Style>, item: &TaskNotePayload) {
    Popup::menu(&btn_response)
    .width(btn_response.rect.width().min(300.0))
    .align(RectAlign::BOTTOM)
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
    .show(|ui| {
        ui.vertical_centered_justified(|ui| {
            ui.set_width(300.0);
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("Task_note:{}", item.id.key_string()));
                });
            });
            ui.horizontal(|ui| {
                if let Some(task_id) = item.task_id.clone() {
                    ui.colored_label(style.visuals.hyperlink_color, "Task ID");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("Task:{}", task_id.key_string()));
                    });
                }
            });
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "Thread ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(item.id_customer_thread.clone().unwrap_or_default());
                });
            });
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "Employee ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(item.id_employee.clone().unwrap_or_default());
                });
            });
            ui.horizontal(|ui| {
                ui.colored_label(style.visuals.hyperlink_color, "User ID");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("User:{}", item.user.key_string()));
                });
            });
        });
    });
}


