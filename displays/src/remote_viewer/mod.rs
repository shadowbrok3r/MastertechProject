use std::time::SystemTime;
use ratagui::BufferMessage;
use ratatui::buffer::Buffer;
use anyhow::Context;
use bincode::{config::*, serde::*};

pub mod ratagui;
pub mod terminal_line;
const ZSTD_LEVEL: i32 = 7;

pub fn encode_buffer(message: &Buffer) -> anyhow::Result<Vec<u8>> {
    let bincoded = encode_to_vec(message, standard()).context("Failed to serialize buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL).context("zstd")?;
    Ok(compressed.into())
}

// Helper to encode (frame_index, buffer) together
pub fn encode_buffer_with_frame(frame_index: u64, buffer: &Buffer) -> anyhow::Result<Vec<u8>> {
    let data = (frame_index, buffer);
    let bincoded = encode_to_vec(&data, standard()).context("Failed to serialize frame and buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL)
        .context("Failed to compress frame data")?;
    Ok(compressed.into())
}

// Updated encoding function
pub fn encode_buffer_with_timestamp(frame_count: u64, buffer: &Buffer) -> anyhow::Result<Vec<u8>, anyhow::Error> {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // let encode_start = Instant::now();
    let message = BufferMessage {
        timestamp,
        frame_count,
        encode_duration: 0, // Placeholder
        buffer: SerializableBuffer::from(buffer.clone()),
    };
    
    let bincoded = encode_to_vec(&message, standard()).context("Failed to serialize frame and buffer")?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL)
        .context("Failed to compress frame data")?;
    
    // let encode_duration = encode_start.elapsed().as_millis() as u64;
    // let updated_message = BufferMessage {
    //     timestamp,
    //     frame_count,
    //     encode_duration,
    //     buffer: buffer.clone(),
    // };
    // let updated_bincoded = serde_json::to_vec(&updated_message).context("Failed to serialize updated frame and buffer")?;
    // let updated_compressed = zstd::encode_all(std::io::Cursor::new(&updated_bincoded), ZSTD_LEVEL)
    //     .context("Failed to compress updated frame data")?;

    Ok(compressed.into())
}

pub fn decode_buffer(packet: &[u8]) -> anyhow::Result<BufferMessage> {
    let bincoded = zstd::decode_all(packet).context("zstd")?;
    let (message, _) = decode_from_slice(&bincoded, standard()).context("bincode")?;
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
use ratatui::buffer::Cell;
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
            skip: cell.skip,
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
        cell.set_skip(wrapper.skip);
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