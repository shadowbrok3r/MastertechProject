use std::str::FromStr;

use eframe::egui::{text::{CCursor, CCursorRange}, vec2, CursorIcon, Margin, TextEdit, Ui, UiKind};
use egui_json_tree::{
    delimiters::ExpandableDelimiter,
    pointer::JsonPointerSegment,
    render::{
        DefaultRender, RenderBaseValueContext, RenderContext, RenderExpandableDelimiterContext,
        RenderPropertyContext,
    },
    DefaultExpand, JsonTree, JsonTreeStyle, JsonTreeVisuals,
};
use serde_json::Value;

use super::QueryEditor;
use crate::ui_tools::{
    controls::{self, IconBox, IconButton},
    icons::svg,
    tokens::Tokens,
};

#[derive(Default)]
pub struct Editor {
    pub edit_events: Vec<EditEvent>,
    pub state: Option<EditState>,
}

pub trait Show {
    fn title(&self) -> &'static str;
    fn show(&mut self, ui: &mut Ui);
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

                    let valid_key = state.key == state.new_key_input
                        || document
                            .pointer(&state.object_pointer)
                            .and_then(|v| v.as_object())
                            .is_some_and(|obj| !obj.contains_key(&state.new_key_input));

                    let save = IconButton::new(svg::CHECK, IconBox::Sm)
                        .tip("Save key")
                        .enabled(valid_key)
                        .show(ui);
                    if save.clicked() {
                        edit_events.push(EditEvent::SaveObjectKeyEdit);
                    }

                    if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Cancel").clicked() {
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

                if controls::icon_button(ui, svg::CHECK, IconBox::Sm, false, "Save value").clicked()
                {
                    edit_events.push(EditEvent::SaveValueEdit);
                }

                if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Cancel").clicked() {
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
            .context_menu(|ui| controls::menu_body(ui, |ui| {
                if context.value.is_object()
                    && controls::menu_item(ui, svg::PLUS, "Add to object").clicked()
                {
                    self.edit_events.push(EditEvent::AddToObject {
                        pointer: context.pointer.to_json_pointer_string(),
                    });
                    ui.close_kind(UiKind::Menu);
                }

                if context.value.is_array()
                    && controls::menu_item(ui, svg::LIST_PLUS, "Add to array").clicked()
                {
                    self.edit_events.push(EditEvent::AddToArray {
                        pointer: context.pointer.to_json_pointer_string(),
                    });
                    ui.close_kind(UiKind::Menu);
                }

                if let Some(parent) = context.pointer.parent() {
                    if let JsonPointerSegment::Key(key) = &context.property {
                        if controls::menu_item(ui, svg::PENCIL_SIMPLE, "Edit key").clicked() {
                            self.state = Some(EditState::EditObjectKey(EditObjectKeyState {
                                key: key.to_string(),
                                object_pointer: parent.to_json_pointer_string(),
                                new_key_input: key.to_string(),
                                request_focus: true,
                                is_new_key: false,
                            }));
                            ui.close_kind(UiKind::Menu);
                        }
                    }

                    if controls::menu_item(ui, svg::TRASH, "Delete").clicked() {
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
                        ui.close_kind(UiKind::Menu);
                    }
                }
            }));
    }

    fn show_value_context_menu(
        &mut self,
        ui: &mut Ui,
        context: RenderBaseValueContext<'_, '_, Value>,
    ) {
        context
            .render_default(ui)
            .on_hover_cursor(CursorIcon::ContextMenu)
            .context_menu(|ui| controls::menu_body(ui, |ui| {
                if controls::menu_item(ui, svg::PENCIL, "Edit value").clicked() {
                    self.state = Some(EditState::EditValue(EditValueState {
                        pointer: context.pointer.to_json_pointer_string(),
                        new_value_input: context.value.to_string(),
                        request_focus: true,
                    }));
                    ui.close_kind(UiKind::Menu);
                }

                match (context.pointer.parent(), context.pointer.last()) {
                    (Some(parent), Some(JsonPointerSegment::Key(key))) => {
                        if controls::menu_item(ui, svg::TRASH, "Delete").clicked() {
                            self.edit_events.push(EditEvent::DeleteFromObject {
                                object_pointer: parent.to_json_pointer_string(),
                                key: key.to_string(),
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                    }
                    (Some(parent), Some(JsonPointerSegment::Index(idx))) => {
                        if controls::menu_item(ui, svg::TRASH, "Delete").clicked() {
                            self.edit_events.push(EditEvent::DeleteFromArray {
                                array_pointer: parent.to_json_pointer_string(),
                                idx: *idx,
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                    }
                    _ => {}
                };
            }));
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
                    .context_menu(|ui| controls::menu_body(ui, |ui| {
                        if controls::menu_item(ui, svg::LIST_PLUS, "Add to array").clicked() {
                            self.edit_events.push(EditEvent::AddToArray {
                                pointer: context.pointer.to_json_pointer_string(),
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                    }));
            }
            ExpandableDelimiter::OpeningObject => {
                context
                    .render_default(ui)
                    .on_hover_cursor(CursorIcon::ContextMenu)
                    .context_menu(|ui| controls::menu_body(ui, |ui| {
                        if controls::menu_item(ui, svg::PLUS, "Add to object").clicked() {
                            self.edit_events.push(EditEvent::AddToObject {
                                pointer: context.pointer.to_json_pointer_string(),
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                    }));
            }
            _ => {
                context.render_default(ui);
            }
        };
    }

    fn show_text_edit_with_focus(ui: &mut Ui, input: &mut String, request_focus: &mut bool) {
        let text_edit_output = TextEdit::singleline(input)
            .code_editor()
            .margin(Margin::symmetric(2, 0))
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

pub enum EditState {
    EditObjectKey(EditObjectKeyState),
    EditValue(EditValueState),
}

pub struct EditObjectKeyState {
    pub key: String,
    pub object_pointer: String,
    pub new_key_input: String,
    pub request_focus: bool,
    pub is_new_key: bool,
}

pub struct EditValueState {
    pub pointer: String,
    pub new_value_input: String,
    pub request_focus: bool,
}

pub enum EditEvent {
    DeleteFromObject { object_pointer: String, key: String },
    DeleteFromArray { array_pointer: String, idx: usize },
    AddToObject { pointer: String },
    AddToArray { pointer: String },
    SaveValueEdit,
    SaveObjectKeyEdit,
    CloseObjectKeyEdit,
    CloseValueEdit,
}

impl Show for QueryEditor {
    fn title(&self) -> &'static str {
        "JSON Editor"
    }

    fn show(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        JsonTree::new(self.title(), &self.response)
        .default_expand(DefaultExpand::ToLevel(4))
        .on_render(|ui, context| self.editor.show(ui, &self.response, context))
        .style(JsonTreeStyle {
            visuals: Some(
                JsonTreeVisuals {
                    bool_color: t.secondary,
                    object_key_color: t.tertiary,
                    array_idx_color: t.secondary,
                    number_color: t.success,
                    string_color: t.primary,
                    highlight_color: t.primary_soft,
                    punctuation_color: t.text_faint,
                    null_color: t.warn,
                }
            ),
            abbreviate_root: false,
            font_id: Some(eframe::egui::FontId::monospace(14.0)),
            ..Default::default()
        })
        .show(ui);

        self.editor.apply_events(&mut self.response);
    }
}