use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontId};
use syntect::dumps::from_uncompressed_data;
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet, SyntaxSetBuilder};
use syntect::util::LinesWithEndings;
use std::path::Path;
use super::ColorTheme;

pub trait Editor {
    fn syntax(&self) -> &str;
    fn theme(&self) -> &ColorTheme;
    fn font_id(&self) -> FontId;
    fn append(&self, job: &mut LayoutJob, text: &str, scope: &str) {
        let theme = self.theme();
        let font_id = self.font_id();
        let mut text_format = TextFormat::simple(font_id.clone(), theme.fg());
        if let Some(color) = theme.color_for_scope(scope) {
            text_format.color = color;
        }
        if let Some(bg_color) = theme.bg_for_scope(scope) {
            text_format.background = bg_color;
        }
        job.append(text, 0.0, text_format);
    }
}

pub struct Token {
    syntax_set: SyntaxSet,
}

impl Default for Token {
    fn default() -> Self {
        let mut builder = SyntaxSetBuilder::new();
        let theme = from_uncompressed_data(include_bytes!("Powershell/Powershell.sublime-syntax"));
        builder.add(theme.unwrap());
        builder.add_plain_text_syntax();
        let syntax_set = builder.build();
        Token { syntax_set }
    }
}

impl Token {

    pub fn highlight<T: Editor>(&self, editor: &T, text: &str) -> LayoutJob {
        let mut job = LayoutJob::default();
        log::info!("Syntax: {:?}", self.syntax_set);
        let syntax = self
            .syntax_set
            .find_syntax_by_extension("ps1")
            .unwrap_or_else(|| self.syntax_set.find_syntax_by_name("PowerShell").unwrap_or_else(|| self.syntax_set.find_syntax_plain_text()));

        let mut parse_state = ParseState::new(syntax);
        let mut scope_stack = ScopeStack::new();

        for line in LinesWithEndings::from(text) {
            let ops = parse_state
                .parse_line(line, &self.syntax_set)
                .unwrap_or_else(|e| panic!("Failed to parse line: {}", e));

            // Process the line to extract text regions based on byte indices
            let mut last_index = 0;
            for (i, (start_index, scope_stack_op)) in ops.iter().enumerate() {
                // Ensure start_index is within bounds
                if *start_index < last_index || *start_index > line.len() {
                    continue; // Skip invalid indices
                }

                // Extract the text region from last_index to start_index
                let region_text = &line[last_index..*start_index];
                if !region_text.is_empty() {
                    // Use the current scope stack to determine the style
                    let scope = scope_stack
                        .as_slice()
                        .iter()
                        .map(|scope| scope.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    editor.append(&mut job, region_text, &scope);
                }

                // Apply the scope operation
                scope_stack
                    .apply(scope_stack_op)
                    .unwrap_or_else(|e| panic!("Failed to apply scope operation: {}", e));

                // Update last_index for the next iteration
                last_index = *start_index;

                // If this is the last operation, handle the remaining text
                if i == ops.len() - 1 && last_index < line.len() {
                    let region_text = &line[last_index..];
                    if !region_text.is_empty() {
                        let scope = scope_stack
                            .as_slice()
                            .iter()
                            .map(|scope| scope.to_string())
                            .collect::<Vec<_>>()
                            .join(" ");
                        editor.append(&mut job, region_text, &scope);
                    }
                }
            }
        }

        job
    }
}

pub type HighlightCache = eframe::egui::util::cache::FrameCache<LayoutJob, Token>;

pub fn highlight<T: Editor + std::hash::Hash>(ctx: &eframe::egui::Context, editor: &T, text: &str) -> LayoutJob {
    ctx.memory_mut(|mem| {
        mem.caches
            .cache::<HighlightCache>()
            .get((editor, text))
    })
}

impl<T: Editor> eframe::egui::util::cache::ComputerMut<(&T, &str), LayoutJob> for Token {
    fn compute(&mut self, (editor, text): (&T, &str)) -> LayoutJob {
        self.highlight(editor, text)
    }
}
