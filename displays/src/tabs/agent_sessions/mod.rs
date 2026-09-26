//! Codex agent event rendering shared by the Ai tab and the terminal assistant: the flattened chat
//! line for tool, shell and file-change rows, the structured tool call, and the full transcript view.

mod transcript;

pub(crate) use transcript::chat_line;
pub use transcript::{ToolCall, transcript_ui};
