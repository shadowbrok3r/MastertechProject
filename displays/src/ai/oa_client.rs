use crate::openai::{Client, config::OpenAIConfig};
use std::sync::Arc;
use super::{GEMINI_API_KEY, GEMINI_API_BASE};
pub type OaClient = Arc<Client<OpenAIConfig>>;

pub fn new_oa_client() -> anyhow::Result<OaClient, anyhow::Error> {
	let config = OpenAIConfig::new()
		.with_api_key(GEMINI_API_KEY)
		.with_api_base(GEMINI_API_BASE);
	Ok(Client::with_config(config).into())
}
