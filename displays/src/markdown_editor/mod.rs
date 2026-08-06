// use crate::ui_tools::autocomplete::AutoCompleteTextEdit;
use core::f32;
use eframe::egui::{text::CCursorRange, *};
use std::collections::BTreeSet;
pub mod chat_markdown;
pub mod highlighter;
pub mod parser;
pub mod viewer;

#[derive(Debug, Clone)]
pub struct EasyMarkEditor {
    message: String,
    highlight_editor: bool,
    show_rendered: bool,
    pub inputs: BTreeSet<String>,
    pub highlighter: highlighter::MemoizedEasymarkHighlighter,
    pub private_note: bool,
    /// When false, the "Private Note" toggle is hidden and notes are always
    /// non-private (used by Task Audit, where notes are Prestashop-only).
    pub allow_private: bool,
}

impl PartialEq for EasyMarkEditor {
    fn eq(&self, other: &Self) -> bool {
        (&self.message, self.highlight_editor, self.show_rendered)
            == (&other.message, other.highlight_editor, other.show_rendered)
    }
}

impl Default for EasyMarkEditor {
    fn default() -> Self {
        Self {
            message: String::new(),
            highlight_editor: true,
            show_rendered: false,
            highlighter: Default::default(),
            inputs: BTreeSet::new(),
            private_note: false,
            allow_private: true,
        }
    }
}

impl EasyMarkEditor {
    pub fn new() -> Self {
        Self {
            message: String::new(),
            highlight_editor: true,
            show_rendered: false,
            highlighter: Default::default(),
            inputs: BTreeSet::new(),
            private_note: false,
            allow_private: true,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<Response> {
        let mut response: Option<Response> = None;
        let mut font = FontId::default();
        font.size = 12.0;
        ui.style_mut().override_font_id = Some(font);
        if self.show_rendered {
            ui.columns(2, |columns| {
                ScrollArea::vertical()
                    .max_height(f32::INFINITY)
                    .id_salt("source")
                    .show(&mut columns[0], |ui| self.editor_ui(ui));
                ScrollArea::vertical()
                    .id_salt("rendered")
                    .max_height(f32::INFINITY)
                    .show(&mut columns[1], |ui| {
                        // TODO(emilk): we can save some more CPU by caching the rendered output.
                        self::viewer::easy_mark(ui, &self.message);
                    });
            });
        } else {
            ScrollArea::vertical()
                .id_salt("source")
                .show(ui, |ui| self.editor_ui(ui));
        }
        ui.separator();

        ui.columns(5, |columns| {
            let _res = columns[0].add(Button::new("Hotkeys").min_size(Vec2::new(60.0, 10.0))).on_hover_ui(nested_hotkeys_ui);
            columns[1].checkbox(&mut self.show_rendered, "Show rendered");
            if self.allow_private {
                columns[3].checkbox(&mut self.private_note, "Private Note");
            } else {
                self.private_note = false;
            }
            let res = columns[4].add(Button::new("Submit").min_size(Vec2::new(60.0, 10.0)));
            response = Some(res);
        });

        response
    }

    fn editor_ui(&mut self, ui: &mut Ui) {
        let Self {
            message,
            highlighter,
            inputs: _inputs,
            ..
        } = self;

        let response = if self.highlight_editor {

            let mut layouter = |ui: &Ui, buf: &dyn eframe::egui::TextBuffer, wrap_width: f32| {
                let mut layout_job = highlighter.highlight(ui.style(), buf.as_str());
                layout_job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(layout_job))
            };

            TextEdit::multiline(message)
                .margin(Margin::symmetric(6, 3))
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .ui(ui)

            // AutoCompleteTextEdit::new(message, &inputs.to_owned())
            //     .set_filter(|s| s.contains('@'))
            //     .highlight_matches(true)
            //     .max_suggestions(10)
            //     .layouter(&mut layouter)
            //     .set_text_edit_properties(|text_edit: TextEdit<'_>| {
            //         text_edit
            //             .desired_width(f32::INFINITY)
            //             .font(FontId::monospace(12.0))
            //             .frame(true)
            //             .desired_rows(10)
            //             .code_editor()
            //     })
            //     .ui(ui)
        } else {
            TextEdit::multiline(message)
                .margin(Margin::symmetric(6, 3))
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .ui(ui)
        };

        if let Some(mut state) = TextEdit::load_state(ui.ctx(), response.id) {
            // info!("Text edit load state");
            if let Some(mut ccursor_range) = state.cursor.char_range() {
                // info!("if let state.cursor.char_range()");
                let any_change = shortcuts(ui, message, &mut ccursor_range);
                if any_change {
                    // info!(" if any_change ");
                    state.cursor.set_char_range(Some(ccursor_range));
                    state.store(ui.ctx(), response.id);
                }
            }
        }
    }

    pub fn submit(&self) -> String {
        self.message.clone()
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }
}

pub const SHORTCUT_BOLD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
pub const SHORTCUT_CODE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
pub const SHORTCUT_ITALICS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
pub const SHORTCUT_SUBSCRIPT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::L);
pub const SHORTCUT_SUPERSCRIPT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
pub const SHORTCUT_STRIKETHROUGH: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::CTRL.plus(Modifiers::SHIFT), Key::Q);
pub const SHORTCUT_UNDERLINE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::CTRL.plus(Modifiers::SHIFT), Key::W);
pub const SHORTCUT_INDENT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::CTRL.plus(Modifiers::SHIFT), Key::E);
pub const SHORTCUT_ENTER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter);

fn nested_hotkeys_ui(ui: &mut Ui) {
    Grid::new("shortcuts").striped(true).show(ui, |ui| {
        let mut label = |shortcut, what| {
            ui.label(what);
            ui.weak(ui.ctx().format_shortcut(&shortcut));
            ui.end_row();
        };

        label(SHORTCUT_BOLD, "*bold*");
        label(SHORTCUT_CODE, "`message`");
        label(SHORTCUT_ITALICS, "/italics/");
        label(SHORTCUT_SUBSCRIPT, "$subscript$");
        label(SHORTCUT_SUPERSCRIPT, "^superscript^");
        label(SHORTCUT_STRIKETHROUGH, "~strikethrough~");
        label(SHORTCUT_UNDERLINE, "_underline_");
        label(SHORTCUT_INDENT, "two spaces"); // Placeholder for tab indent
    });
}

pub fn shortcuts(ui: &Ui, message: &mut dyn TextBuffer, ccursor_range: &mut CCursorRange) -> bool {
    let mut any_change = false;
    // info!("In shortcuts fn");
    if ui.input_mut(|i| i.consume_shortcut(&SHORTCUT_INDENT)) {
        // This is a placeholder till we can indent the active line
        any_change = true;
        // info!("In input mut");
        let [primary, _secondary] = ccursor_range.sorted_cursors();

        let advance = message.insert_text("  ", primary.index);
        ccursor_range.primary.index += advance;
        ccursor_range.secondary.index += advance;
    }

    for (shortcut, surrounding) in [
        (SHORTCUT_BOLD, "*"),
        (SHORTCUT_CODE, "`"),
        (SHORTCUT_ITALICS, "/"),
        // (SHORTCUT_SUBSCRIPT, "$"),
        (SHORTCUT_SUPERSCRIPT, "^"),
        (SHORTCUT_STRIKETHROUGH, "~"),
        (SHORTCUT_UNDERLINE, "_"),
    ] {
        if ui.input_mut(|i| i.consume_shortcut(&shortcut)) {
            any_change = true;
            toggle_surrounding(message, ccursor_range, surrounding);
        };
    }

    any_change
}

/// E.g. toggle *strong* with `toggle_surrounding(&mut text, &mut cursor, "*")`
fn toggle_surrounding(
    message: &mut dyn TextBuffer,
    ccursor_range: &mut CCursorRange,
    surrounding: &str,
) {
    // info!("In toggle_surrounding");
    let [primary, secondary] = ccursor_range.sorted_cursors();

    let surrounding_ccount = surrounding.chars().count();

    let prefix_crange = primary.index.saturating_sub(surrounding_ccount)..primary.index;
    let suffix_crange = secondary.index..secondary.index.saturating_add(surrounding_ccount);
    let already_surrounded = message.char_range(prefix_crange.clone()) == surrounding
        && message.char_range(suffix_crange.clone()) == surrounding;

    if already_surrounded {
        // info!("already_surrounded");
        message.delete_char_range(suffix_crange);
        message.delete_char_range(prefix_crange);
        ccursor_range.primary.index -= surrounding_ccount;
        ccursor_range.secondary.index -= surrounding_ccount;
    } else {
        // info!("else");
        message.insert_text(surrounding, secondary.index);
        let advance = message.insert_text(surrounding, primary.index);

        ccursor_range.primary.index += advance;
        ccursor_range.secondary.index += advance;
    }
}

// ----------------------------------------------------------------------------

const _DEFAULT_CODE: &str = r#"
# EasyMark
EasyMark is a markup language, designed for extreme simplicity.

```
WARNING: EasyMark is still an evolving specification,
and is also missing some features.
```

----------------

# At a glance
- inline text:
  - normal, `message`, *strong*, ~strikethrough~, _underline_, /italics/, ^raised^, $small$
  - `\` escapes the next character
  - [hyperlink](https://github.com/emilk/egui)
  - Embedded URL: <https://github.com/emilk/egui>
- `# ` header
- `---` separator (horizontal line)
- `> ` quote
- `- ` bullet list
- `1. ` numbered list
- \`\`\` message fence
- a^2^ + b^2^ = c^2^
- $Remember to read the small print$

# Design
> /"Why do what everyone else is doing, when everyone else is already doing it?"
>   \- Emil

Goals:
1. easy to parse
2. easy to learn
3. similar to markdown

[The reference parser](https://github.com/emilk/egui/blob/master/crates/egui_demo_lib/src/easy_mark/easy_mark_parser.rs) is \~250 lines of message, using only the Rust standard library. The parser uses no look-ahead or recursion.

There is never more than one way to accomplish the same thing, and each special character is only used for one thing. For instance `*` is used for *strong* and `-` is used for bullet lists. There is no alternative way to specify the *strong* style or getting a bullet list.

Similarity to markdown is kept when possible, but with much less ambiguity and some improvements (like _underlining_).

# Details
All style changes are single characters, so it is `*strong*`, NOT `**strong**`. Style is reset by a matching character, or at the end of the line.

Style change characters and escapes (`\`) work everywhere except for in inline message, message blocks and in URLs.

You can mix styles. For instance: /italics _underline_/ and *strong `message`*.

You can use styles on URLs: ~my webpage is at <http://www.example.com>~.

Newlines are preserved. If you want to continue text on the same line, just do so. Alternatively, escape the newline by ending the line with a backslash (`\`). \
Escaping the newline effectively ignores it.

The style characters are chosen to be similar to what they are representing:
  `_` = _underline_
  `~` = ~strikethrough~ (`-` is used for bullet points)
  `/` = /italics/
  `*` = *strong*
  `$` = $small$
  `^` = ^raised^

# To do
- Sub-headers (`## h2`, `### h3` etc)
- Hotkey Editor
- International keyboard algorithm for non-letter keys
- ALT+SHIFT+Num1 is not a functioning hotkey
- Tab Indent Increment/Decrement CTRL+], CTRL+[

- Images
  - we want to be able to optionally specify size (width and\/or height)
  - centering of images is very desirable
  - captioning (image with a text underneath it)
  - `![caption=My image][width=200][center](url)` ?
- Nicer URL:s
  - `<url>` and `[url](url)` do the same thing yet look completely different.
  - let's keep similarity with images
- Tables
- Inspiration: <https://mycorrhiza.wiki/help/en/mycomarkup>
"#;
