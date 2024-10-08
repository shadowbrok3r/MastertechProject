use eframe::egui::{popup, Context, Id, Key, Modifiers, PopupCloseBehavior, TextBuffer, TextEdit, TextStyle, Ui, Widget};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use serde::{Deserialize, Serialize};
use std::{cmp::{min, Reverse}, collections::BTreeSet};

use crate::markdown_editor::highlighter::MemoizedEasymarkHighlighter;

use super::autocomplete::highlight_matches;

#[derive(Debug, Clone)]
pub struct MentionHandler {
    input_text: String,
    at_index: Option<usize>,
    available_mentions: BTreeSet<String>, // Set of all possible mentions
    selected_index: Option<usize>, // For tracking the selected suggestion
    highlighter: MemoizedEasymarkHighlighter
}

impl MentionHandler {
    pub fn new(available_mentions: BTreeSet<String>) -> Self {
        Self {
            input_text: String::new(),
            at_index: None,
            available_mentions,
            selected_index: None,
            highlighter: Default::default()
        }
    }
}

impl Default for MentionHandler {
    fn default() -> Self {
        Self { input_text: Default::default(), at_index: Default::default(), available_mentions: Default::default(), selected_index: Default::default(), highlighter: Default::default() }
    }
}

impl Widget for MentionHandler {
    fn ui(mut self, ui: &mut Ui) -> eframe::egui::Response {

        let mut layouter = |ui: &Ui, easymark: &str, wrap_width: f32| {
            let mut layout_job = self.highlighter.highlight(ui.style(), easymark);
            layout_job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(layout_job))
        };
    
        let text_response = ui.add(
            TextEdit::multiline(&mut self.input_text)
                .desired_width(f32::INFINITY).font(TextStyle::Monospace) 
                .layouter(&mut layouter)
        );
    
        let popup_id = Id::new("mention_popup");
        let mut state = MentionHandlerState::load(ui.ctx(), popup_id).unwrap_or_default();
        // only consume up/down presses if the text box is focused. This overwrites default behavior
        // to move to start/end of the string
        let up_pressed = state.focused
            && ui.input_mut(|input| input.consume_key(Modifiers::default(), Key::ArrowUp));
        let down_pressed = state.focused
            && ui.input_mut(|input| input.consume_key(Modifiers::default(), Key::ArrowDown));
            
        // Detect when '@' is typed and start the matching process
        if let Some('@') = self.input_text.chars().last() {
            self.at_index = Some(self.input_text.len() - 1);
        }
    
        // If we are in mention mode (after '@'), filter suggestions
        if let Some(at_index) = self.at_index {
            let search_term = &self.input_text[at_index + 1..];
            if search_term.contains(' ') || search_term.is_empty() {
                self.at_index = None; // Exit mention mode if a space is typed or the search term is empty
            } else {
                // Display the suggestions popup
                state.focused = text_response.has_focus();
    
                let matcher = SkimMatcherV2::default().ignore_case();
        
                let mut match_results = self.available_mentions.clone()
                    .into_iter()
                    .filter_map(|s| {
                        let score = matcher.fuzzy_indices(s.as_ref(), &self.input_text.as_str());
                        score.map(|(score, indices)| (s, score, indices))
                    })
                    .collect::<Vec<_>>();
                match_results.sort_by_key(|k| Reverse(k.1));
        
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
                    10,
                );
                
                let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter))
                    || ui.input_mut(|input| input.key_pressed(Key::Tab));
        
                if let (Some(index), true) = (
                    self.selected_index,
                    // If accepted by keyboard, close the popup. If the popup is closed with a selected index, take that text
                    accepted_by_keyboard || !ui.memory(|mem| mem.is_popup_open(popup_id)),
                ) {
                    self.input_text.replace_with(match_results[index].0.as_ref());
                    self.selected_index = None;
                }
                // Display the suggestions popup
                popup::popup_below_widget(ui, popup_id, &text_response, PopupCloseBehavior::IgnoreClicks, |ui| {
                    for (i, (output, _, match_indices)) in
                        match_results.iter().take(10).enumerate()
                    {
                        let mut selected = if let Some(x) = self.selected_index {
                            x == i
                        } else {
                            false
                        };
    
                        // let text = {
                        //     let mut job = LayoutJob::default();
                        //     job.append(output.as_ref(), 0.0, TextFormat::default());
                        //     job
                        // };
    
                        let text = highlight_matches(
                            output.as_ref(),
                            match_indices,
                            ui.style().visuals.widgets.active.text_color(),
                        );
                        // Update selected index based on hover
                        if ui.toggle_value(&mut selected, text.clone()).hovered() {
                            self.selected_index = Some(i);
                        }
    
                        // If a suggestion is clicked, replace the text
                        if ui.toggle_value(&mut selected, text).clicked() {
                            let full_mention = format!("@{}", output);
                            self.input_text.replace_range(at_index.., &full_mention);
                            self.at_index = None; // Exit mention mode after selection
                            break;
                        }
                    }
                });
    
                // Open the popup if necessary
                if !self.input_text.is_empty() && text_response.has_focus() {
                    ui.memory_mut(|mem| mem.open_popup(popup_id));
                } else {
                    ui.memory_mut(|mem| {
                        if mem.is_popup_open(popup_id) {
                            mem.close_popup()
                        }
                    });
                }
            }
        }
    
        // Handle normal text input
        if text_response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            // Process the message or send it
            println!("Final message: {}", self.input_text);
            self.input_text.clear(); // Clear the input after sending
        }

        state.store(ui.ctx(), popup_id);

        text_response
    }
}

/// Stores the currently selected index in egui state
#[derive(Clone, Default, Serialize, Deserialize)]
struct MentionHandlerState {
    /// Currently selected index, is `None` if nothing is selected
    selected_index: Option<usize>,
    /// Whether or not the text edit was focused last frame
    focused: bool,
}

impl MentionHandlerState {
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
