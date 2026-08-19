use ratagui::{BufferMessage, TerminalEvent};
use ratatui::buffer::Buffer;
use anyhow::Context;
use bincode::{config::*, serde::*};
use crossbeam::channel::{Receiver, Sender};
use once_cell::sync::Lazy;

pub mod input_focus;
pub mod preboot;
pub mod ratagui;
pub mod terminal_line;

// ─── Global terminal input channel ────────────────────────────────────────────
//
// The admin's terminal viewer sends `TerminalEvent`s as untagged JSON over whatever transport the
// session negotiated. Only the WebSocket receive path ever decoded them, so on a direct-TCP or
// relay session every click and keystroke fell through to `try_deserialize_command` and was dropped
// as an undecodable `Cmd`. Both receive paths now push into this process-wide channel and
// `terminal_mode`'s draw loop drains it, the same shape as `plugins::egui_input_sender`.

static TERMINAL_INPUT_CHANNEL: Lazy<(Sender<TerminalEvent>, Receiver<TerminalEvent>)> =
    Lazy::new(crossbeam::channel::unbounded);

/// Sender for `TerminalEvent`s decoded off any transport. Call from a receive path.
pub fn terminal_input_sender() -> Sender<TerminalEvent> {
    TERMINAL_INPUT_CHANNEL.0.clone()
}

/// Drain all pending `TerminalEvent`s. Called from `terminal_mode`'s draw loop.
pub fn drain_terminal_inputs() -> impl Iterator<Item = TerminalEvent> {
    std::iter::from_fn(|| TERMINAL_INPUT_CHANNEL.1.try_recv().ok())
}

// zstd (C backend) doesn't build on wasm32; its only callers are the tokio
// live-terminal viewer, so the buffer codecs are gated to that feature.
#[cfg(feature = "tokio")]
const ZSTD_LEVEL: i32 = 3;

#[cfg(feature = "tokio")]
pub fn encode_buffer(message: &Buffer) -> anyhow::Result<Vec<u8>> {
    let bincoded = encode_to_vec(message, standard()).context("Failed to serialize buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL).context("zstd")?;
    Ok(compressed.into())
}

// Helper to encode (frame_index, buffer) together
#[cfg(feature = "tokio")]
pub fn encode_buffer_with_frame(frame_index: u64, buffer: &Buffer) -> anyhow::Result<Vec<u8>> {
    let data = (frame_index, buffer);
    let bincoded = encode_to_vec(&data, standard()).context("Failed to serialize frame and buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL).context("Failed to compress frame data")?;
    Ok(compressed.into())
}

// Updated encoding function
#[cfg(feature = "tokio")]
pub fn encode_buffer_with_timestamp(frame_count: u64, buffer: &Buffer) -> anyhow::Result<Vec<u8>, anyhow::Error> {
    
    let timestamp = web_time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // let encode_start = std::time::Instant::now();
    
    let message = BufferMessage {
        timestamp,
        frame_count,
        encode_duration: 0, // Placeholder
        buffer: SerializableBuffer::from(buffer.clone()),
    };
    
    let bincoded = encode_to_vec(&message, standard()).context("Failed to serialize frame and buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL).context("Failed to compress frame data")?;
    
    Ok(compressed.into())
}

#[cfg(feature = "tokio")]
pub fn decode_buffer(packet: &[u8]) -> anyhow::Result<BufferMessage> {
    let bincoded = zstd::decode_all(packet).context("zstd")?;
    let (message, _) = decode_from_slice(&bincoded, tcp_protocol::WIRE_DECODE).context("bincode")?;
    Ok(message)
}



#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct SerializableColor(ratatui::style::Color);

impl From<ratatui::style::Color> for SerializableColor {
    fn from(color: ratatui::style::Color) -> Self {
        SerializableColor(color)
    }
}

impl From<SerializableColor> for ratatui::style::Color {
    fn from(wrapper: SerializableColor) -> Self {
        wrapper.0
    }
}

impl SerializableColor {
    pub fn inner(&self) -> ratatui::style::Color {
        self.0
    }
}
impl Serialize for SerializableColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Use a struct to ensure fixed serialization
        #[derive(Serialize)]
        struct ColorData {
            discriminant: u8,
            data: [u8; 3], // Max: r, g, b for Rgb
        }

        let (discriminant, data) = match self.0 {
            Color::Reset => (0, [0; 3]),
            Color::Black => (1, [0; 3]),
            Color::Red => (2, [0; 3]),
            Color::Green => (3, [0; 3]),
            Color::Yellow => (4, [0; 3]),
            Color::Blue => (5, [0; 3]),
            Color::Magenta => (6, [0; 3]),
            Color::Cyan => (7, [0; 3]),
            Color::Gray => (8, [0; 3]),
            Color::DarkGray => (9, [0; 3]),
            Color::LightRed => (10, [0; 3]),
            Color::LightGreen => (11, [0; 3]),
            Color::LightYellow => (12, [0; 3]),
            Color::LightBlue => (13, [0; 3]),
            Color::LightMagenta => (14, [0; 3]),
            Color::LightCyan => (15, [0; 3]),
            Color::White => (16, [0; 3]),
            Color::Rgb(r, g, b) => (17, [r, g, b]),
            Color::Indexed(i) => (18, [i, 0, 0]),
        };

        ColorData { discriminant, data }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ColorData {
            discriminant: u8,
            data: [u8; 3],
        }

        let color_data = ColorData::deserialize(deserializer)?;
        let color = match color_data.discriminant {
            0 => Color::Reset,
            1 => Color::Black,
            2 => Color::Red,
            3 => Color::Green,
            4 => Color::Yellow,
            5 => Color::Blue,
            6 => Color::Magenta,
            7 => Color::Cyan,
            8 => Color::Gray,
            9 => Color::DarkGray,
            10 => Color::LightRed,
            11 => Color::LightGreen,
            12 => Color::LightYellow,
            13 => Color::LightBlue,
            14 => Color::LightMagenta,
            15 => Color::LightCyan,
            16 => Color::White,
            17 => Color::Rgb(color_data.data[0], color_data.data[1], color_data.data[2]),
            18 => Color::Indexed(color_data.data[0]),
            _ => return Err(serde::de::Error::custom("invalid color discriminant")),
        };
        Ok(SerializableColor(color))
    }
}
use ratatui::buffer::{Cell, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Debug, Default)]
pub struct SerializableCell {
    symbol: compact_str::CompactString,
    fg: SerializableColor,
    bg: SerializableColor,
    underline_color: SerializableColor,
    modifier: Modifier,
    skip: bool,
}

impl From<Cell> for SerializableCell {
    fn from(cell: Cell) -> Self {
        SerializableCell {
            symbol: cell.symbol().into(),
            fg: cell.fg.into(),
            bg: cell.bg.into(),
            underline_color: cell.underline_color.into(),
            modifier: cell.modifier,
            skip: cell.diff_option == CellDiffOption::Skip,
        }
    }
}

impl From<SerializableCell> for Cell {
    fn from(wrapper: SerializableCell) -> Self {
        let mut cell = Cell::default();
        cell.set_fg(wrapper.fg.into());
        cell.set_bg(wrapper.bg.into());
        cell.underline_color = wrapper.underline_color.into();
        cell.modifier = wrapper.modifier;
        cell.set_diff_option(if wrapper.skip { CellDiffOption::Skip } else { CellDiffOption::None });
        cell.set_symbol(&wrapper.symbol);

        cell
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Debug, Default)]
pub struct SerializableBuffer {
    pub area: Rect,
    pub content: Vec<SerializableCell>,
}

impl From<Buffer> for SerializableBuffer {
    fn from(buffer: Buffer) -> Self {
        SerializableBuffer {
            area: buffer.area,
            content: buffer.content.into_iter().map(SerializableCell::from).collect(),
        }
    }
}

impl From<SerializableBuffer> for Buffer {
    fn from(wrapper: SerializableBuffer) -> Self {
        Buffer {
            area: wrapper.area,
            content: wrapper.content.into_iter().map(Cell::from).collect(),
        }
    }
}