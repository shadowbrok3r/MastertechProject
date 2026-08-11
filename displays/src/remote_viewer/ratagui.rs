//! This module provides the `RataguiBackend` implementation for the [`Backend`] trait.
use eframe::egui::{epaint::{text::{LayoutJob, TextFormat, TextWrapping},Color32, FontFamily, FontId, Fonts}, Align, Key, Modifiers, Response, Stroke, Ui, Widget};
use ratatui::{backend::{Backend, ClearType, WindowSize}, buffer::{Buffer, Cell}, layout::{Position, Rect, Size}, style::{Color, Modifier}};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use web_time::Instant;
// crossterm::event::{KeyCode, KeyModifiers}
use super::input_focus::RemoteViewFocus;
use super::{terminal_line::paint_terminal_line, SerializableBuffer};

#[derive(Debug, Clone, PartialEq, Hash)]
struct InstantWrapper(Instant);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerminalEvent {
    MouseClick { x: u16, y: u16 },
    KeyPress { code: Key, modifiers: Modifiers },
    MouseMove { x: u16, y: u16 },
    MouseScroll { x: u16, y: u16, up: bool },
    // KeyPress { code: KeyCode, modifiers: KeyModifiers }
}

// New struct to wrap buffer with timestamp
#[derive(Debug, Serialize, Deserialize)]
pub struct BufferMessage {
    pub frame_count: u64,
    pub timestamp: u128, // Milliseconds since epoch or elapsed time
    pub buffer: SerializableBuffer, // Encoded buffer data
    pub encode_duration: u64,
}

impl Default for InstantWrapper {
    fn default() -> Self {
        Self(Instant::now())
    }
}

///The RataguiBackend is the widget+backend itself , from which you can make a ratatui terminal ,
/// then you can do ui.add(terminal.backend_mut()) inside an egui context    .
/// Spawn with RataguiBackend::new() or RataguiBackend::new_with_fonts()   .
#[derive(Debug, Clone, serde::Serialize)]
pub struct RataguiBackend {
    width: u16,
    buffer: Buffer,
    height: u16,
    cursor: bool,
    font_size: u16,
    pos: Position,
    regular_font: FontId,
    bold_font: FontId,
    italic_font: FontId,
    bolditalic_font: FontId,
    #[serde(skip)]
    timestamp: InstantWrapper,
    blinking_slow: bool,
    blinking_fast: bool,
    cached_job: Option<Vec<LayoutJob>>,
    // cached_job: Option<LayoutJob>, // Cache the rendered buffer
    frame_index: u64,              // New: Track frame index
    buffer_changed: bool, // New: Track buffer content changes
    #[serde(skip)]
    hover_events: bool,
    #[serde(skip)]
    scroll_accum: f32,
    #[serde(skip)]
    event_tx: Sender<TerminalEvent>,
    /// Gates keyboard forwarding and swallows the keys the host must not act on.
    #[serde(skip)]
    focus: RemoteViewFocus,
    #[serde(skip)]
    input_capture: bool,
}

impl Widget for &mut RataguiBackend {
    fn ui(self, ui: &mut Ui) -> Response {
        let elpsd = self.timestamp.0.elapsed().as_millis();

        if elpsd > 1200 {
            self.timestamp.0 = Instant::now();
            self.blinking_fast = false;
            self.blinking_slow = false;
        } else if elpsd > 1000 {
            self.blinking_fast = true;
        } else if elpsd > 800 {
            self.blinking_slow = true;
            self.blinking_fast = false;
        } else if elpsd > 600 {
            self.blinking_fast = true;
        } else if elpsd > 400 {
            self.blinking_fast = false;
        } else if elpsd > 200 {
            self.blinking_fast = true;
        }

        let char_width = ui.fonts_mut(|f| f.glyph_width(&self.regular_font, ' '));
        let char_height = ui.fonts_mut(|f| f.row_height(&self.regular_font));
        let available_size = ui.available_size();
        let available_chars_width = (available_size.x / char_width).max(1.0) as u16;
        let available_chars_height = (available_size.y / char_height).max(1.0) as u16;

        let cur_size = self.size().expect("Could not get current backend size");
        let needs_resize = cur_size.width != available_chars_width || cur_size.height != available_chars_height;
        if needs_resize {
            self.resize(available_chars_width, available_chars_height);
            self.buffer_changed = true;
        }

        let needs_rebuild = self.buffer_changed || self.cached_job.is_none();
        if needs_rebuild {
            self.cached_job = Some((0..available_chars_height)
                .map(|y| self.build_row_job(ui, y, available_chars_width))
                .collect::<Vec<_>>());
            self.buffer_changed = false;
            log::debug!("Rebuilt LayoutJob: frame_index={}", self.frame_index);
        }

        let jobs = self
            .cached_job
            .as_ref()
            .expect("Cached job should be initialized")
            .clone();

        // One rect over the whole grid, so a click on a blank cell lands and the cell math has a
        // single stable origin.
        let grid_size = eframe::egui::vec2(
            available_chars_width as f32 * char_width,
            available_chars_height as f32 * char_height,
        );
        let grid_rect = eframe::egui::Rect::from_min_size(ui.cursor().min, grid_size);
        // Click, not click_and_drag: drag sensing defers `clicked()` until egui can rule out a drag,
        // which reads as lag on a TUI where every click is a menu selection.
        let sense = if self.input_capture {
            eframe::egui::Sense::click()
        } else {
            eframe::egui::Sense::hover()
        };
        let response = ui.allocate_rect(grid_rect, sense);

        for (row, job) in jobs.into_iter().enumerate() {
            let top_left = grid_rect.min + eframe::egui::vec2(0.0, row as f32 * char_height);
            paint_terminal_line(ui, top_left, job);
        }

        if !self.input_capture {
            if needs_rebuild || needs_resize {
                ui.ctx().request_repaint();
            }
            return response;
        }

        let focused = self.focus.update(&response);
        let cell_at = |pos: eframe::egui::Pos2| -> (u16, u16) {
            let rel = pos - grid_rect.min;
            let x = (rel.x / char_width).floor().clamp(0.0, available_chars_width as f32 - 1.0);
            let y = (rel.y / char_height).floor().clamp(0.0, available_chars_height as f32 - 1.0);
            (x as u16, y as u16)
        };

        if (response.clicked() || response.secondary_clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = cell_at(pos);
            if self.event_tx.send(TerminalEvent::MouseClick { x, y }).is_ok() {
                log::debug!("Sent mouse click event: x={x}, y={y}");
            } else {
                log::warn!("Failed to send mouse event");
            }
        }

        if self.hover_events && let Some(pos) = response.hover_pos() {
            let (x, y) = cell_at(pos);
            let _ = self.event_tx.send(TerminalEvent::MouseMove { x, y });
        }

        // One scroll event per accumulated wheel notch under the pointer.
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_y != 0.0
            && response.contains_pointer()
            && let Some(pos) = response.hover_pos()
        {
            const SCROLL_STEP: f32 = 50.0;
            self.scroll_accum += scroll_y;
            let (x, y) = cell_at(pos);
            while self.scroll_accum.abs() >= SCROLL_STEP {
                let up = self.scroll_accum > 0.0;
                self.scroll_accum -= if up { SCROLL_STEP } else { -SCROLL_STEP };
                let _ = self.event_tx.send(TerminalEvent::MouseScroll { x, y, up });
            }
        }

        // Keys only while the grid owns focus, then swallowed so the host UI never sees them.
        if focused {
            let keys: Vec<TerminalEvent> = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        eframe::egui::Event::Key { key, pressed: true, modifiers, .. } => {
                            Some(TerminalEvent::KeyPress { code: *key, modifiers: *modifiers })
                        }
                        _ => None,
                    })
                    .collect()
            });
            for event in keys {
                log::debug!("Sent key press event: {event:?}");
                if let Err(e) = self.event_tx.send(event) {
                    log::warn!("Failed to send key event: {e:?}");
                }
            }
            self.focus.swallow_keys(ui);
        }

        if needs_rebuild || needs_resize {
            ui.ctx().request_repaint();
        }
        response
    }
}

pub use crate::ui_tools::terminal_font::{terminal_font, TERMINAL_FONT_FAMILY, TERMINAL_FONT_SIZE};

impl RataguiBackend {
    /// Creates a new `RataguiBackend` with the specified width and height.
    pub fn new(width: u16, height: u16, event_tx: Sender<TerminalEvent>) -> Self {
        let font = terminal_font(TERMINAL_FONT_SIZE as f32);
        Self {
            width,
            height,
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            cursor: false,
            // Must match the FontIds below or `fit_font_to_grid` scales by the wrong ratio.
            font_size: TERMINAL_FONT_SIZE,
            pos: (0, 0).into(),
            regular_font: font.clone(),
            bold_font: font.clone(),
            italic_font: font.clone(),
            bolditalic_font: font,
            timestamp: Default::default(),
            blinking_slow: false,
            blinking_fast: false,
            cached_job: None,
            frame_index: 0,
            buffer_changed: false,
            hover_events: false,
            scroll_accum: 0.0,
            event_tx,
            focus: RemoteViewFocus::new(),
            input_capture: true,
        }
    }

    pub fn new_with_fonts(
        width: u16,
        height: u16,
        event_tx: Sender<TerminalEvent>,
        regular: String,
        bold: String,
        italic: String,
        bolditalic: String,
    ) -> Self {
        Self {
            width,
            height,
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            cursor: false,
            font_size: 16,
            pos: (0, 0).into(),
            regular_font: FontId::new(16.0, FontFamily::Name(regular.to_owned().into())),
            bold_font: FontId::new(16.0, FontFamily::Name(bold.to_owned().into())),
            italic_font: FontId::new(16.0, FontFamily::Name(italic.to_owned().into())),
            bolditalic_font: FontId::new(16.0, FontFamily::Name(bolditalic.to_owned().into())),
            timestamp: Default::default(),
            blinking_slow: false,
            blinking_fast: false,
            cached_job: None,
            frame_index: 0,
            buffer_changed: false,
            hover_events: false,
            scroll_accum: 0.0,
            event_tx,
            focus: RemoteViewFocus::new(),
            input_capture: true,
        }
    }

    pub fn set_frame_index(&mut self, index: u64) {
        self.frame_index = index; // Don’t invalidate cache here
    }

    /// Character grid the last `ui()` laid out, in cells.
    ///
    /// This is the only grid that exists on screen — a caller that wants to request a frame size
    /// should ask for this, not convert points to cells itself.
    pub fn grid(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Emit `TerminalEvent::MouseMove` while the pointer hovers a row. Off by
    /// default so the remote viewer's event channel stays click-only.
    pub fn set_hover_events(&mut self, enabled: bool) {
        self.hover_events = enabled;
    }

    /// Emit input events and hold keyboard focus over the grid. On by default.
    ///
    /// Turn it off for a caller that runs its own focus id and reads `egui` input directly —
    /// otherwise the grid takes focus out from under it on the first click and its keyboard dies.
    pub fn set_input_capture(&mut self, enabled: bool) {
        self.input_capture = enabled;
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    pub fn get_font_size(&self) -> u16 {
        self.font_size.clone()
    }
    
    pub fn set_font_size(&mut self, desired: u16) {
        self.font_size = desired;

        self.regular_font = FontId::new(desired as f32, self.regular_font.family.to_owned());
        self.bold_font = FontId::new(desired as f32, self.bold_font.family.to_owned());
        self.italic_font = FontId::new(desired as f32, self.italic_font.family.to_owned());
        self.bolditalic_font = FontId::new(desired as f32, self.bolditalic_font.family.to_owned());
        // Cached rows carry the old font metrics.
        self.cached_job = None;
        self.buffer_changed = true;
    }

    /// Largest font size (6..=24) whose glyph grid still fits `cols`×`rows`
    /// inside the currently available space.
    pub fn fit_font_to_grid(&mut self, ui: &mut Ui, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        let avail = ui.available_size();
        if avail.x <= 0.0 || avail.y <= 0.0 {
            return;
        }
        let cur = self.font_size.max(1) as f32;
        let (cw, ch) = ui.fonts_mut(|f| {
            (f.glyph_width(&self.regular_font, ' '), f.row_height(&self.regular_font))
        });
        if cw <= 0.0 || ch <= 0.0 {
            return;
        }
        // Per-point glyph metrics; egui font metrics scale linearly with size.
        let by_w = avail.x / (cols as f32 * (cw / cur));
        let by_h = avail.y / (rows as f32 * (ch / cur));
        let fitted = ((by_w.min(by_h) * 0.98).floor() as u16).clamp(6, 24);
        if fitted != self.font_size {
            self.set_font_size(fitted);
        }
    }

    /// Returns a reference to the internal buffer of the `RataguiBackend`.
    pub const fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Resizes the `RataguiBackend` to the specified width and height.
    pub fn resize(&mut self, width: u16, height: u16) {
        if self.width != width || self.height != height {
            self.buffer.resize(Rect::new(0, 0, width, height));
            self.width = width;
            self.height = height;
            self.buffer_changed = true;
            log::info!("Backend resized to: width={}, height={}", width, height);
        }
    }

    pub fn update_buffer(&mut self, new_buffer: Buffer) { // New: Explicit buffer update method
        if self.content_changed(&new_buffer) {
            self.buffer = new_buffer;
            self.buffer_changed = true;
            log::info!("Backend buffer updated: frame_index={}", self.frame_index);
        }
    }

    // Changed: Add content comparison to reduce redundant updates
    pub fn content_changed(&self, new_buffer: &Buffer) -> bool {
        if self.buffer.area != new_buffer.area {
            return true;
        }
        self.buffer.content != new_buffer.content
    }
    
    pub fn get_font_width(&self, fonts: &mut Fonts) -> f32 {
        let fid = self.regular_font.clone();
        // let width = fonts.glyph_width(&fid, ' ');
        let mut view = fonts.with_pixels_per_point(1.0);

        let width = view.glyph_width(&fid, ' ');
        // log::info!("widik is {:#?}",width);
        width
    }

    pub fn rat_to_egui_color(rat_col: &ratatui::style::Color, is_a_fg: bool) -> Color32 {
        match rat_col {
            Color::Reset => {
                if is_a_fg {
                    Color32::from_rgb(204, 204, 255)
                } else {
                    Color32::from_rgb(10,10,14)
                }
            }
            Color::Black => Color32::BLACK,
            Color::Red => Color32::DARK_RED,
            Color::Green => Color32::DARK_GREEN,
            Color::Yellow => Color32::GOLD,
            Color::Blue => Color32::DARK_BLUE,
            Color::Magenta => Color32::from_rgb(99, 9, 99),
            Color::Cyan => Color32::BLUE,
            Color::Gray => Color32::GRAY,
            Color::DarkGray => Color32::DARK_GRAY,
            Color::LightRed => Color32::LIGHT_RED,
            Color::LightGreen => Color32::GREEN,
            Color::LightBlue => Color32::LIGHT_BLUE,
            Color::LightYellow => Color32::LIGHT_YELLOW,
            Color::LightMagenta => Color32::from_rgb(139, 0, 139),
            Color::LightCyan => Color32::from_rgb(224, 255, 255),
            Color::White => Color32::WHITE,
            Color::Indexed(i) => Color32::from_rgb(
                i.overflowing_mul(i.to_owned()).0,
                i.overflowing_add(i.to_owned()).0,
                i.to_owned(),
            ),
            Color::Rgb(r, g, b) => Color32::from_rgb(r.to_owned(), g.to_owned(), b.to_owned()),
        }
    }


    fn build_row_job(&self, ui: &Ui, y: u16, width: u16) -> LayoutJob {
        let char_height = ui.fonts_mut(|f| f.row_height(&self.regular_font));
        let singular_wrapping = TextWrapping {
            max_width: f32::INFINITY,
            max_rows: 1,
            break_anywhere: false,
            overflow_character: None,
        };

        let mut row_job = LayoutJob {
            text: String::with_capacity(width as usize), // Pre-allocate
            sections: Vec::with_capacity(width as usize), // Pre-allocate
            wrap: singular_wrapping,
            first_row_min_height: char_height,
            break_on_newline: false,
            halign: Align::LEFT,
            justify: false,
            round_output_to_gui: false,
            keep_trailing_whitespace: false,
        };

        let cur_buf = &self.buffer;
        for x in 0..width {
            if let Some(cur_cell) = cur_buf.cell(Position { x, y }) {
                let is_bold = cur_cell.modifier.contains(Modifier::BOLD);
                let is_italic = cur_cell.modifier.contains(Modifier::ITALIC);
                let is_underlined = cur_cell.modifier.contains(Modifier::UNDERLINED);
                let is_slowblink = cur_cell.modifier.contains(Modifier::SLOW_BLINK);
                let is_rapidblink = cur_cell.modifier.contains(Modifier::RAPID_BLINK);
                let is_reversed = cur_cell.modifier.contains(Modifier::REVERSED);
                let is_dim = cur_cell.modifier.contains(Modifier::DIM);
                let is_hidden = cur_cell.modifier.contains(Modifier::HIDDEN);
                let is_crossed_out = cur_cell.modifier.contains(Modifier::CROSSED_OUT);
                let tf_font = if is_bold && is_italic {
                    self.bolditalic_font.clone()
                } else if is_bold {
                    self.bold_font.clone()
                } else if is_italic {
                    self.italic_font.clone()
                } else {
                    self.regular_font.clone()
                };
                let mut tf_fg_color = RataguiBackend::rat_to_egui_color(&cur_cell.fg, true);
                let mut tf_bg_color = RataguiBackend::rat_to_egui_color(&cur_cell.bg, false);
                if is_slowblink && self.blinking_slow {
                    tf_fg_color = tf_bg_color.clone();
                }
                if is_rapidblink && self.blinking_fast {
                    tf_fg_color = tf_bg_color.clone();
                }
                if is_dim {
                    tf_fg_color = tf_fg_color.gamma_multiply(0.7);
                    tf_bg_color = tf_bg_color.gamma_multiply(0.7);
                }
                if is_reversed {
                    std::mem::swap(&mut tf_fg_color, &mut tf_bg_color);
                }
                if is_hidden {
                    tf_fg_color = tf_bg_color.clone();
                }
                let tf_stroke = if is_crossed_out {
                    Stroke::new(char_height / 8.0, tf_fg_color)
                } else {
                    Stroke::NONE
                };
                let tf_underline = if is_underlined {
                    Stroke::new(char_height / 3.0, tf_fg_color)
                } else {
                    Stroke::NONE
                };
                let tf = TextFormat {
                    font_id: tf_font,
                    color: tf_fg_color,
                    background: tf_bg_color,
                    strikethrough: tf_stroke,
                    underline: tf_underline,
                    ..Default::default()
                };
                let symbol = cur_cell.symbol();
                if !symbol.is_empty() { // Skip empty cells
                    row_job.append(symbol, 0.0, tf);
                }
                // row_job.append(cur_cell.symbol(), 0.0, tf);
            }
        }
        row_job
    }
}

impl Backend for RataguiBackend {
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut changed = false;
        for (x, y, c) in content {
            if let Some(cell) = self.buffer.cell_mut(Position { x, y }) {
                if cell != c {
                    *cell = c.clone();
                    changed = true;
                }
            }
        }
        if changed {
            self.buffer_changed = true;
            self.cached_job = None;
            log::info!(
                "Terminal buffer updated: frame_index={}, area={:?}",
                self.frame_index,
                self.buffer.area
            );
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        self.cursor = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        self.cursor = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<Position> {
        Ok(self.pos.into())
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> std::io::Result<()> {
        self.pos = position.into();
        Ok(())
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.buffer.reset();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> std::io::Result<()> {
        match clear_type {
            ClearType::All => self.clear()?,
            ClearType::AfterCursor => {
                let index = self.buffer.index_of(self.pos.x, self.pos.y) + 1;
                self.buffer.content[index..].fill(Cell::default());
            }
            ClearType::BeforeCursor => {
                let index = self.buffer.index_of(self.pos.x, self.pos.y);
                self.buffer.content[..index].fill(Cell::default());
            }
            ClearType::CurrentLine => {
                let line_start_index = self.buffer.index_of(0, self.pos.y);
                let line_end_index = self.buffer.index_of(self.width - 1, self.pos.y);
                self.buffer.content[line_start_index..=line_end_index].fill(Cell::default());
            }
            ClearType::UntilNewLine => {
                let index = self.buffer.index_of(self.pos.x, self.pos.y);
                let line_end_index = self.buffer.index_of(self.width - 1, self.pos.y);
                self.buffer.content[index..=line_end_index].fill(Cell::default());
            }
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> std::io::Result<()> {
        let (cur_x, cur_y) = self.get_cursor_position()?.into();

        // the next column ensuring that we don't go past the last column
        let new_cursor_x = cur_x.saturating_add(1).min(self.width.saturating_sub(1));

        let max_y = self.height.saturating_sub(1);
        let lines_after_cursor = max_y.saturating_sub(cur_y);
        if n > lines_after_cursor {
            let rotate_by = n.saturating_sub(lines_after_cursor).min(max_y);

            if rotate_by == self.height - 1 {
                self.clear()?;
            }

            self.set_cursor_position((0, rotate_by))?;
            self.clear_region(ClearType::BeforeCursor)?;
            self.buffer
                .content
                .rotate_left((self.width * rotate_by).into());
        }

        let new_cursor_y = cur_y.saturating_add(n).min(max_y);
        self.set_cursor_position((new_cursor_x, new_cursor_y))?;

        Ok(())
    }

    fn size(&self) -> std::io::Result<Size> {
        Ok(Size::new(self.width, self.height))
    }

    fn window_size(&mut self) -> std::io::Result<WindowSize> {
        // Some arbitrary window pixel size, probably doesn't need much testing.
        static WINDOW_PIXEL_SIZE: Size = Size {
            width: 640,
            height: 480,
        };
        Ok(WindowSize {
            columns_rows: (self.width, self.height).into(),
            pixels: WINDOW_PIXEL_SIZE,
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}