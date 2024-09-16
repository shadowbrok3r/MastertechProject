use crate::app_state::MtechServerContext;
use anyhow::{Error, Result};
use core::f32;
use database::{schema::helper_traits::UserHelper, DATABASE};
use eframe::egui::{
    text::{CCursor, CCursorRange},
    vec2, Align, CentralPanel, Color32, CursorIcon, Frame, Layout, Margin, ScrollArea, SidePanel,
    TextEdit, TextStyle, TopBottomPanel, Ui,
};
use egui_json_tree::{
    delimiters::ExpandableDelimiter,
    pointer::JsonPointerSegment,
    render::{
        DefaultRender, RenderBaseValueContext, RenderContext, RenderExpandableDelimiterContext,
        RenderPropertyContext,
    },
    DefaultExpand, JsonTree, JsonTreeStyle,
};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use wasm_bindgen_futures::spawn_local;

#[derive(Default)]
pub enum JsonEditorState {
    #[default]
    SettingsPage,
    TasksPage,
    CustomersPage,
    ComputersPage,
}

impl MtechServerContext {
    pub fn json_viewer(&mut self, ui: &mut Ui) {
        let s_frame = Frame::default();
        s_frame.inner_margin(Margin::same(20.));
        s_frame.outer_margin(Margin::same(10.));
        SidePanel::left("left-panel")
            .frame(s_frame)
            .max_width(130.)
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.add_space(5.);
                    let settings = ui.button("Settings");
                    ui.add_space(5.);
                    let task = ui.button("Tasks");
                    ui.add_space(5.);
                    let customers = ui.button("Customers");
                    ui.add_space(5.);
                    let computers = ui.button("Computers");

                    ui.add_space(ui.available_height() - 30.);
                    let submit = ui.button("Submit");

                    // let customers = &self.data_output.customers;
                    // let computers = &self.data_output.computers;
                    // let services = &self.data_output.tickets;

                    if settings.clicked() {
                        self.json_editor_state = JsonEditorState::SettingsPage;
                        self.json_editor
                            .set_value(self.user_settings.clone())
                            .unwrap();
                    }

                    if task.clicked() {
                        self.json_editor_state = JsonEditorState::TasksPage;
                        // self.json_editor.set_value(self.).unwrap();
                    }

                    if customers.clicked() {
                        self.json_editor_state = JsonEditorState::CustomersPage;
                    }

                    if computers.clicked() {
                        self.json_editor_state = JsonEditorState::ComputersPage;
                        // self.json_editor.set_value(self.tasks.clone()).unwrap();
                    }

                    if submit.clicked() {
                        self.user_settings = serde_json::from_value(self.json_editor.value.clone())
                            .unwrap_or_default();

                        if let Some(mut usr) = self.current_user.clone() {
                            usr.user_settings = Some(self.user_settings.clone());
                            spawn_local(async move {
                                match usr.save_user_settings().await {
                                    Ok(_) => info!("Updated User Settings"),
                                    Err(e) => info!("Error updating User Settings: {e:?}"),
                                }
                            });
                        }
                    }

                    if self.json_editor.value.is_null() {
                        self.json_editor.set_value(self.user_settings.clone());
                    }
                });
            });

        TopBottomPanel::top("top-panel").show_inside(ui, |ui| {
            ui.vertical_centered(|ui| ui.heading("Json Editor"));
        });

        let c_frame = Frame::default();
        c_frame.inner_margin(Margin::same(10.));

        CentralPanel::default()
            .frame(c_frame)
            .show_inside(ui, |ui| {
                let available_height = ui.available_height();
                let font_id = TextStyle::Body.resolve(ui.style());
                let row_height = ui.fonts(|f| f.row_height(&font_id)) + ui.spacing().item_spacing.y;
                let total_rows = (available_height / row_height).floor() as usize;
                ScrollArea::new([false, true])
                    .max_width(f32::INFINITY)
                    .auto_shrink(false)
                    .show_rows(ui, row_height, total_rows, |ui, _row_range| {
                        self.json_editor.show(ui);
                    });
            });
    }
}

trait Show {
    fn title(&self) -> &'static str;
    fn show(&mut self, ui: &mut Ui);
}

#[derive(Default)]
pub struct JsonEditor {
    pub value: Value,
    pub editor: Editor,
}

impl JsonEditor {
    fn new(value: Value) -> Self {
        Self {
            value,
            editor: Default::default(),
        }
    }

    fn set_value<T: Serialize + for<'de> Deserialize<'de>>(
        &mut self,
        data: T,
    ) -> Result<(), Error> {
        self.value = serde_json::to_value(&data)?;
        Ok(())
    }
}

#[derive(Default)]
struct Editor {
    edit_events: Vec<EditEvent>,
    state: Option<EditState>,
}

impl Editor {
    fn show(&mut self, ui: &mut Ui, document: &Value, context: RenderContext<'_, '_, Value>) {
        match self.state.as_mut() {
            Some(EditState::EditObjectKey(state)) => {
                Self::show_edit_object_key(ui, document, context, state, &mut self.edit_events)
            }
            Some(EditState::EditValue(state)) => {
                Self::show_edit_value(ui, context, state, &mut self.edit_events);
            }
            None => {
                self.show_with_context_menus(ui, context);
            }
        };
    }

    fn show_edit_object_key(
        ui: &mut Ui,
        document: &Value,
        context: RenderContext<Value>,
        state: &mut EditObjectKeyState,
        edit_events: &mut Vec<EditEvent>,
    ) {
        if let RenderContext::Property(context) = &context {
            if let JsonPointerSegment::Key(key) = context.property {
                if key == state.key
                    && context
                        .pointer
                        .parent()
                        .map(|parent| parent.to_json_pointer_string())
                        .is_some_and(|object_pointer| object_pointer == state.object_pointer)
                {
                    Self::show_text_edit_with_focus(
                        ui,
                        &mut state.new_key_input,
                        &mut state.request_focus,
                    );

                    ui.add_space(5.0);

                    let valid_key = state.key == state.new_key_input
                        || document
                            .pointer(&state.object_pointer)
                            .and_then(|v| v.as_object())
                            .is_some_and(|obj| !obj.contains_key(&state.new_key_input));

                    ui.add_enabled_ui(valid_key, |ui| {
                        if ui.small_button("✅").clicked() {
                            edit_events.push(EditEvent::SaveObjectKeyEdit);
                        }
                    });

                    ui.add_space(5.0);

                    if ui.small_button("❌").clicked() {
                        if state.is_new_key {
                            edit_events.push(EditEvent::DeleteFromObject {
                                object_pointer: state.object_pointer.to_string(),
                                key: key.to_string(),
                            });
                        }
                        edit_events.push(EditEvent::CloseObjectKeyEdit);
                    }
                    return;
                }
            }
        }
        context.render_default(ui);
    }

    fn show_edit_value(
        ui: &mut Ui,
        context: RenderContext<Value>,
        state: &mut EditValueState,
        edit_events: &mut Vec<EditEvent>,
    ) {
        if let RenderContext::BaseValue(context) = &context {
            if state.pointer == context.pointer.to_json_pointer_string() {
                Self::show_text_edit_with_focus(
                    ui,
                    &mut state.new_value_input,
                    &mut state.request_focus,
                );

                ui.add_space(5.0);

                if ui.small_button("✅").clicked() {
                    edit_events.push(EditEvent::SaveValueEdit);
                }

                ui.add_space(5.0);

                if ui.small_button("❌").clicked() {
                    edit_events.push(EditEvent::CloseValueEdit);
                }
                return;
            }
        }
        context.render_default(ui);
    }

    fn show_with_context_menus(&mut self, ui: &mut Ui, context: RenderContext<Value>) {
        match context {
            RenderContext::Property(context) => {
                self.show_property_context_menu(ui, context);
            }
            RenderContext::BaseValue(context) => {
                self.show_value_context_menu(ui, context);
            }
            RenderContext::ExpandableDelimiter(context) => {
                self.show_expandable_delimiter_context_menu(ui, context);
            }
        };
    }

    fn show_property_context_menu(
        &mut self,
        ui: &mut Ui,
        context: RenderPropertyContext<'_, '_, Value>,
    ) {
        context
            .render_default(ui)
            .on_hover_cursor(CursorIcon::ContextMenu)
            .context_menu(|ui| {
                if context.value.is_object() && ui.button("Add to object").clicked() {
                    self.edit_events.push(EditEvent::AddToObject {
                        pointer: context.pointer.to_json_pointer_string(),
                    });
                    ui.close_menu();
                }

                if context.value.is_array() && ui.button("Add to array").clicked() {
                    self.edit_events.push(EditEvent::AddToArray {
                        pointer: context.pointer.to_json_pointer_string(),
                    });
                    ui.close_menu();
                }

                if let Some(parent) = context.pointer.parent() {
                    if let JsonPointerSegment::Key(key) = &context.property {
                        if ui.button("Edit key").clicked() {
                            self.state = Some(EditState::EditObjectKey(EditObjectKeyState {
                                key: key.to_string(),
                                object_pointer: parent.to_json_pointer_string(),
                                new_key_input: key.to_string(),
                                request_focus: true,
                                is_new_key: false,
                            }));
                            ui.close_menu()
                        }
                    }

                    if ui.button("Delete").clicked() {
                        let event = match context.property {
                            JsonPointerSegment::Key(key) => EditEvent::DeleteFromObject {
                                object_pointer: parent.to_json_pointer_string(),
                                key: key.to_string(),
                            },
                            JsonPointerSegment::Index(idx) => EditEvent::DeleteFromArray {
                                array_pointer: parent.to_json_pointer_string(),
                                idx,
                            },
                        };
                        self.edit_events.push(event);
                        ui.close_menu();
                    }
                }
            });
    }

    fn show_value_context_menu(
        &mut self,
        ui: &mut Ui,
        context: RenderBaseValueContext<'_, '_, Value>,
    ) {
        context
            .render_default(ui)
            .on_hover_cursor(CursorIcon::ContextMenu)
            .context_menu(|ui| {
                if ui.button("Edit value").clicked() {
                    self.state = Some(EditState::EditValue(EditValueState {
                        pointer: context.pointer.to_json_pointer_string(),
                        new_value_input: context.value.to_string(),
                        request_focus: true,
                    }));
                    ui.close_menu();
                }

                match (context.pointer.parent(), context.pointer.last()) {
                    (Some(parent), Some(JsonPointerSegment::Key(key))) => {
                        if ui.button("Delete").clicked() {
                            self.edit_events.push(EditEvent::DeleteFromObject {
                                object_pointer: parent.to_json_pointer_string(),
                                key: key.to_string(),
                            });
                            ui.close_menu();
                        }
                    }
                    (Some(parent), Some(JsonPointerSegment::Index(idx))) => {
                        if ui.button("Delete").clicked() {
                            self.edit_events.push(EditEvent::DeleteFromArray {
                                array_pointer: parent.to_json_pointer_string(),
                                idx: *idx,
                            });
                            ui.close_menu();
                        }
                    }
                    _ => {}
                };
            });
    }

    fn show_expandable_delimiter_context_menu(
        &mut self,
        ui: &mut Ui,
        context: RenderExpandableDelimiterContext<'_, '_, Value>,
    ) {
        match context.delimiter {
            ExpandableDelimiter::OpeningArray => {
                context
                    .render_default(ui)
                    .on_hover_cursor(CursorIcon::ContextMenu)
                    .context_menu(|ui| {
                        if ui.button("Add to array").clicked() {
                            self.edit_events.push(EditEvent::AddToArray {
                                pointer: context.pointer.to_json_pointer_string(),
                            });
                            ui.close_menu();
                        }
                    });
            }
            ExpandableDelimiter::OpeningObject => {
                context
                    .render_default(ui)
                    .on_hover_cursor(CursorIcon::ContextMenu)
                    .context_menu(|ui| {
                        if ui.button("Add to object").clicked() {
                            self.edit_events.push(EditEvent::AddToObject {
                                pointer: context.pointer.to_json_pointer_string(),
                            });
                            ui.close_menu();
                        }
                    });
            }
            _ => {
                context.render_default(ui);
            }
        };
    }

    fn show_text_edit_with_focus(ui: &mut Ui, input: &mut String, request_focus: &mut bool) {
        let text_edit_output = TextEdit::singleline(input)
            .code_editor()
            .margin(Margin::symmetric(2.0, 0.0))
            .clip_text(false)
            .desired_width(0.0)
            .min_size(vec2(10.0, 2.0))
            .show(ui);

        if *request_focus {
            *request_focus = false;
            let text_edit_id = text_edit_output.response.id;
            if let Some(mut text_edit_state) = TextEdit::load_state(ui.ctx(), text_edit_id) {
                text_edit_state
                    .cursor
                    .set_char_range(Some(CCursorRange::two(
                        CCursor::new(0),
                        CCursor::new(input.len()),
                    )));
                text_edit_state.store(ui.ctx(), text_edit_id);
                ui.ctx().memory_mut(|mem| mem.request_focus(text_edit_id));
            }
        }
    }

    fn apply_events(&mut self, document: &mut Value) {
        for event in self.edit_events.drain(..) {
            match event {
                EditEvent::DeleteFromArray { array_pointer, idx } => {
                    if let Some(arr) = document
                        .pointer_mut(&array_pointer)
                        .and_then(|value| value.as_array_mut())
                    {
                        arr.remove(idx);
                    }
                }
                EditEvent::DeleteFromObject {
                    object_pointer,
                    key,
                } => {
                    if let Some(obj) = document
                        .pointer_mut(&object_pointer)
                        .and_then(|value| value.as_object_mut())
                    {
                        obj.remove(&key);
                    }
                }
                EditEvent::AddToObject { pointer } => {
                    if let Some(obj) = document
                        .pointer_mut(&pointer)
                        .and_then(|value| value.as_object_mut())
                    {
                        let mut counter = 0;
                        let mut new_key = "new_key".to_string();

                        while obj.contains_key(&new_key) {
                            counter += 1;
                            new_key = format!("new_key_{counter}");
                        }

                        obj.insert(new_key.clone(), Value::String(String::new()));

                        self.state = Some(EditState::EditObjectKey(EditObjectKeyState {
                            key: new_key.clone(),
                            object_pointer: pointer,
                            new_key_input: new_key,
                            request_focus: true,
                            is_new_key: true,
                        }));
                    }
                }
                EditEvent::AddToArray { pointer } => {
                    if let Some(arr) = document
                        .pointer_mut(&pointer)
                        .and_then(|value| value.as_array_mut())
                    {
                        arr.push(Value::String(String::new()));
                    }
                }
                EditEvent::SaveValueEdit => {
                    if let Some(EditState::EditValue(value_edit)) = self.state.take() {
                        if let Some(value) = document.pointer_mut(&value_edit.pointer) {
                            match Value::from_str(&value_edit.new_value_input) {
                                Ok(new_value) => *value = new_value,
                                Err(_) => *value = Value::String(value_edit.new_value_input),
                            }
                        }
                    }
                }
                EditEvent::SaveObjectKeyEdit => {
                    if let Some(EditState::EditObjectKey(object_key_edit)) = self.state.take() {
                        let obj = document
                            .pointer_mut(&object_key_edit.object_pointer)
                            .and_then(|value| value.as_object_mut());

                        if let Some(obj) = obj {
                            if let Some(value) = obj.remove(&object_key_edit.key) {
                                obj.insert(object_key_edit.new_key_input, value);
                            }
                        }
                    }
                }
                EditEvent::CloseObjectKeyEdit | EditEvent::CloseValueEdit => {
                    self.state.take();
                }
            }
        }
    }
}

enum EditState {
    EditObjectKey(EditObjectKeyState),
    EditValue(EditValueState),
}

struct EditObjectKeyState {
    key: String,
    object_pointer: String,
    new_key_input: String,
    request_focus: bool,
    is_new_key: bool,
}

struct EditValueState {
    pointer: String,
    new_value_input: String,
    request_focus: bool,
}

enum EditEvent {
    DeleteFromObject { object_pointer: String, key: String },
    DeleteFromArray { array_pointer: String, idx: usize },
    AddToObject { pointer: String },
    AddToArray { pointer: String },
    SaveValueEdit,
    SaveObjectKeyEdit,
    CloseObjectKeyEdit,
    CloseValueEdit,
}

impl Show for JsonEditor {
    fn title(&self) -> &'static str {
        "JSON Editor"
    }

    fn show(&mut self, ui: &mut Ui) {
        JsonTree::new(self.title(), &self.value)
            .abbreviate_root(true)
            .default_expand(DefaultExpand::All)
            .on_render(|ui, context| self.editor.show(ui, &self.value, context))
            .style(JsonTreeStyle {
                bool_color: Color32::LIGHT_BLUE,
                object_key_color: Color32::LIGHT_GREEN,
                array_idx_color: Color32::from_rgb(120, 20, 120),
                number_color: Color32::GREEN,
                string_color: Color32::from_rgb(120, 20, 120),
                highlight_color: Color32::from_rgba_premultiplied(120, 20, 120, 100),
                punctuation_color: Color32::LIGHT_RED,
                ..Default::default()
            })
            .show(ui);

        self.editor.apply_events(&mut self.value);
    }
}
