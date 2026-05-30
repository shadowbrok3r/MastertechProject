use crate::openai::{Client, config::OpenAIConfig};
use std::sync::Arc;
use super::{effective_api_base, effective_api_key};
pub type OaClient = Arc<Client<OpenAIConfig>>;

pub fn new_oa_client() -> anyhow::Result<OaClient, anyhow::Error> {
	let config = OpenAIConfig::new()
		.with_api_key(effective_api_key())
		.with_api_base(effective_api_base());
	Ok(Client::with_config(config).into())
}
