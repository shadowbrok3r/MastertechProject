use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH}};
use humansize::{format_size, BINARY};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, HighlightSpacing, List, ListState, StatefulWidgetRef, WidgetRef},
};
use rayon::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use super::{File, FileExplorer};

const BAR_LEN: usize = 16;                      // inner cells of the size bar
const BAR_CELL_WIDTH: usize = BAR_LEN + 4;      // " [" + BAR_LEN + "] "
const SIZE_COL_WIDTH: usize = 11;               // right‑aligned size string
const LOAD_FRAME_MS: u128 = 250;                // animation speed for loading bars
const PART: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

type LineFactory = Arc<dyn Fn(&FileExplorer) -> Line<'static> + Send + Sync>;

pub struct Renderer<'a>(pub(crate) &'a FileExplorer);

impl WidgetRef for Renderer<'_> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let explorer = self.0;
        let mut state = ListState::default().with_selected(Some(explorer.selected_idx()));

        // inner width = list area minus block borders (always 2)
        let inner_w = area.width.saturating_sub(2) as usize;
        let theme = explorer.theme();
        let parent_size = explorer.sizes.get_or_spawn(explorer.cwd());
        let items = explorer.files().iter().map(|f| line_for(f, theme, inner_w, &explorer.sizes, parent_size));

        let mut list = List::new(items)
            .style(theme.style)
            .highlight_spacing(theme.highlight_spacing.clone())
            .highlight_style(if explorer.current().is_dir() { theme.highlight_dir_style } else { theme.highlight_item_style })
            .scroll_padding(theme.scroll_padding);

        if let Some(sym) = theme.highlight_symbol.as_deref() { list = list.highlight_symbol(sym); }

        if let Some(block) = theme.block.as_ref() {
            let mut block = block.clone();
            for t in theme.title_top(explorer) { block = block.title_top(t); }
            for t in theme.title_bottom(explorer) { block = block.title_bottom(t); }
            list = list.block(block);
        }
        StatefulWidgetRef::render_ref(&list, area, buf, &mut state);
    }
}

#[derive(Clone, Default)]
pub struct SizeCache {
    map: Arc<std::sync::RwLock<std::collections::HashMap<std::path::PathBuf, u64>>>,
}

impl SizeCache {
    /// Non‑blocking query. Returns `None` while a directory walk is still running.
    fn get_or_spawn(&self, path: &std::path::Path) -> Option<u64> {
        {
            if let Ok(map) = self.map.read() {
                if let Some(sz) = map.get(path) {
                    return Some(*sz);
                }
            }
        }
        // Spawn once per path using write‑lock as a guard
        let need_spawn = {
            if let Ok(mut w) = self.map.write() {
                if !w.contains_key(path) {
                    w.insert(path.to_path_buf(), 0); // placeholder so we don't spawn again
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if need_spawn {
            let cache = self.clone();
            let path = path.to_path_buf();
            rayon::spawn(move || {
                let sz = dir_size_parallel(&path);
                if let Ok(mut map) = cache.map.write() {
                    map.insert(path, sz);
                }
            });
        }
        None
    }

    /// Optional eager preload (blocking)
    #[allow(dead_code)]
    fn preload(&self, root: &std::path::Path) {
        let sz = dir_size_parallel(root);
        if let Ok(mut map) = self.map.write() {
            map.insert(root.to_path_buf(), sz);
        }
    }
}

/// Recursively walk a directory summing file sizes, using Rayon parallelism.
fn dir_size_parallel(dir: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .par_bridge()
        .filter_map(Result::ok)
        .filter_map(|e| std::fs::symlink_metadata(e.path()).ok())
        .map(|m| if m.is_file() { m.len() } else { 0 })
        .sum()
}         // right‑aligned size string

fn fixed_width_size(opt: Option<u64>) -> String {
    let raw = opt.map_or("…".into(), |b| format_size(b, BINARY));
    let w = raw.width();
    if w >= SIZE_COL_WIDTH { raw } else { format!("{:>width$}", raw, width = SIZE_COL_WIDTH) }
}

// -----------------------------------------------------------------------------
// Helper: right‑align a human‑readable size string into SIZE_COL_WIDTH cells
// -----------------------------------------------------------------------------
fn truncate_to_width(s: &str, max_w: usize) -> String {
    if s.width() <= max_w { return s.into(); }
    let mut used = 0; let mut out = String::new();
    for ch in s.chars() {
        let w = ch.width().unwrap_or(1);
        if used + w > max_w - 1 { out.push('…'); break; }
        out.push(ch); used += w;
    }
    out
}

// -----------------------------------------------------------------------------
// Build one line (filename ··· bar ··· size) with perfect alignment
// -----------------------------------------------------------------------------
fn line_for<'a>(file: &'a File, theme: &Theme, inner_w: usize, cache: &SizeCache, dir_size: Option<u64>) -> Line<'a> {
    let style = if file.is_dir() { *theme.dir_style() } else { *theme.item_style() };

    // 1️⃣  Filename + icon -----------------------
    let mut name = format!("{} {}", if file.is_dir() { "🗀 " } else { "🗋 " }, file.name());

    // size lookup
    let size_opt = if file.is_dir() {
        cache.get_or_spawn(file.path())
    } else {
        std::fs::metadata(file.path()).ok().map(|m| m.len())
    };

    let size_str = fixed_width_size(size_opt);

    // choose bar kind
    let bar = match (size_opt, dir_size) {
        (Some(child), Some(parent)) if parent > 0 => size_bar(child, parent),
        _ => loading_bar(),
    };

    // layout calculation
    let rhs_w = BAR_CELL_WIDTH + SIZE_COL_WIDTH;
    let avail = inner_w.saturating_sub(rhs_w);
    if name.width() > avail { name = truncate_to_width(&name, avail); }
    let pad = avail.saturating_sub(name.width());

    Line::from(vec![
        Span::styled(name, style),
        Span::raw(" ".repeat(pad)),
        Span::raw(bar),
        Span::styled(size_str, style),
    ])
}

// -----------------------------------------------------------------------------
// 8‑cell bar with guaranteed width (8) — always fills exactly BAR_CELL_WIDTH
// -----------------------------------------------------------------------------
fn size_bar(child: u64, parent: u64) -> String {
    if parent == 0 { return loading_bar(); }
    const FULL: char = '█';
    let frac = (child as f64 / parent as f64).clamp(0.0, 1.0);
    let full_cells = (frac * BAR_LEN as f64).floor() as usize;
    let remainder_idx = ((frac * BAR_LEN as f64 - full_cells as f64) * 8.0).round() as usize;
    let rem = remainder_idx.min(7);

    let mut inner = String::with_capacity(BAR_LEN);
    for i in 0..BAR_LEN {
        if i < full_cells {
            inner.push(FULL);
        } else if i == full_cells && full_cells < BAR_LEN {
            inner.push(PART[rem]);
        } else {
            inner.push(' ');
        }
    }
    format!(" [{}] ", inner)
}
fn loading_bar() -> String {
    // animated sweep using PART glyph set across BAR_LEN
    let epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let frame = (epoch_ms / LOAD_FRAME_MS) as usize;
    let pos = frame % (BAR_LEN * PART.len());          // 0 .. BAR_LEN*8-1
    let cell = pos / PART.len();                       // which cell

    let mut inner = String::with_capacity(BAR_LEN);
    for i in 0..BAR_LEN {
        if i < cell {
            inner.push('▉');                           // already filled
        } else if i == cell {
            inner.push('.');               // partial progress
        } else {
            inner.push(' ');
        }
    }
    format!(" [{}] ", inner)
}

/// The theme of the file explorer.
///
/// This struct is used to customize the look of the file explorer.
/// It allows to set the style of the widget and the style of the files.
/// You can also wrap the widget in a block with the [`Theme::with_block`](#method.block)
/// method and add customizable titles to it with [`Theme::with_title_top`](#method.title_top)
/// and [`Theme::with_title_bottom`](#method.title_bottom).
#[derive(Clone)]
pub struct Theme {
    block: Option<Block<'static>>,
    title_top: Vec<LineFactory>,
    title_bottom: Vec<LineFactory>,
    style: Style,
    item_style: Style,
    dir_style: Style,
    highlight_spacing: HighlightSpacing,
    highlight_item_style: Style,
    highlight_dir_style: Style,
    highlight_symbol: Option<String>,
    scroll_padding: usize,
}

impl Theme {
    /// Create a new empty theme.
    ///
    /// The theme will not have any style set. To get a theme with the default style, use [`Theme::default`](#method.default).
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::new();
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            block: None,
            title_top: Vec::new(),
            title_bottom: Vec::new(),
            style: Style::new(),
            item_style: Style::new(),
            dir_style: Style::new(),
            highlight_spacing: HighlightSpacing::WhenSelected,
            highlight_item_style: Style::new(),
            highlight_dir_style: Style::new(),
            highlight_symbol: None,
            scroll_padding: 0,
        }
    }

    /// Add a top title to the theme.
    /// The title is the current working directory.
    ///
    /// # Example
    /// Suppose you have this tree file, with `passport.png` selected inside `file_explorer`:
    /// ```plaintext
    /// /
    /// ├── .git
    /// └── Documents
    ///     ├── passport.png  <- selected
    ///     └── resume.pdf
    /// ```
    /// You will end up with something like this:
    /// ```plaintext
    /// ┌/Documents────────────────────────┐
    /// │ ../                              │
    /// │ passport.png                     │
    /// │ resume.pdf                       │
    /// └──────────────────────────────────┘
    /// ```
    /// With this code:
    /// ```no_run
    /// use ratatui::widgets::*;
    /// use ratatui_explorer::{FileExplorer, Theme};
    ///
    /// let theme = Theme::default()
    ///     .with_block(Block::default().borders(Borders::ALL))
    ///     .add_default_title();
    ///
    /// let file_explorer = FileExplorer::with_theme(theme).unwrap();
    ///
    /// /* user select `password.png` */
    ///
    /// let widget = file_explorer.widget();
    /// /* render the widget */
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn add_default_title(self) -> Self {
        self.with_title_top(|file_explorer: &FileExplorer| {
            Line::from(file_explorer.cwd().display().to_string())
        })
    }

    /// Wrap the file explorer with a custom [`Block`](https://docs.rs/ratatui/latest/ratatui/widgets/block/struct.Block.html) widget.
    ///
    /// Behind the scene, it use the [`List::block`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.block) method. See its documentation for more.
    ///
    /// You can use [`Theme::with_title_top`](#method.title_top) and [`Theme::with_title_bottom`](#method.title_bottom)
    /// to add customizable titles to the block.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::widgets::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_block(Block::default().borders(Borders::ALL));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_block(mut self, block: Block<'static>) -> Self {
        self.block = Some(block);
        self
    }

    /// Set the style of the widget.
    ///
    /// Behind the scene, it use the [`List::style`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.style) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_style(Style::default().fg(Color::Yellow));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_style<S: Into<Style>>(mut self, style: S) -> Self {
        self.style = style.into();
        self
    }

    /// Set the style of all non directories items. To set the style of the directories, use [`Theme::with_dir_style`](#method.dir_style).
    ///
    /// Behind the scene, it use the [`Span::styled`](https://docs.rs/ratatui/latest/ratatui/text/struct.Span.html#method.styled) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_item_style(Style::default().fg(Color::White));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_item_style<S: Into<Style>>(mut self, item_style: S) -> Self {
        self.item_style = item_style.into();
        self
    }

    /// Set the style of all directories items. To set the style of the non directories, use [`Theme::with_item_style`](#method.item_style).
    ///
    /// Behind the scene, it use the [`Span::styled`](https://docs.rs/ratatui/latest/ratatui/text/struct.Span.html#method.styled) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_dir_style(Style::default().fg(Color::Blue));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_dir_style<S: Into<Style>>(mut self, dir_style: S) -> Self {
        self.dir_style = dir_style.into();
        self
    }

    /// Set the style of all highlighted non directories items. To set the style of the highlighted directories, use [`Theme::with_highlight_dir_style`](#method.highlight_dir_style).
    ///
    /// Behind the scene, it use the [`List::highlight_style`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.highlight_style) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_highlight_item_style(Style::default().add_modifier(Modifier::BOLD));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_highlight_item_style<S: Into<Style>>(mut self, highlight_item_style: S) -> Self {
        self.highlight_item_style = highlight_item_style.into();
        self
    }

    /// Set the style of all highlighted directories items. To set the style of the highlighted non directories, use [`Theme::with_highlight_item_style`](#method.highlight_item_style).
    ///
    /// Behind the scene, it use the [`List::highlight_style`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.highlight_style) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_highlight_dir_style(Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD));
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_highlight_dir_style<S: Into<Style>>(mut self, highlight_dir_style: S) -> Self {
        self.highlight_dir_style = highlight_dir_style.into();
        self
    }

    /// Set the symbol used to highlight the selected item.
    ///
    /// Behind the scene, it use the [List::highlight_symbol](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.highlight_symbol) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_highlight_symbol("> ");
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_highlight_symbol(mut self, highlight_symbol: &str) -> Self {
        self.highlight_symbol = Some(highlight_symbol.to_owned());
        self
    }

    /// Set the spacing between the highlighted item and the other items.
    ///
    /// Behind the scene, it use the [`List::highlight_spacing`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.highlight_spacing) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::widgets::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_highlight_spacing(HighlightSpacing::Never);
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_highlight_spacing(mut self, highlight_spacing: HighlightSpacing) -> Self {
        self.highlight_spacing = highlight_spacing;
        self
    }

    /// Sets the number of items around the currently selected item that should be kept visible.
    ///
    /// /// Behind the scene, it use the [List::scroll_padding](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html#method.scroll_padding) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::widgets::*;
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default().with_scroll_padding(1);
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_scroll_padding(mut self, scroll_padding: usize) -> Self {
        self.scroll_padding = scroll_padding;
        self
    }

    /// Add a top title factory to the theme.
    ///
    /// `title_top` is a function that take a reference to the current [`FileExplorer`] and returns
    /// a [`Line`](https://docs.rs/ratatui/latest/ratatui/text/struct.Line.html)
    /// to be displayed as a title at the top of the wrapping block (if it exist) of the file explorer. You can call
    /// this function multiple times to add multiple titles.
    ///
    /// Behind the scene, it use the [`Block::title_top`](https://docs.rs/ratatui/latest/ratatui/widgets/block/struct.Block.html#method.title_top) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// use ratatui::prelude::*;
    /// # use ratatui_explorer::{FileExplorer, Theme};
    /// let theme = Theme::default()
    ///     .with_title_top(|file_explorer: &FileExplorer| {
    ///         Line::from(format!("cwd - {}", file_explorer.cwd().display()))
    ///     })
    ///     .with_title_top(|file_explorer: &FileExplorer| {
    ///         Line::from(format!("{} files", file_explorer.files().len() - 1)).right_aligned()
    ///     });
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_title_top(
        mut self,
        title_top: impl Fn(&FileExplorer) -> Line<'static> + 'static + Send + Sync,
    ) -> Self {
        self.title_top.push(Arc::new(title_top));
        self
    }

    /// Add a bottom title factory to the theme.
    ///
    /// `title_bottom` is a function that take a reference to the current [`FileExplorer`] and returns
    /// a [`Line`](https://docs.rs/ratatui/latest/ratatui/text/struct.Line.html)
    /// to be displayed as a title at the bottom of the wrapping block (if it exist) of the file explorer. You can call
    /// this function multiple times to add multiple titles.
    ///
    /// Behind the scene, it use the [`Block::title_bottom`](https://docs.rs/ratatui/latest/ratatui/widgets/block/struct.Block.html#method.title_bottom) method. See its documentation for more.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui::prelude::*;
    /// # use ratatui_explorer::{FileExplorer, Theme};
    /// let theme = Theme::default()
    ///     .with_title_bottom(|file_explorer: &FileExplorer| {
    ///         Line::from(format!("cwd - {}", file_explorer.cwd().display()))
    ///     })
    ///     .with_title_bottom(|file_explorer: &FileExplorer| {
    ///         Line::from(format!("{} files", file_explorer.files().len() - 1)).right_aligned()
    ///     });
    /// ```
    #[inline]
    #[must_use = "method moves the value of self and returns the modified value"]
    pub fn with_title_bottom(
        mut self,
        title_bottom: impl Fn(&FileExplorer) -> Line<'static> + 'static + Send + Sync,
    ) -> Self {
        self.title_bottom.push(Arc::new(title_bottom));
        self
    }

    /// Returns the wrapping block (if it exist) of the file explorer of the theme.
    #[inline]
    #[must_use]
    pub const fn block(&self) -> Option<&Block<'static>> {
        self.block.as_ref()
    }

    /// Returns the style of the widget of the theme.
    #[inline]
    #[must_use]
    pub const fn style(&self) -> &Style {
        &self.style
    }

    /// Returns the style of the non directories items of the theme.
    #[inline]
    #[must_use]
    pub const fn item_style(&self) -> &Style {
        &self.item_style
    }

    /// Returns the style of the directories items of the theme.
    #[inline]
    #[must_use]
    pub const fn dir_style(&self) -> &Style {
        &self.dir_style
    }

    /// Returns the style of the highlighted non directories items of the theme.
    #[inline]
    #[must_use]
    pub const fn highlight_item_style(&self) -> &Style {
        &self.highlight_item_style
    }

    /// Returns the style of the highlighted directories items of the theme.
    #[inline]
    #[must_use]
    pub const fn highlight_dir_style(&self) -> &Style {
        &self.highlight_dir_style
    }

    /// Returns the symbol used to highlight the selected item of the theme.
    #[inline]
    #[must_use]
    pub fn highlight_symbol(&self) -> Option<&str> {
        self.highlight_symbol.as_deref()
    }

    /// Returns the spacing between the highlighted item and the other items of the theme.
    #[inline]
    #[must_use]
    pub const fn highlight_spacing(&self) -> &HighlightSpacing {
        &self.highlight_spacing
    }

    /// Returns the number of items around the currently selected item that should be kept visible.
    #[inline]
    #[must_use]
    pub const fn scroll_padding(&self) -> usize {
        self.scroll_padding
    }

    /// Returns the generated top titles of the theme.
    #[inline]
    #[must_use]
    pub fn title_top(&self, file_explorer: &FileExplorer) -> Vec<Line> {
        self.title_top
            .iter()
            .map(|title_top| title_top(file_explorer))
            .collect()
    }

    /// Returns the generated bottom titles of the theme.
    #[inline]
    #[must_use]
    pub fn title_bottom(&self, file_explorer: &FileExplorer) -> Vec<Line> {
        self.title_bottom
            .iter()
            .map(|title_bottom| title_bottom(file_explorer))
            .collect()
    }
}

impl Default for Theme {
    /// Return a slightly customized default theme. To get a theme with no style set, use [`Theme::new`](#method.new).
    ///
    /// The theme will have a block with all borders, a white style for the items, a light blue style for the directories,
    /// a dark gray background for all the highlighted items.
    ///
    /// # Example
    /// ```no_run
    /// # use ratatui_explorer::Theme;
    /// let theme = Theme::default();
    /// ```
    fn default() -> Self {
        Self {
            block: Some(Block::default().borders(Borders::ALL)),
            title_top: Vec::new(),
            title_bottom: Vec::new(),
            style: Style::default(),
            item_style: Style::default().fg(Color::White),
            dir_style: Style::default().fg(Color::LightBlue),
            highlight_spacing: HighlightSpacing::Always,
            highlight_item_style: Style::default().fg(Color::White).bg(Color::DarkGray),
            highlight_dir_style: Style::default().fg(Color::LightBlue).bg(Color::DarkGray),
            highlight_symbol: None,
            scroll_padding: 0,
        }
    }
}
