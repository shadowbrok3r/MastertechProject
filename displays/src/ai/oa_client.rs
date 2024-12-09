use crate::openai::{Client, config::OpenAIConfig};
use std::sync::Arc;
use super::OPENAI_API_KEY;
pub type OaClient = Arc<Client<OpenAIConfig>>;

pub fn new_oa_client() -> anyhow::Result<OaClient, anyhow::Error> {
	let config = OpenAIConfig::new().with_api_key(OPENAI_API_KEY);
	Ok(Client::with_config(config).into())
}
