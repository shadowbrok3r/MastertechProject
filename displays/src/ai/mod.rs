pub(crate) const GEMINI_API_KEY: &str = env!("GEMINI_API_KEY");
pub(crate) const GEMINI_API_BASE: &str = "https://openrouter.ai/api/v1";

// region:    --- Modules

pub mod chat;
pub mod conv;
pub mod gpts;
pub mod model;
pub mod oa_client;
pub mod tool_call;
pub mod tools;
pub mod utils;

// endregion: --- Modules
