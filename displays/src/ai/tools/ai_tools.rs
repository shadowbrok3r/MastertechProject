use crate::openai::types::chat::{ChatCompletionTool, ChatCompletionTools};
use rpc_router::Router;
use std::sync::Arc;

#[derive(Clone)]
pub struct AiTools {
	router: Router,
	chat_tools: Arc<Vec<ChatCompletionTool>>,
}

impl AiTools {
	pub fn new(router: Router, chat_tools: Vec<ChatCompletionTool>) -> Self {
		AiTools {
			router,
			chat_tools: Arc::new(chat_tools),
		}
	}
}

impl AiTools {
	pub fn router(&self) -> &Router {
		&self.router
	}

	/// async-openai 0.38 split chat tools into the `ChatCompletionTools` enum
	/// (Function / Custom). We only ship function tools, so wrap each on the way out.
	pub fn chat_tools_clone(&self) -> Vec<ChatCompletionTools> {
		self.chat_tools
			.as_ref()
			.iter()
			.cloned()
			.map(ChatCompletionTools::Function)
			.collect()
	}
}
