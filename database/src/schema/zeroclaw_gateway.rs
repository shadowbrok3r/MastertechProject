//! The ZeroClaw gateway address and token Root users read to browse agents and automations.

use serde::{Deserialize, Serialize};

use super::SurrealValue;
use crate::db;

pub const ZEROCLAW_GATEWAY_TABLE: &str = "zeroclaw_gateway";

/// The one gateway row; empty for anyone but an active Root user.
pub const FETCH_GATEWAY_SQL: &str = "SELECT url, token FROM zeroclaw_gateway:shop";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, SurrealValue)]
pub struct ZeroclawGateway {
    pub url: String,
    pub token: String,
}

impl std::fmt::Debug for ZeroclawGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZeroclawGateway").field("url", &self.url).field("token", &"<redacted>").finish()
    }
}

impl ZeroclawGateway {
    /// The gateway the signed-in user may browse; `None` unless they are an active Root user.
    pub async fn fetch() -> anyhow::Result<Option<Self>> {
        let rows: Vec<Self> = db().query(FETCH_GATEWAY_SQL).await?.take(0)?;
        Ok(rows.into_iter().next().filter(|g| !g.url.trim().is_empty() && !g.token.trim().is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_shows_the_token() {
        let gateway = ZeroclawGateway { url: "https://zc.example".into(), token: "zc_secret".into() };
        let shown = format!("{gateway:?}");
        assert!(shown.contains("https://zc.example"));
        assert!(!shown.contains("zc_secret"));
    }
}
