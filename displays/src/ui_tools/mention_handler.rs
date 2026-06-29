use eframe::egui::{
    text::{CCursor, CCursorRange, LayoutJob},
    Align, Button, Color32, Context, FontId, Id, Key, LayerId, Layout, Margin, Modifiers, Order,
    Popup, PopupAnchor, PopupCloseBehavior, RectAlign, Response, ScrollArea, TextBuffer, TextEdit,
    TextFormat, TextStyle, TextWrapMode, Ui, Vec2,
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use serde::{Deserialize, Serialize};
use std::{cmp::{min, Reverse}, collections::BTreeSet};

use crate::markdown_editor::highlighter::MemoizedEasymarkHighlighter;

use super::autocomplete::highlight_matches;

const MAX_SUGGESTIONS: usize = 10;
const MENTION_COLOR: Color32 = Color32::from_rgb(191, 33, 101);

/// Chat input that highlights `@username` mentions and shows an autocomplete
/// popup anchored above the text cursor. Drop-in replacement for the old
/// `EasyMarkEditor` used by `ChatView`.
#[derive(Debug, Clone)]
pub struct MentionHandler {
    message: String,
    /// Set of taggable handles, each already prefixed with `@`.
    pub inputs: BTreeSet<String>,
    pub private_note: bool,
    /// When false the "Private Note" toggle is hidden and notes stay non-private.
    pub allow_private: bool,
    highlighter: MemoizedEasymarkHighlighter,
}

impl Default for MentionHandler {
    fn default() -> Self {
        Self {
            message: String::new(),
            inputs: BTreeSet::new(),
            private_note: false,
            allow_private: true,
            highlighter: MemoizedEasymarkHighlighter::default(),
        }
    }
}

impl MentionHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&self) -> String {
        self.message.clone()
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    /// Renders the input, mention popup, and the submit row. Returns the submit
    /// button response.
    pub fn ui(&mut self, ui: &mut Ui) -> Option<Response> {
        let id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        let mut state = MentionHandlerState::load(ui.ctx(), id).unwrap_or_default();

        let up_pressed = state.focused
            && ui.input_mut(|i| i.consume_key(Modifiers::default(), Key::ArrowUp));
        let down_pressed = state.focused
            && ui.input_mut(|i| i.consume_key(Modifiers::default(), Key::ArrowDown));

        // Plain Enter / Tab accept the active suggestion (consumed before the
        // text edit so they don't insert a newline or move focus). Shift+Enter
        // is reserved for submit and is consumed by the caller beforehand.
        // open_bool popups don't register in Memory, so track open state here.
        let popup_open = state.popup_open;
        let key_accept = popup_open
            && state.selected_index.is_some()
            && (ui.input_mut(|i| i.consume_key(Modifiers::default(), Key::Enter))
                | ui.input_mut(|i| i.consume_key(Modifiers::default(), Key::Tab)));

        let reserve = 36.0;
        let max_height = (ui.available_height() - reserve).max(48.0);
        ScrollArea::vertical()
            .id_salt(("mention_scroll", id))
            .max_height(max_height)
            .show(ui, |ui| {
                self.text_ui(ui, id, &mut state, up_pressed, down_pressed, key_accept);
            });

        ui.separator();
        let submit = ui
            .horizontal(|ui| {
                if self.allow_private {
                    ui.checkbox(&mut self.private_note, "Private Note");
                } else {
                    self.private_note = false;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(Button::new("Submit").min_size(Vec2::new(60.0, 10.0)))
                })
                .inner
            })
            .inner;

        state.store(ui.ctx(), id);
        Some(submit)
    }

    fn text_ui(
        &mut self,
        ui: &mut Ui,
        id: Id,
        state: &mut MentionHandlerState,
        up_pressed: bool,
        down_pressed: bool,
        key_accept: bool,
    ) {
        let text_edit_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);

        let output = {
            let Self { message, highlighter, .. } = self;
            let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap_width: f32| {
                let mut job = highlighter.highlight(ui.style(), buf.as_str());
                job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            TextEdit::multiline(message)
                .id(text_edit_id)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .margin(Margin::symmetric(6, 3))
                .font(TextStyle::Monospace)
                .layouter(&mut layouter)
                .show(ui)
        };

        let text_response = output.response;
        let galley = output.galley;
        let galley_pos = output.galley_pos;
        let mut text_edit_state = output.state;
        state.focused = text_response.has_focus();

        // Find the mention fragment between the nearest '@' before the cursor
        // and the cursor itself, bailing out the moment whitespace appears.
        let mut match_results: Vec<(String, i64, Vec<usize>)> = Vec::new();
        let mut at_byte: Option<usize> = None;
        let mut cursor_byte = self.message.len();
        let mut cursor_char = self.message.chars().count();

        if let Some(range) = text_edit_state.cursor.char_range() {
            cursor_char = range.primary.index.0.min(self.message.chars().count());
            cursor_byte = self
                .message
                .char_indices()
                .nth(cursor_char)
                .map(|(i, _)| i)
                .unwrap_or(self.message.len());

            let before = &self.message[..cursor_byte];
            if let Some(at) = before.rfind('@') {
                let frag = &before[at + 1..];
                if !frag.chars().any(|c| c.is_whitespace()) {
                    at_byte = Some(at);
                    if frag.is_empty() {
                        match_results = self
                            .inputs
                            .iter()
                            .map(|s| (s.clone(), 0i64, Vec::new()))
                            .collect();
                    } else {
                        let matcher = SkimMatcherV2::default().ignore_case();
                        match_results = self
                            .inputs
                            .iter()
                            .filter_map(|s| {
                                matcher
                                    .fuzzy_indices(s.as_str(), frag)
                                    .map(|(score, indices)| (s.clone(), score, indices))
                            })
                            .collect();
                        match_results.sort_by_key(|k| Reverse(k.1));
                    }
                }
            }
        }

        if text_response.changed()
            || state.selected_index.map_or(false, |i| i >= match_results.len())
        {
            state.selected_index = None;
        }
        state.update_index(down_pressed, up_pressed, match_results.len(), MAX_SUGGESTIONS);
        if state.selected_index.is_none() && !match_results.is_empty() {
            state.selected_index = Some(0);
        }

        let mut accept_index = if key_accept { state.selected_index } else { None };
        let mut clicked_index: Option<usize> = None;
        let open_now = at_byte.is_some() && !match_results.is_empty() && text_response.has_focus();
        state.popup_open = open_now;
        let mut open = open_now;

        // Fixed popup width sized to the widest suggestion so each username
        // stays on its own row and hovering can't reflow the list.
        let suggestion_font = FontId::default();
        let content_width = match_results
            .iter()
            .take(MAX_SUGGESTIONS)
            .map(|(candidate, _, _)| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(candidate.clone(), suggestion_font.clone(), Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0_f32, f32::max);
        let popup_width = content_width + 24.0;

        let anchor = galley
            .pos_from_cursor(CCursor::new(cursor_char))
            .translate(galley_pos.to_vec2())
            .left_top();

        Popup::new(
            id,
            ui.ctx().clone(),
            PopupAnchor::Position(anchor),
            LayerId::new(Order::Foreground, id),
        )
        .width(popup_width)
        .align(RectAlign::TOP_START)
        .gap(4.0)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .open_bool(&mut open)
        .show(|ui| {
            ui.set_min_width(popup_width);
            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
            for (i, (candidate, _, indices)) in match_results.iter().take(MAX_SUGGESTIONS).enumerate()
            {
                let response = ui.selectable_label(
                    state.selected_index == Some(i),
                    highlight_matches(candidate.as_str(), indices, MENTION_COLOR),
                );
                if response.hovered() {
                    state.selected_index = Some(i);
                }
                if response.clicked() {
                    clicked_index = Some(i);
                }
            }
        });

        if clicked_index.is_some() {
            accept_index = clicked_index;
        }

        if let (Some(index), Some(at)) = (accept_index, at_byte) {
            if index < match_results.len() {
                let replacement = format!("{} ", match_results[index].0);
                let at_char = self.message[..at].chars().count();
                self.message.replace_range(at..cursor_byte, &replacement);

                let new_char = at_char + replacement.chars().count();
                let mut cursor = text_edit_state.cursor.clone();
                cursor.set_char_range(Some(CCursorRange::one(CCursor::new(new_char))));
                text_edit_state.cursor = cursor;
                text_edit_state.store(ui.ctx(), text_edit_id);

                text_response.request_focus();
                state.selected_index = None;
                state.popup_open = false;
            }
        }
    }
}

/// Builds a read-only layout that colors `@username` mentions while leaving the
/// rest of the note as plain text (no markdown rendering).
pub fn mention_label_job(ui: &Ui, text: &str, wrap_width: f32) -> LayoutJob {
    let font = TextStyle::Body.resolve(ui.style());
    let base = TextFormat::simple(font.clone(), ui.visuals().text_color());
    let mention = TextFormat::simple(font, super::theme::accent_secondary(ui));

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('@') {
            Some(at) => {
                if at > 0 {
                    job.append(&rest[..at], 0.0, base.clone());
                }
                let after = &rest[at..];
                let end = after[1..]
                    .find(|c: char| c.is_whitespace())
                    .map_or(after.len(), |i| i + 1);
                job.append(&after[..end], 0.0, mention.clone());
                rest = &after[end..];
            }
            None => {
                job.append(rest, 0.0, base.clone());
                break;
            }
        }
    }
    job
}

/// Stores the currently selected suggestion index in egui state.
#[derive(Clone, Default, Serialize, Deserialize)]
struct MentionHandlerState {
    /// Currently selected index, `None` if nothing is selected.
    selected_index: Option<usize>,
    /// Whether the text edit was focused last frame.
    focused: bool,
    /// Whether the suggestion popup was open last frame.
    popup_open: bool,
}

impl MentionHandlerState {
    fn store(self, ctx: &Context, id: Id) {
        ctx.data_mut(|d| d.insert_persisted(id, self));
    }

    fn load(ctx: &Context, id: Id) -> Option<Self> {
        ctx.data_mut(|d: &mut eframe::egui::util::IdTypeMap| d.get_persisted(id))
    }

    /// Moves the selection within bounds in response to arrow keys.
    fn update_index(
        &mut self,
        down_pressed: bool,
        up_pressed: bool,
        match_results_count: usize,
        max_suggestions: usize,
    ) {
        self.selected_index = match self.selected_index {
            Some(index) if down_pressed => {
                if index + 1 < min(match_results_count, max_suggestions) {
                    Some(index + 1)
                } else {
                    Some(index)
                }
            }
            Some(index) if up_pressed => {
                if index == 0 {
                    None
                } else {
                    Some(index - 1)
                }
            }
            None if down_pressed => Some(0),
            Some(index) => Some(index),
            None => None,
        }
    }
}
