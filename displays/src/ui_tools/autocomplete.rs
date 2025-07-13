use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{LayerId, Margin, Order, PopupAnchor, RectAlign, TextBuffer};
use eframe::egui::{
    Popup,
    text::LayoutJob,
    Color32,
    Context,
    FontId,
    Id,
    Key,
    Modifiers,
    PopupCloseBehavior,
    Response,
    TextEdit,
    TextFormat,
    Ui,
    Widget, // TextBuffer
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    cmp::{min, Reverse},
    fmt::Debug,
};

/// Trait that can be used to modify the TextEdit
type SetTextEditProperties = dyn FnOnce(TextEdit) -> TextEdit;

/// An extension to the [`egui::TextEdit`] that allows for a dropdown box with autocomplete to popup while typing.
pub struct AutoCompleteTextEdit<'a, T> {
    /// Contents of text edit passed into [`egui::TextEdit`]
    text_field: &'a mut String,
    /// Data to use as the search term
    search: T,
    /// A limit that can be placed on the maximum number of autocomplete suggestions shown
    max_suggestions: usize,
    /// If true, highlights the macthing indices in the dropdown
    highlight: bool,
    /// Used to set properties on the internal TextEdit
    set_properties: Option<Box<SetTextEditProperties>>,
    filter: Option<Box<dyn Fn(&str) -> bool>>,
    layouter: Option<&'a mut dyn FnMut(&Ui, &dyn eframe::egui::TextBuffer, f32) -> Arc<eframe::egui::Galley>>,
}

impl<'a, T, S> AutoCompleteTextEdit<'a, T>
where
    T: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    /// Creates a new [`AutoCompleteTextEdit`].
    ///
    /// `text_field` - Contents of the text edit passed into [`egui::TextEdit`]
    /// `search` - Data use as the search term
    pub fn new(text_field: &'a mut String, search: T) -> Self {
        Self {
            text_field,
            search,
            max_suggestions: 10,
            highlight: false,
            set_properties: None,
            filter: None,
            layouter: None,
        }
    }
}

impl<'a, T, S> AutoCompleteTextEdit<'a, T>
where
    T: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    /// This determines the number of options appear in the dropdown menu
    pub fn max_suggestions(mut self, max_suggestions: usize) -> Self {
        self.max_suggestions = max_suggestions;
        self
    }

    /// If set to true, characters will be highlighted in the dropdown to show the match
    pub fn highlight_matches(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    /// Can be used to set the properties of the internal [`egui::TextEdit`]
    /// # Example
    /// ```rust
    /// # use egui_autocomplete::AutoCompleteTextEdit;
    /// # fn make_text_edit(mut search_field: String, inputs: Vec<String>) {
    /// AutoCompleteTextEdit::new(&mut search_field, &inputs)
    ///     .set_text_edit_properties(|text_edit: egui::TextEdit<'_>| {
    ///         text_edit
    ///             .hint_text("Hint Text")
    ///             .text_color(egui::Color32::RED)
    ///     });
    /// # }
    /// ```
    pub fn set_text_edit_properties(
        mut self,
        set_properties: impl FnOnce(TextEdit) -> TextEdit + 'static,
    ) -> Self {
        self.set_properties = Some(Box::new(set_properties));
        self
    }

    /// Sets a filter function to filter the search results.
    pub fn set_filter<F>(mut self, filter: F) -> Self
    where
        F: Fn(&str) -> bool + 'static,
    {
        self.filter = Some(Box::new(filter));
        self
    }

    /// Sets the layouter function for custom text layout.
    pub fn layouter(
        mut self,
        layouter: &'a mut dyn FnMut(&Ui, &dyn eframe::egui::TextBuffer, f32) -> Arc<eframe::egui::Galley>,
    ) -> Self {
        self.layouter = Some(layouter);
        self
    }
}

impl<'a, T, S> Widget for AutoCompleteTextEdit<'a, T>
where
    T: IntoIterator<Item = S>,
    S: AsRef<str> + Debug,
{
    /// The response returned is the response from the internal text_edit
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            text_field,
            search,
            max_suggestions,
            highlight,
            set_properties,
            filter,
            layouter,
        } = self;

        let id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        let mut state = AutoCompleteTextEditState::load(ui.ctx(), id).unwrap_or_default();

        // only consume up/down presses if the text box is focused. This overwrites default behavior
        // to move to start/end of the string
        let up_pressed = state.focused
            && ui.input_mut(|input| input.consume_key(Modifiers::default(), Key::ArrowUp));
        let down_pressed = state.focused
            && ui.input_mut(|input| input.consume_key(Modifiers::default(), Key::ArrowDown));

        let text_edit_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);

        let mut text_edit = TextEdit::singleline(text_field).margin(Margin::symmetric(10, 3)).id(text_edit_id);
        if let Some(set_properties) = set_properties {
            text_edit = set_properties(text_edit);
        }
        if let Some(layouter) = layouter {
            text_edit = text_edit.layouter(layouter);
        }
        let text_output = text_edit.show(ui);
        let text_response = text_output.response;
        let mut text_edit_state = text_output.state;
        state.focused = text_response.has_focus();
        // Get cursor position and extract substring
        let mut match_results = Vec::new();
        let mut trigger_char_position = None;
        let mut cursor_char_index = 0;

        if let Some(ccursor_range) = text_edit_state.cursor.char_range() {
            // Get the primary cursor position
            cursor_char_index = ccursor_range.primary.index;

            // Ensure cursor_char_index is within bounds
            cursor_char_index = cursor_char_index.min(text_field.chars().count());

            // Convert cursor_char_index to byte index
            let cursor_byte_index = text_field
                .char_indices()
                .nth(cursor_char_index)
                .map(|(i, _)| i)
                .unwrap_or_else(|| text_field.len());

            let text_before_cursor = &text_field[..cursor_byte_index];

            trigger_char_position = text_before_cursor.rfind('@');

            let matcher = SkimMatcherV2::default().ignore_case();
            if let Some(at_byte_pos) = trigger_char_position {
                let match_text = &text_field[at_byte_pos + 1..cursor_byte_index];

                if !match_text.is_empty() {

                    match_results = search
                        .into_iter()
                        .filter(|s| filter.as_ref().map_or(true, |f| f(s.as_ref())))
                        .filter_map(|s| {
                            let score = matcher.fuzzy_indices(s.as_ref(), match_text);
                            score.map(|(score, indices)| (s, score, indices))
                        })
                        .collect::<Vec<_>>();

                    match_results.sort_by_key(|k| Reverse(k.1));
                }
            } else {
                if !text_field.is_empty() {
                    match_results = search
                        .into_iter()
                        .filter_map(|s| {
                            let score = matcher.fuzzy_indices(s.as_ref(), text_field.as_str());
                            score.map(|(score, indices)| (s, score, indices))
                        })
                        .collect::<Vec<_>>();
                    match_results.sort_by_key(|k| Reverse(k.1)); 
                }
            }
        }

        if text_response.changed()
            || (state.selected_index.is_some()
                && state.selected_index.unwrap() >= match_results.len())
        {
            state.selected_index = None;
        }

        state.update_index(
            down_pressed,
            up_pressed,
            match_results.len(),
            max_suggestions,
        );

        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter))
            || ui.input_mut(|input| input.key_pressed(Key::Tab));

        // if let (Some(index), true) = (
        //     state.selected_index,
        //     // If accepted by keyboard, close the popup. If the popup is closed with a selected index, take that text
        //     accepted_by_keyboard || !ui.memory(|mem| mem.is_popup_open(id)),
        // ) {
        //     text_field.replace_with(match_results[index].0.as_ref());
        //     state.selected_index = None;
        // }

        // if accepted_by_keyboard {
        //     text_response.request_focus()
        // }

        let ctx = ui.ctx();

        let open = &mut false;
        if let (Some(index), true) = (
            state.selected_index,
            accepted_by_keyboard || !Popup::is_id_open(&ctx, id),
        ) {
            if let Some(at_char_index) = trigger_char_position {
                let selected_text = match_results[index].0.as_ref();
                text_response.request_focus();
                // Replace from '@' to cursor position with the selected text
                // Convert character indices to byte indices for slicing
                let at_byte_index = text_field
                    .char_indices()
                    .nth(at_char_index)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let cursor_byte_index = text_field
                    .char_indices()
                    .nth(cursor_char_index)
                    .map(|(i, _)| i)
                    .unwrap_or_else(|| text_field.len());
                text_field.replace_range(at_byte_index..cursor_byte_index, &selected_text);

                // Calculate the new cursor character index
                let inserted_text_length = selected_text.chars().count();
                let new_cursor_char_index = at_char_index + inserted_text_length;

                // Update the TextEditState cursor
                let new_ccursor = CCursor::new(new_cursor_char_index);
                let mut new_cursor_state = text_edit_state.cursor.clone();
                let cursor_range = CCursorRange::one(new_ccursor);
                new_cursor_state.set_char_range(Some(cursor_range));
                text_edit_state.cursor = new_cursor_state;

                // Store the updated TextEditState
                text_edit_state.store(&ctx, text_edit_id);
            } else {
                if match_results.len() > 0 {
                    text_field.replace_with(match_results[index].0.as_ref());
                }
            }
            state.selected_index = None;
        }

        if !match_results.is_empty() && text_response.has_focus() {
            *open = true;
        } else {
            if Popup::is_id_open(&ctx, id) {
                *open = false;
            }
        }
        
        if !text_field.as_str().is_empty() && text_response.has_focus() && !match_results.is_empty()
        {
            *open = true;
        } else {
            if Popup::is_id_open(&ctx, id) {
                *open = false;
            }
        }

        Popup::new(
            id, 
            ctx.clone(), 
            PopupAnchor::from(&text_response), 
            LayerId::new(Order::Foreground, id)
        )
        .width(text_response.rect.width().max(250.0))
        .align(RectAlign::BOTTOM)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .open_bool(open)
        .show(|ui| {
            for (i, (output, _, match_indices)) in
                match_results.iter().take(max_suggestions).enumerate()
            {
                let mut selected = if let Some(x) = state.selected_index {
                    x == i
                } else {
                    false
                };

                let text = if highlight {
                    highlight_matches(
                        output.as_ref(),
                        match_indices,
                        Color32::from_rgb(191, 33, 101),
                    )
                } else {
                    let mut job = LayoutJob::default();
                    job.append(output.as_ref(), 0.0, TextFormat::default());
                    job
                };
                //  Update selected index based on hover
                if ui.toggle_value(&mut selected, text).hovered() {
                    state.selected_index = Some(i);
                    // text_field.replace_with(match_results[index].0.as_ref());
                }
                // if ui.toggle_value(&mut selected, text).clicked() {
                //     text_field.replace_with(output.as_ref());
                // }
            }
        });

        state.store(&ctx, id);

        text_response
    }
}

/// Highlights all the match indices in the provided text
pub fn highlight_matches(text: &str, match_indices: &[usize], color: Color32) -> LayoutJob {
    let mut formatted = LayoutJob::default();
    let mut it = text.char_indices().enumerate().peekable();
    // Iterate through all indices in the string
    while let Some((char_idx, (byte_idx, c))) = it.next() {
        let start = byte_idx;
        let mut end = byte_idx + (c.len_utf8() - 1);
        let match_state = match_indices.contains(&char_idx);
        // Find all consecutive characters that have the same state
        while let Some((peek_char_idx, (_, k))) = it.peek() {
            if match_state == match_indices.contains(peek_char_idx) {
                end += k.len_utf8();
                // Advance the iterator, we already peeked the value so it is fine to ignore
                _ = it.next();
            } else {
                break;
            }
        }
        // Format current slice based on the state
        let format = if match_state {
            TextFormat::simple(FontId::default(), color)
        } else {
            TextFormat::default()
        };
        let slice = &text[start..=end];
        formatted.append(slice, 0.0, format);
    }
    formatted
}

/// Stores the currently selected index in egui state
#[derive(Clone, Default, Serialize, Deserialize)]
struct AutoCompleteTextEditState {
    /// Currently selected index, is `None` if nothing is selected
    selected_index: Option<usize>,
    /// Whether or not the text edit was focused last frame
    focused: bool,
}

impl AutoCompleteTextEditState {
    /// Store the state with egui
    fn store(self, ctx: &Context, id: Id) {
        ctx.data_mut(|d| d.insert_persisted(id, self));
    }

    /// Get the state from egui if it exists
    fn load(ctx: &Context, id: Id) -> Option<Self> {
        ctx.data_mut(|d: &mut eframe::egui::util::IdTypeMap| d.get_persisted(id))
    }

    /// Updates in selected index, checks to make sure nothing goes out of bounds
    fn update_index(
        &mut self,
        down_pressed: bool,
        up_pressed: bool,
        match_results_count: usize,
        max_suggestions: usize,
    ) {
        self.selected_index = match self.selected_index {
            // Increment selected index when down is pressed, limit it to the number of matches and max_suggestions
            Some(index) if down_pressed => {
                if index + 1 < min(match_results_count, max_suggestions) {
                    Some(index + 1)
                } else {
                    Some(index)
                }
            }
            // Decrement selected index if up is pressed. Deselect if at first index
            Some(index) if up_pressed => {
                if index == 0 {
                    None
                } else {
                    Some(index - 1)
                }
            }
            // If nothing is selected and down is pressed, select first item
            None if down_pressed => Some(0),
            // Do nothing if no keys are pressed
            Some(index) => Some(index),
            None => None,
        }
    }
}

// impl SerializableAny for AutoCompleteTextEditState {

// }
