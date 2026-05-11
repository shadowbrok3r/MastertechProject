#![allow(non_snake_case)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports, unused_variables))]
//! GitHub issue creation via REST API (no token for this public repo).

pub async fn create_new_issue(
    title: String,
    body: String,
    client: reqwest::Client,
) -> anyhow::Result<String, anyhow::Error> {
    displays::tabs::github::create_new_issue(title, body, client).await
}
